use crate::{
    piano::{Piano, RATE},
    settings::Voice,
    win::OutputError,
};
use std::{
    collections::VecDeque,
    ptr::null_mut,
    slice,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Devices::FunctionDiscovery::PKEY_Device_FriendlyName,
        Foundation::RPC_E_CHANGED_MODE,
        Media::Audio::{
            AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_NOPERSIST,
            AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, DEVICE_STATE_ACTIVE, EDataFlow,
            IAudioCaptureClient, IAudioClient, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator,
            MMDeviceEnumerator, WAVE_FORMAT_PCM, WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eCapture,
            eRender,
        },
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoTaskMemFree, CoUninitialize, STGM_READ, StructuredStorage::PropVariantToStringAlloc,
        },
    },
    core::{GUID, HSTRING, PWSTR, Result},
};

const QUEUED_FRAMES: u32 = RATE * 16 / 1000;
const RELEASE_TAIL: Duration = Duration::from_millis(850);

pub struct Endpoint {
    pub id: String,
    pub name: String,
}

impl Endpoint {
    pub fn is_virtual_cable(&self) -> bool {
        cable_device(&self.name)
    }
}

#[derive(Default)]
pub struct Devices {
    pub destinations: Vec<Endpoint>,
    pub microphones: Vec<Endpoint>,
}

impl Devices {
    pub fn scan() -> Result<Self> {
        let _com = Apartment::enter()?;
        let devices = enumerator()?;
        let destinations = endpoints(&devices, eRender)?;
        let mut microphones = endpoints(&devices, eCapture)?;
        microphones.retain(|device| !device.is_virtual_cable());
        Ok(Self {
            destinations,
            microphones,
        })
    }

    pub fn cable(&self) -> Option<&Endpoint> {
        self.destinations
            .iter()
            .find(|device| cable_render(&device.name))
    }
}

fn cable_render(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("cable input (") && name.contains("vb-audio")
}

fn cable_device(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("cable") && name.contains("vb-audio")
}

struct Apartment(bool);

impl Apartment {
    fn enter() -> Result<Self> {
        let status = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if status == RPC_E_CHANGED_MODE {
            return Ok(Self(false));
        }
        status.ok()?;
        Ok(Self(true))
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

fn enumerator() -> Result<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER) }
}

fn endpoints(devices: &IMMDeviceEnumerator, flow: EDataFlow) -> Result<Vec<Endpoint>> {
    unsafe {
        let collection = devices.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE)?;
        Ok((0..collection.GetCount()?)
            .filter_map(|index| {
                let device = collection.Item(index).ok()?;
                Some(Endpoint {
                    id: take_string(device.GetId().ok()?),
                    name: device_name(&device).ok()?,
                })
            })
            .collect())
    }
}

fn device_name(device: &IMMDevice) -> Result<String> {
    unsafe {
        let properties = device.OpenPropertyStore(STGM_READ)?;
        let value = properties.GetValue(&PKEY_Device_FriendlyName)?;
        Ok(take_string(PropVariantToStringAlloc(&value)?))
    }
}

unsafe fn take_string(value: PWSTR) -> String {
    let text = unsafe { value.to_string() }.unwrap_or_default();
    unsafe { CoTaskMemFree(Some(value.0.cast())) };
    text
}

fn client(device: &IMMDevice) -> Result<IAudioClient> {
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: 1,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * 2,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    unsafe {
        let client: IAudioClient = device.Activate(CLSCTX_INPROC_SERVER, None)?;
        client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY
                | AUDCLNT_STREAMFLAGS_NOPERSIST,
            400_000,
            0,
            &format,
            None,
        )?;
        Ok(client)
    }
}

struct RenderStream {
    render: IAudioRenderClient,
    output: IAudioClient,
    capacity: u32,
}

impl RenderStream {
    fn open(device: &IMMDevice) -> Result<Self> {
        let output = client(device)?;
        unsafe {
            let render: IAudioRenderClient = output.GetService()?;
            let capacity = output.GetBufferSize()?;
            Ok(Self {
                render,
                output,
                capacity,
            })
        }
    }

    fn start(&self) -> Result<()> {
        let frames = self.capacity.min(RATE / 200);
        unsafe {
            self.render.GetBuffer(frames)?;
            self.render
                .ReleaseBuffer(frames, AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)?;
            self.output.Start()
        }
    }

    fn stop(&self) -> Result<()> {
        unsafe {
            self.output.Stop()?;
            self.output.Reset()
        }
    }

    fn write(&self, mut sample: impl FnMut() -> i16) -> Result<()> {
        let padding = unsafe { self.output.GetCurrentPadding()? };
        let frames = self.capacity.min(QUEUED_FRAMES).saturating_sub(padding);
        if frames == 0 {
            return Ok(());
        }
        let buffer = unsafe {
            let data = self.render.GetBuffer(frames)?;
            slice::from_raw_parts_mut(data.cast::<i16>(), frames as usize)
        };
        for frame in buffer {
            *frame = sample();
        }
        unsafe { self.render.ReleaseBuffer(frames, 0) }
    }
}

impl Drop for RenderStream {
    fn drop(&mut self) {
        unsafe {
            let _ = self.output.Stop();
        }
    }
}

pub struct Stream {
    output: RenderStream,
    destination: String,
    monitor: Option<Monitor>,
    monitor_id: String,
    monitor_unavailable: bool,
    mic: Option<Microphone>,
    piano: Piano,
    limit: f32,
    active: bool,
    notes_active: bool,
    tail_until: Option<Instant>,
    _com: Apartment,
}

impl Stream {
    pub fn open(settings: &Voice) -> std::result::Result<Self, OutputError> {
        let _com = Apartment::enter().map_err(|_| OutputError::VoiceUnavailable)?;
        let devices = enumerator().map_err(|_| OutputError::VoiceUnavailable)?;
        let auto_id;
        let destination = if settings.destination.is_empty() {
            auto_id = endpoints(&devices, eRender)
                .map_err(|_| OutputError::VoiceUnavailable)?
                .into_iter()
                .find(|device| cable_render(&device.name))
                .ok_or(OutputError::VoiceCableMissing)?
                .id;
            auto_id.as_str()
        } else {
            &settings.destination
        };
        let device = unsafe { devices.GetDevice(&HSTRING::from(destination)) }
            .map_err(|_| OutputError::VoiceUnavailable)?;
        let piano = Piano::new().ok_or(OutputError::VoiceSamplesUnavailable)?;
        let mic = if settings.microphone.is_empty() {
            None
        } else {
            Some(
                Microphone::open(&devices, &settings.microphone)
                    .map_err(|_| OutputError::VoiceMicUnavailable)?,
            )
        };
        let output = RenderStream::open(&device).map_err(|_| OutputError::VoiceUnavailable)?;
        let monitor = Self::open_monitor(&devices, destination, &settings.monitor);
        Ok(Self {
            output,
            destination: destination.to_owned(),
            monitor_unavailable: monitor.is_err(),
            monitor: monitor.ok().flatten(),
            monitor_id: settings.monitor.clone(),
            mic,
            piano,
            limit: 1.0,
            active: false,
            notes_active: false,
            tail_until: None,
            _com,
        })
    }

    fn open_monitor(
        devices: &IMMDeviceEnumerator,
        destination: &str,
        monitor: &str,
    ) -> Result<Option<Monitor>> {
        if monitor.is_empty() || monitor == destination {
            return Ok(None);
        }
        Monitor::open(devices, monitor).map(Some)
    }

    pub fn piano(&mut self) -> &mut Piano {
        &mut self.piano
    }

    pub fn start(&mut self, settings: &Voice) -> std::result::Result<bool, OutputError> {
        if self.notes_active {
            return Ok(false);
        }
        if self.monitor_unavailable {
            self.monitor_id.clear();
        }
        self.activate(settings)?;
        self.notes_active = true;
        self.tail_until = None;
        Ok(true)
    }

    fn activate(&mut self, settings: &Voice) -> std::result::Result<(), OutputError> {
        self.sync_monitor(settings);
        if self.mic.is_none() && !settings.microphone.is_empty() {
            let devices = enumerator().map_err(|_| OutputError::VoiceMicUnavailable)?;
            self.mic = Some(
                Microphone::open(&devices, &settings.microphone)
                    .map_err(|_| OutputError::VoiceMicUnavailable)?,
            );
        }
        if !self.active {
            self.output
                .start()
                .map_err(|_| OutputError::VoiceUnavailable)?;
            self.start_monitor();
        }
        if let Some(mic) = &mut self.mic
            && !mic.running
        {
            mic.start().map_err(|_| OutputError::VoiceMicUnavailable)?;
        }
        self.active = true;
        Ok(())
    }

    fn start_monitor(&mut self) {
        if let Some(monitor) = &self.monitor
            && monitor.output.start().is_err()
        {
            self.monitor = None;
            self.monitor_unavailable = true;
        }
    }

    pub fn finish(&mut self, keep_mic_live: bool) {
        if !self.notes_active {
            return;
        }
        if !keep_mic_live {
            self.stop_mic();
        }
        self.notes_active = false;
        self.piano.finish();
        self.tail_until = Some(Instant::now() + RELEASE_TAIL);
    }

    fn stop_mic(&mut self) {
        if let Some(mic) = &mut self.mic
            && mic.stop().is_err()
        {
            self.mic = None;
        }
    }

    pub fn reset(&mut self) -> Result<()> {
        if self.active {
            if let Some(mic) = &mut self.mic {
                mic.stop()?;
            }
            self.output.stop()?;
            if let Some(monitor) = &mut self.monitor {
                let stopped = monitor.output.stop().is_ok();
                monitor.queue.clear();
                monitor.limit = 1.0;
                if !stopped {
                    self.monitor = None;
                    self.monitor_unavailable = true;
                }
            }
        }
        self.active = false;
        self.notes_active = false;
        self.tail_until = None;
        self.piano.reset();
        self.limit = 1.0;
        Ok(())
    }

    pub fn mic_live(&self) -> bool {
        self.mic.as_ref().is_some_and(|mic| mic.running)
    }

    pub fn mic_peak(&self) -> f32 {
        self.mic.as_ref().map_or(0.0, |mic| mic.peak)
    }

    pub fn monitor_unavailable(&self) -> bool {
        self.monitor_unavailable
    }

    fn sync_monitor(&mut self, settings: &Voice) {
        if settings.monitor != self.monitor_id {
            self.monitor_id.clone_from(&settings.monitor);
            let result = enumerator().and_then(|devices| {
                Self::open_monitor(&devices, &self.destination, &settings.monitor)
            });
            self.monitor_unavailable = result.is_err();
            self.monitor = result.ok().flatten();
            if self.active {
                self.start_monitor();
            }
        }
    }

    pub fn pump(&mut self, settings: &Voice) -> std::result::Result<bool, OutputError> {
        let keep_mic = settings.keep_mic_live && !settings.microphone.is_empty();
        if keep_mic && !self.mic_live() {
            self.activate(settings)?;
        } else if !keep_mic && !self.notes_active && self.mic_live() {
            self.stop_mic();
        }
        if !self.active {
            return Ok(false);
        }
        if self.tail_until.is_some_and(|until| Instant::now() >= until) {
            self.tail_until = None;
            self.piano.reset();
        }
        if !self.notes_active && self.tail_until.is_none() && !self.mic_live() {
            self.reset().map_err(|_| OutputError::VoiceUnavailable)?;
            return Ok(false);
        }
        self.sync_monitor(settings);
        if let Some(mic) = &mut self.mic
            && mic.running
        {
            mic.read().map_err(|_| OutputError::VoiceMicUnavailable)?;
        }
        self.render_frames(settings)
            .map_err(|_| OutputError::VoiceUnavailable)?;
        if let Some(monitor) = &mut self.monitor
            && monitor.pump(settings.monitor_level / 100.0).is_err()
        {
            self.monitor = None;
            self.monitor_unavailable = true;
        }
        Ok(true)
    }

    fn render_frames(&mut self, settings: &Voice) -> Result<()> {
        let piano_gain = settings.piano_level / 100.0;
        let mic_gain = settings.mic_level / 100.0;
        let room = settings.room / 100.0;
        let mut mic = self.mic.as_mut().filter(|mic| mic.running);
        self.output.write(|| {
            let piano = self.piano.frame(room);
            if let Some(monitor) = &mut self.monitor {
                monitor.queue.samples.push_back(piano);
            }
            let mic = mic.as_deref_mut().map_or(0.0, Microphone::frame);
            pcm(piano * piano_gain + mic * mic_gain, &mut self.limit)
        })
    }
}

fn pcm(sample: f32, limit: &mut f32) -> i16 {
    *limit = (*limit + (1.0 - *limit) * 0.0003).min(0.92 / sample.abs().max(0.92));
    (sample * *limit * f32::from(i16::MAX)) as i16
}

struct Monitor {
    output: RenderStream,
    queue: SampleQueue,
    limit: f32,
}

impl Monitor {
    fn open(devices: &IMMDeviceEnumerator, id: &str) -> Result<Self> {
        let device = unsafe { devices.GetDevice(&HSTRING::from(id))? };
        if cable_device(&device_name(&device)?) {
            return Err(windows::core::Error::from(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }
        Ok(Self {
            output: RenderStream::open(&device)?,
            queue: SampleQueue::new(RATE, 14),
            limit: 1.0,
        })
    }

    fn pump(&mut self, gain: f32) -> Result<()> {
        self.queue.update();
        self.output
            .write(|| pcm(self.queue.frame().unwrap_or(0.0) * gain, &mut self.limit))
    }
}

struct SampleQueue {
    samples: VecDeque<f32>,
    rate: f32,
    target: usize,
    step: f32,
    phase: f32,
    started: bool,
}

impl SampleQueue {
    fn new(rate: u32, latency_ms: u32) -> Self {
        let target = (rate * latency_ms / 1000) as usize;
        Self {
            samples: VecDeque::with_capacity(target * 4 + 2),
            rate: rate as f32,
            target,
            step: rate as f32 / RATE as f32,
            phase: 0.0,
            started: false,
        }
    }

    fn clear(&mut self) {
        self.samples.clear();
        self.phase = 0.0;
        self.started = false;
    }

    fn update(&mut self) {
        if self.samples.len() > self.target * 4 {
            self.samples.drain(..self.samples.len() - self.target);
            self.phase = 0.0;
        }
        if self.samples.len() >= self.target {
            self.started = true;
        }
        // The devices can have slightly different hardware clocks.
        let error = (self.samples.len() as f32 - self.target as f32) / self.rate;
        self.step = self.rate / RATE as f32 * (1.0 + (error * 0.3).clamp(-0.005, 0.005));
    }

    fn frame(&mut self) -> Option<f32> {
        if !self.started || self.samples.len() < 2 {
            self.started = false;
            self.phase = 0.0;
            return None;
        }
        let sample = self.samples[0] * (1.0 - self.phase) + self.samples[1] * self.phase;
        self.phase += self.step;
        while self.phase >= 1.0 {
            self.samples.pop_front();
            self.phase -= 1.0;
        }
        Some(sample)
    }
}

const PCM_SUBFORMAT: GUID = GUID::from_u128(0x00000001_0000_0010_8000_00aa00389b71);
const FLOAT_SUBFORMAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

#[derive(Clone, Copy)]
enum Encoding {
    Pcm8,
    Pcm16,
    Pcm24,
    Pcm32,
    Float32,
    Float64,
}

impl Encoding {
    fn read(self, bytes: &[u8]) -> f32 {
        let value = match self {
            Self::Pcm8 => (f32::from(bytes[0]) - 128.0) / 128.0,
            Self::Pcm16 => f32::from(i16::from_le_bytes(bytes.try_into().unwrap())) / 32_768.0,
            Self::Pcm24 => {
                i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]) as f32 / 2_147_483_648.0
            }
            Self::Pcm32 => i32::from_le_bytes(bytes.try_into().unwrap()) as f32 / 2_147_483_648.0,
            Self::Float32 => f32::from_le_bytes(bytes.try_into().unwrap()),
            Self::Float64 => f64::from_le_bytes(bytes.try_into().unwrap()) as f32,
        };
        if value.is_finite() {
            value.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    }
}

struct CaptureFormat {
    encoding: Encoding,
    channels: usize,
    frame_bytes: usize,
    sample_bytes: usize,
    rate: u32,
}

impl CaptureFormat {
    unsafe fn read(format: *const WAVEFORMATEX) -> Option<Self> {
        if format.is_null() {
            return None;
        }
        let wave = unsafe { format.read_unaligned() };
        let subtype = match wave.wFormatTag {
            1 => PCM_SUBFORMAT,
            3 => FLOAT_SUBFORMAT,
            0xfffe if wave.cbSize >= 22 => {
                unsafe { format.cast::<WAVEFORMATEXTENSIBLE>().read_unaligned() }.SubFormat
            }
            _ => return None,
        };
        let bits = wave.wBitsPerSample;
        let encoding = match (subtype == PCM_SUBFORMAT, subtype == FLOAT_SUBFORMAT, bits) {
            (true, _, 8) => Encoding::Pcm8,
            (true, _, 16) => Encoding::Pcm16,
            (true, _, 24) => Encoding::Pcm24,
            (true, _, 32) => Encoding::Pcm32,
            (_, true, 32) => Encoding::Float32,
            (_, true, 64) => Encoding::Float64,
            _ => return None,
        };
        let channels = usize::from(wave.nChannels);
        let frame_bytes = usize::from(wave.nBlockAlign);
        let sample_bytes = usize::from(bits / 8);
        let rate = wave.nSamplesPerSec;
        if channels == 0
            || frame_bytes < channels * sample_bytes
            || !(8_000..=384_000).contains(&rate)
        {
            return None;
        }
        Some(Self {
            encoding,
            channels,
            frame_bytes,
            sample_bytes,
            rate,
        })
    }

    fn sample(&self, frame: &[u8], channel: usize) -> f32 {
        let start = channel * self.sample_bytes;
        self.encoding.read(&frame[start..start + self.sample_bytes])
    }

    fn active_channel(&self, data: &[u8], current: usize, power: &mut [f32]) -> usize {
        if self.channels == 1 {
            return 0;
        }
        // Some microphones carry speech on only one channel.
        power.fill(0.0);
        for frame in data.chunks_exact(self.frame_bytes) {
            for (channel, power) in power.iter_mut().enumerate() {
                let sample = self.sample(frame, channel);
                *power += sample * sample;
            }
        }
        match power
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.total_cmp(b))
        {
            Some((channel, energy)) if *energy > power[current] * 1.5 => channel,
            _ => current,
        }
    }
}

struct Microphone {
    capture: IAudioCaptureClient,
    input: IAudioClient,
    queue: SampleQueue,
    format: CaptureFormat,
    channel: usize,
    power: Vec<f32>,
    dc: f32,
    peak: f32,
    running: bool,
}

impl Microphone {
    fn open(devices: &IMMDeviceEnumerator, id: &str) -> Result<Self> {
        let device = unsafe { devices.GetDevice(&HSTRING::from(id))? };
        if cable_device(&device_name(&device)?) {
            return Err(windows::core::Error::from(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }
        let input: IAudioClient = unsafe { device.Activate(CLSCTX_INPROC_SERVER, None)? };
        let format = unsafe {
            let ptr = input.GetMixFormat()?;
            let format = CaptureFormat::read(ptr);
            let initialized = format.as_ref().map(|_| {
                input.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_NOPERSIST,
                    400_000,
                    0,
                    ptr,
                    None,
                )
            });
            CoTaskMemFree(Some(ptr.cast()));
            initialized.ok_or_else(|| {
                windows::core::Error::from(windows::Win32::Foundation::E_INVALIDARG)
            })??;
            format.unwrap()
        };
        let capture = unsafe { input.GetService()? };
        Ok(Self {
            capture,
            input,
            queue: SampleQueue::new(format.rate, 14),
            channel: 0,
            power: vec![0.0; format.channels],
            format,
            dc: 0.0,
            peak: 0.0,
            running: false,
        })
    }

    fn start(&mut self) -> Result<()> {
        unsafe { self.input.Start()? };
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if self.running {
            unsafe {
                self.input.Stop()?;
                self.input.Reset()?;
            }
            self.running = false;
            self.dc = 0.0;
            self.peak = 0.0;
            self.queue.clear();
        }
        Ok(())
    }

    fn read(&mut self) -> Result<()> {
        self.peak *= 0.965;
        unsafe {
            while self.capture.GetNextPacketSize()? != 0 {
                let (mut data, mut frames, mut flags) = (null_mut(), 0, 0);
                self.capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                    self.queue
                        .samples
                        .resize(self.queue.samples.len() + frames as usize, 0.0);
                } else {
                    let input =
                        slice::from_raw_parts(data, frames as usize * self.format.frame_bytes);
                    self.push(input);
                }
                self.capture.ReleaseBuffer(frames)?;
            }
        }
        self.queue.update();
        Ok(())
    }

    fn push(&mut self, data: &[u8]) {
        self.channel = self
            .format
            .active_channel(data, self.channel, &mut self.power);
        for frame in data.chunks_exact(self.format.frame_bytes) {
            let sample = self.format.sample(frame, self.channel);
            self.dc += (sample - self.dc) * (420.0 / self.format.rate as f32);
            let sample = sample - self.dc;
            self.peak = self.peak.max(sample.abs());
            self.queue.samples.push_back(sample);
        }
    }

    fn frame(&mut self) -> f32 {
        self.queue.frame().unwrap_or(0.0)
    }
}

impl Drop for Microphone {
    fn drop(&mut self) {
        unsafe {
            let _ = self.input.Stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_speech_from_the_active_channel_in_native_stereo() {
        let wave = WAVEFORMATEX {
            wFormatTag: 3,
            nChannels: 2,
            nSamplesPerSec: 44_100,
            nAvgBytesPerSec: 352_800,
            nBlockAlign: 8,
            wBitsPerSample: 32,
            cbSize: 0,
        };
        let format = unsafe { CaptureFormat::read(&wave) }.unwrap();
        let frames = [0.0_f32, 0.3, 0.0, -0.45]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect::<Vec<_>>();
        let channel = format.active_channel(&frames, 0, &mut [0.0; 2]);
        assert_eq!(channel, 1);
        assert_eq!(format.sample(&frames[8..], channel), -0.45);
        let mut queue = SampleQueue::new(format.rate, 10);
        queue.samples.resize(441, 0.25);
        queue.update();
        for _ in 0..470 {
            assert_eq!(queue.frame(), Some(0.25));
        }
    }
}
