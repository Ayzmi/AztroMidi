use crate::{
    humanize::Humanizer,
    keys,
    schedule::{Action, Event},
    settings::{Output, Voice},
    win::{self, OutputError},
};
use eframe::egui;
use std::sync::{
    Arc, Mutex,
    mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError},
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Stopped,
    Countdown,
    Playing,
    Paused,
}

#[derive(Clone, Copy)]
pub struct Snapshot {
    pub state: State,
    pub position: f64,
    pub length: f64,
    pub countdown: f64,
    pub waiting: bool,
    pub mic_live: bool,
    pub mic_peak: f32,
    pub monitor_unavailable: bool,
    pub live: bool,
    pub output_error: Option<OutputError>,
    pub index: usize,
    pub generation: u64,
    pub finished: u64,
    pub held: [bool; 128],
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            state: State::Stopped,
            position: 0.0,
            length: 0.0,
            countdown: 0.0,
            waiting: false,
            mic_live: false,
            mic_peak: 0.0,
            monitor_unavailable: false,
            live: false,
            output_error: None,
            index: 0,
            generation: 0,
            finished: 0,
            held: [false; 128],
        }
    }
}

impl Snapshot {
    pub fn playing(self) -> bool {
        matches!(self.state, State::Playing | State::Countdown)
    }

    pub fn status(self) -> &'static str {
        if self.output_error.is_some() {
            return "Output unavailable";
        }
        if self.live {
            return "Live piano";
        }
        match self.state {
            State::Stopped => "Ready",
            State::Countdown => "Starting",
            State::Playing if self.waiting => "Waiting for Roblox",
            State::Playing => "Playing",
            State::Paused => "Paused",
        }
    }
}

pub enum Command {
    Load {
        events: Arc<[Event]>,
        humanizer: Humanizer,
        generation: u64,
        preserve_position: bool,
    },
    Configure {
        output: Output,
        speed: f64,
        velocity: bool,
        voice: Voice,
    },
    Play(f64),
    Pause,
    Stop,
    Seek(f64),
    LiveKey {
        pitch: u8,
        down: bool,
    },
    EndLive,
    Shutdown,
}

pub struct Engine {
    sender: Sender<Command>,
    snapshot: Arc<Mutex<Snapshot>>,
    worker: Option<JoinHandle<()>>,
}

impl Engine {
    pub fn spawn(ctx: egui::Context) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::channel();
        let snapshot = Arc::new(Mutex::new(Snapshot::default()));
        let shared = snapshot.clone();
        let worker = thread::Builder::new()
            .name("AztroMidi Playback".into())
            .spawn(move || run(receiver, shared, ctx))?;
        Ok(Self {
            sender,
            snapshot,
            worker: Some(worker),
        })
    }

    pub fn send(&self, command: Command) {
        let _ = self.sender.send(command);
    }

    pub fn snapshot(&self) -> Snapshot {
        *self.snapshot.lock().unwrap()
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

trait Device {
    fn configure_voice(&mut self, _settings: Voice) -> bool {
        false
    }
    fn prepare(&mut self, _output: Output) -> Result<bool, OutputError> {
        Ok(false)
    }
    fn finish_voice(&mut self) {}
    fn warm_voice(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn close_voice(&mut self) {}
    fn ready(&self, output: Output) -> bool;
    fn release_pending(&self) -> bool;
    fn retry_release(&mut self);
    fn reset(&mut self, output: Output);
    fn press(
        &mut self,
        output: Output,
        pitch: u8,
        key: keys::Key,
        velocity: u8,
    ) -> Result<(), OutputError>;
    fn release(&mut self, output: Output, pitch: u8, key: keys::Key) -> Result<(), OutputError>;
    fn velocity(&mut self, key: keys::Key) -> Result<(), OutputError>;
    fn sustain(&mut self, output: Output, down: bool) -> Result<(), OutputError>;
}

impl Device for win::Outputs {
    fn configure_voice(&mut self, settings: Voice) -> bool {
        self.configure_voice(settings)
    }
    fn prepare(&mut self, output: Output) -> Result<bool, OutputError> {
        if output == Output::Voice {
            self.prepare_voice()
        } else {
            Ok(false)
        }
    }
    fn finish_voice(&mut self) {
        self.finish_voice();
    }
    fn warm_voice(&mut self) -> Result<(), OutputError> {
        self.warm_voice()
    }
    fn close_voice(&mut self) {
        self.close_voice();
    }
    fn ready(&self, output: Output) -> bool {
        output != Output::Keyboard || win::roblox_focused()
    }
    fn release_pending(&self) -> bool {
        self.waiting_for_release()
    }
    fn retry_release(&mut self) {
        self.retry_release();
    }
    fn reset(&mut self, output: Output) {
        self.reset(output);
    }
    fn press(
        &mut self,
        output: Output,
        pitch: u8,
        key: keys::Key,
        velocity: u8,
    ) -> Result<(), OutputError> {
        self.press(output, pitch, key, velocity)
    }
    fn release(&mut self, output: Output, pitch: u8, key: keys::Key) -> Result<(), OutputError> {
        self.release(output, pitch, key)
    }
    fn velocity(&mut self, key: keys::Key) -> Result<(), OutputError> {
        self.velocity(key)
    }
    fn sustain(&mut self, output: Output, down: bool) -> Result<(), OutputError> {
        self.sustain(output, down)
    }
}

struct Player {
    snapshot: Snapshot,
    events: Arc<[Event]>,
    human: Option<Humanizer>,
    output: Output,
    speed: f64,
    velocity_on: bool,
    last_bucket: Option<usize>,
    owners: [Option<usize>; 128],
    sustain: bool,
    restore_sustain: bool,
    last_tick: Instant,
    start_at: Option<Instant>,
    live_until: Option<Instant>,
}

impl Player {
    fn new() -> Self {
        Self {
            snapshot: Snapshot::default(),
            events: Arc::new([]),
            human: None,
            output: Output::Keyboard,
            speed: 1.0,
            velocity_on: false,
            last_bucket: None,
            owners: [None; 128],
            sustain: false,
            restore_sustain: false,
            last_tick: Instant::now(),
            start_at: None,
            live_until: None,
        }
    }

    fn clear(&mut self, device: &mut impl Device) {
        if self.output == Output::Voice
            || self.sustain
            || self.snapshot.held.iter().any(|held| *held)
            || device.release_pending()
        {
            device.reset(self.output);
        }
        self.snapshot.live = false;
        self.live_until = None;
        self.clear_keys();
    }

    fn end_live(&mut self, device: &mut impl Device) {
        if self.snapshot.live {
            device.finish_voice();
            self.snapshot.live = false;
            self.live_until = None;
            self.clear_keys();
        }
    }

    fn stop_notes(&mut self, device: &mut impl Device) {
        if self.output == Output::Voice {
            device.finish_voice();
            self.snapshot.live = false;
            self.live_until = None;
            self.clear_keys();
        } else {
            self.clear(device);
        }
    }

    fn live_key(&mut self, pitch: u8, down: bool, device: &mut impl Device) {
        if self.output != Output::Voice || self.snapshot.playing() {
            return;
        }
        let Some(key) = keys::lookup(pitch) else {
            return;
        };
        if self.snapshot.held[pitch as usize] == down {
            return;
        }
        let result = if down {
            device
                .prepare(Output::Voice)
                .and_then(|_| device.press(Output::Voice, pitch, key, 88))
        } else {
            device.release(Output::Voice, pitch, key)
        };
        if let Err(error) = result {
            self.clear(device);
            self.snapshot.output_error = Some(error);
            return;
        }
        self.snapshot.output_error = None;
        self.snapshot.held[pitch as usize] = down;
        self.snapshot.live = true;
        self.live_until = (!self.snapshot.held.iter().any(|held| *held))
            .then(|| Instant::now() + Duration::from_secs(3));
    }

    fn clear_keys(&mut self) {
        self.owners.fill(None);
        self.snapshot.held.fill(false);
        self.last_bucket = None;
        self.sustain = false;
        self.restore_sustain = true;
    }

    fn seek(&mut self, position: f64) {
        self.snapshot.position = position.clamp(0.0, self.snapshot.length);
        self.snapshot.index = self
            .events
            .partition_point(|event| event.time < self.snapshot.position);
        self.last_tick = Instant::now();
    }

    fn command(&mut self, command: Command, device: &mut impl Device) -> bool {
        match command {
            Command::Load {
                events,
                humanizer,
                generation,
                preserve_position,
            } => {
                let position = if preserve_position {
                    self.snapshot.position
                } else {
                    0.0
                };
                self.clear(device);
                self.snapshot.generation = generation;
                self.snapshot.length = events.last().map_or(0.0, |event| event.time);
                self.events = events;
                self.human = Some(humanizer);
                if !preserve_position || self.events.is_empty() {
                    self.snapshot.state = State::Stopped;
                    self.snapshot.waiting = false;
                    self.snapshot.countdown = 0.0;
                    self.start_at = None;
                }
                if !preserve_position {
                    self.snapshot.finished = 0;
                    self.snapshot.output_error = None;
                }
                self.seek(position);
            }
            Command::Configure {
                output,
                speed,
                velocity,
                voice,
            } => {
                let keep_mic_live = voice.keep_mic_live && !voice.microphone.is_empty();
                let voice_route_changed = device.configure_voice(voice);
                let mode_changed = self.output != output;
                if mode_changed || (output == Output::Voice && voice_route_changed) {
                    self.clear(device);
                    if mode_changed && (self.output == Output::Voice || output == Output::Voice) {
                        self.seek(0.0);
                        self.snapshot.state = State::Stopped;
                        self.snapshot.countdown = 0.0;
                        self.start_at = None;
                    }
                    self.output = output;
                    self.snapshot.waiting = false;
                    self.snapshot.output_error = None;
                    if mode_changed && output != Output::Voice {
                        device.close_voice();
                    }
                }
                if output == Output::Voice && (mode_changed || voice_route_changed || keep_mic_live)
                {
                    match device.warm_voice() {
                        Ok(()) if keep_mic_live => self.snapshot.output_error = None,
                        Err(error) if keep_mic_live => self.snapshot.output_error = Some(error),
                        _ => {}
                    }
                }
                self.speed = speed;
                self.velocity_on = velocity;
            }
            Command::Play(countdown) if !self.events.is_empty() => {
                self.end_live(device);
                self.snapshot.output_error = None;
                if self.snapshot.index >= self.events.len() {
                    self.seek(0.0);
                }
                self.last_tick = Instant::now();
                if countdown > 0.0 && self.snapshot.state == State::Stopped {
                    self.start_at = Some(self.last_tick + Duration::from_secs_f64(countdown));
                    self.snapshot.countdown = countdown;
                    self.snapshot.state = State::Countdown;
                } else {
                    self.snapshot.state = State::Playing;
                    self.start_at = None;
                }
            }
            Command::Pause => {
                self.stop_notes(device);
                if self.snapshot.playing() {
                    self.snapshot.state = State::Paused;
                }
                self.start_at = None;
                self.snapshot.countdown = 0.0;
                self.snapshot.waiting = false;
            }
            Command::Stop => {
                self.stop_notes(device);
                self.seek(0.0);
                self.snapshot.state = State::Stopped;
                self.snapshot.output_error = None;
                self.snapshot.waiting = false;
                self.snapshot.countdown = 0.0;
                self.start_at = None;
            }
            Command::Seek(position) => {
                self.clear(device);
                self.seek(position);
            }
            Command::LiveKey { pitch, down } => self.live_key(pitch, down, device),
            Command::EndLive => self.end_live(device),
            Command::Shutdown => {
                self.clear(device);
                return false;
            }
            Command::Play(_) => {}
        }
        true
    }

    fn tick(&mut self, now: Instant, device: &mut impl Device) -> Option<(Duration, bool)> {
        device.retry_release();
        let mut dt = now.saturating_duration_since(self.last_tick).as_secs_f64();
        self.last_tick = now;
        if let Some(start_at) = self.start_at {
            if now < start_at {
                self.snapshot.countdown = (start_at - now).as_secs_f64();
                return Some((Duration::from_millis(16).min(start_at - now), false));
            }
            self.start_at = None;
            self.snapshot.countdown = 0.0;
            self.snapshot.state = State::Playing;
            return Some((Duration::ZERO, false));
        }
        if self.snapshot.state != State::Playing {
            if self.live_until.is_some_and(|until| now >= until) {
                self.end_live(device);
            }
            if let Some(until) = self.live_until {
                return Some((until.saturating_duration_since(now), false));
            }
            return device
                .release_pending()
                .then_some((Duration::from_millis(40), false));
        }
        if !device.ready(self.output) {
            if !self.snapshot.waiting {
                self.snapshot.waiting = true;
                self.clear(device);
            }
            return Some((Duration::from_millis(30), false));
        }

        match device.prepare(self.output) {
            Ok(true) => {
                self.last_tick = Instant::now();
                dt = 0.0;
            }
            Ok(false) => {}
            Err(error) => return self.output_failed(error, device),
        }
        if let Err(error) = self.restore_pedal(device) {
            return self.output_failed(error, device);
        }
        let factor = self.speed
            * self
                .human
                .as_ref()
                .map_or(1.0, |h| h.tempo_factor(self.snapshot.position));
        if self.snapshot.waiting {
            self.snapshot.waiting = false;
        } else {
            self.snapshot.position =
                (self.snapshot.position + dt * factor).min(self.snapshot.length);
        }
        while let Some(event) = self.events.get(self.snapshot.index).copied() {
            if event.time > self.snapshot.position {
                let gap = (event.time - self.snapshot.position) / factor;
                return Some((Duration::from_secs_f64(gap.min(0.014)), gap <= 0.014));
            }
            if let Err(error) = self.dispatch(event, device) {
                return self.output_failed(error, device);
            }
            self.snapshot.index += 1;
        }
        self.stop_notes(device);
        self.snapshot.state = State::Stopped;
        self.snapshot.finished += 1;
        device
            .release_pending()
            .then_some((Duration::from_millis(40), false))
    }

    fn restore_pedal(&mut self, device: &mut impl Device) -> Result<(), OutputError> {
        if self.restore_sustain {
            let previous = self.events[..self.snapshot.index]
                .iter()
                .rev()
                .find_map(|event| match event.action {
                    Action::PedalDown => Some(true),
                    Action::PedalUp => Some(false),
                    _ => None,
                });
            if previous == Some(true) {
                device.sustain(self.output, true)?;
                self.sustain = true;
            }
            self.restore_sustain = false;
        }
        Ok(())
    }

    fn output_failed(
        &mut self,
        error: OutputError,
        device: &mut impl Device,
    ) -> Option<(Duration, bool)> {
        self.clear(device);
        if error == OutputError::FocusLost {
            self.snapshot.waiting = true;
            return Some((Duration::from_millis(30), false));
        }
        self.snapshot.waiting = false;
        self.snapshot.state = State::Paused;
        self.snapshot.output_error = Some(error);
        device
            .release_pending()
            .then_some((Duration::from_millis(40), false))
    }

    fn dispatch(&mut self, event: Event, device: &mut impl Device) -> Result<(), OutputError> {
        let pitch = event.pitch as usize;
        match event.action {
            Action::Press => {
                if let Some(key) = keys::lookup(event.pitch) {
                    if self.owners[pitch].is_some() {
                        device.release(self.output, event.pitch, key)?;
                    }
                    if self.velocity_on && self.output == Output::Keyboard {
                        let bucket = keys::velocity_bucket(event.velocity);
                        if self.last_bucket != Some(bucket) {
                            device.velocity(keys::velocity_key(bucket))?;
                            self.last_bucket = Some(bucket);
                        }
                    }
                    device.press(self.output, event.pitch, key, event.velocity)?;
                    self.owners[pitch] = Some(event.id);
                    self.snapshot.held[pitch] = true;
                }
            }
            Action::Release if self.owners[pitch] == Some(event.id) => {
                if let Some(key) = keys::lookup(event.pitch) {
                    device.release(self.output, event.pitch, key)?;
                }
                self.owners[pitch] = None;
                self.snapshot.held[pitch] = false;
            }
            Action::PedalDown | Action::PedalUp => {
                let down = event.action == Action::PedalDown;
                if self.sustain != down {
                    device.sustain(self.output, down)?;
                    self.sustain = down;
                }
            }
            Action::Release => {}
        }
        Ok(())
    }
}

fn run(receiver: Receiver<Command>, shared: Arc<Mutex<Snapshot>>, ctx: egui::Context) {
    let mut player = Player::new();
    let mut device = win::Outputs::new();
    let mut timer = None;
    let mut last_update = Instant::now();
    let mut previous = Snapshot::default();
    let mut command = None;
    loop {
        let changed = command.is_some();
        if let Some(cmd) = command.take()
            && !player.command(cmd, &mut device)
        {
            break;
        }
        if (player.snapshot.playing() || player.snapshot.live) && timer.is_none() {
            timer = Some(win::TimerResolution::new());
        }
        let now = Instant::now();
        let (mut deadline, mut precise) = player
            .tick(now, &mut device)
            .map_or((None, false), |(wait, precise)| (Some(now + wait), precise));
        let audio_active = match device.pump_voice() {
            Ok(active) => active,
            Err(error) => {
                if player.snapshot.playing() {
                    let _ = player.output_failed(error, &mut device);
                } else {
                    player.clear(&mut device);
                    player.snapshot.output_error = Some(error);
                }
                false
            }
        };
        if audio_active {
            if timer.is_none() {
                timer = Some(win::TimerResolution::new());
            }
            let audio_tick = now + Duration::from_millis(5);
            if deadline.is_none_or(|deadline| deadline > audio_tick) {
                deadline = Some(audio_tick);
                precise = false;
            }
        }
        if !player.snapshot.playing() && !audio_active {
            timer = None;
        }
        player.snapshot.mic_live = device.mic_live();
        player.snapshot.mic_peak = device.mic_peak();
        player.snapshot.monitor_unavailable = device.monitor_unavailable();
        let status = player.snapshot;
        let state_changed = previous.state != status.state
            || previous.waiting != status.waiting
            || previous.mic_live != status.mic_live
            || previous.monitor_unavailable != status.monitor_unavailable
            || previous.live != status.live
            || previous.output_error != status.output_error
            || previous.generation != status.generation
            || previous.finished != status.finished;
        if changed || state_changed || now - last_update >= Duration::from_millis(16) {
            *shared.lock().unwrap() = status;
            previous = status;
            last_update = now;
            ctx.request_repaint();
        }
        command = match next_command(&receiver, deadline, precise) {
            Ok(command) => command,
            Err(()) => break,
        };
    }
}

fn next_command(
    receiver: &Receiver<Command>,
    deadline: Option<Instant>,
    precise: bool,
) -> Result<Option<Command>, ()> {
    let Some(deadline) = deadline else {
        return receiver.recv().map(Some).map_err(|_| ());
    };
    let spin = if precise {
        Duration::from_micros(650)
    } else {
        Duration::ZERO
    };
    let wait = deadline
        .saturating_duration_since(Instant::now())
        .saturating_sub(spin);
    if !wait.is_zero() {
        match receiver.recv_timeout(wait) {
            Ok(command) => return Ok(Some(command)),
            Err(RecvTimeoutError::Disconnected) => return Err(()),
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
    loop {
        match receiver.try_recv() {
            Ok(command) => return Ok(Some(command)),
            Err(TryRecvError::Disconnected) => return Err(()),
            Err(TryRecvError::Empty) => {}
        }
        if !precise || Instant::now() >= deadline {
            return Ok(None);
        }
        std::hint::spin_loop();
    }
}

pub fn upcoming(events: &[Event], index: usize, count: usize, output: Output) -> String {
    let mut events = events
        .iter()
        .skip(index)
        .filter(|e| e.action == Action::Press)
        .peekable();
    let mut result = Vec::with_capacity(count);
    while result.len() < count {
        let Some(first) = events.next() else { break };
        let end = first.time + 0.06;
        let mut labels = Vec::new();
        for event in
            std::iter::once(first).chain(std::iter::from_fn(|| events.next_if(|e| e.time < end)))
        {
            if let Some(key) = keys::lookup(event.pitch) {
                let label = if output == Output::Voice {
                    const NOTES: [&str; 12] = [
                        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
                    ];
                    format!(
                        "{}{}",
                        NOTES[(event.pitch % 12) as usize],
                        event.pitch / 12 - 1
                    )
                } else {
                    key.display()
                };
                if !labels.contains(&label) {
                    labels.push(label);
                }
            }
        }
        match labels.len() {
            0 => {}
            1 => result.push(labels.pop().unwrap()),
            _ => result.push(format!("[{}]", labels.concat())),
        }
    }
    result.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder {
        focused: bool,
        presses: usize,
        resets: usize,
        next_error: Option<OutputError>,
        pedal: Vec<bool>,
        finishes: usize,
    }

    impl Device for Recorder {
        fn finish_voice(&mut self) {
            self.finishes += 1;
        }

        fn ready(&self, output: Output) -> bool {
            output == Output::Preview || self.focused
        }
        fn release_pending(&self) -> bool {
            false
        }
        fn retry_release(&mut self) {}
        fn reset(&mut self, _output: Output) {
            self.resets += 1;
        }
        fn press(
            &mut self,
            _output: Output,
            _pitch: u8,
            _key: keys::Key,
            _velocity: u8,
        ) -> Result<(), OutputError> {
            if let Some(error) = self.next_error.take() {
                return Err(error);
            }
            self.presses += 1;
            Ok(())
        }
        fn release(
            &mut self,
            _output: Output,
            _pitch: u8,
            _key: keys::Key,
        ) -> Result<(), OutputError> {
            Ok(())
        }
        fn velocity(&mut self, _key: keys::Key) -> Result<(), OutputError> {
            Ok(())
        }
        fn sustain(&mut self, _output: Output, down: bool) -> Result<(), OutputError> {
            self.pedal.push(down);
            Ok(())
        }
    }

    #[test]
    fn waits_for_roblox_then_resumes_without_skipping() {
        let mut player = Player::new();
        let mut recorder = Recorder::default();
        player.events = Arc::new([
            Event {
                time: 0.0,
                action: Action::Press,
                pitch: 60,
                velocity: 100,
                id: 0,
            },
            Event {
                time: 1.0,
                action: Action::Release,
                pitch: 60,
                velocity: 0,
                id: 0,
            },
        ]);
        player.snapshot.length = 1.0;
        player.command(Command::Play(0.0), &mut recorder);
        let start = player.last_tick;
        player.tick(start + Duration::from_secs(20), &mut recorder);
        assert!(player.snapshot.waiting);
        assert_eq!(player.snapshot.index, 0);
        recorder.focused = true;
        player.tick(start + Duration::from_secs(21), &mut recorder);
        assert_eq!(recorder.presses, 1);
        assert_eq!(player.snapshot.position, 0.0);
        player.command(Command::Pause, &mut recorder);
        assert_eq!(recorder.resets, 1);
        assert_eq!(player.snapshot.state, State::Paused);
    }

    #[test]
    fn live_piano_uses_voice_output_and_releases_on_exit() {
        let mut player = Player::new();
        let mut recorder = Recorder::default();
        let press = || Command::LiveKey {
            pitch: 60,
            down: true,
        };

        player.command(press(), &mut recorder);
        assert_eq!(recorder.presses, 0);
        player.output = Output::Voice;
        player.command(press(), &mut recorder);
        player.command(press(), &mut recorder);
        assert_eq!(recorder.presses, 1);
        assert!(player.snapshot.held[60]);
        assert!(player.snapshot.live);
        assert!(!player.snapshot.playing());
        player.command(Command::EndLive, &mut recorder);
        assert_eq!(recorder.finishes, 1);
        assert!(!player.snapshot.held[60]);
        assert!(!player.snapshot.live);
        player.command(press(), &mut recorder);
        player.command(
            Command::LiveKey {
                pitch: 60,
                down: false,
            },
            &mut recorder,
        );
        player.tick(player.live_until.unwrap(), &mut recorder);
        assert_eq!(recorder.finishes, 2);
        assert!(!player.snapshot.live);
    }

    #[test]
    fn stopping_voice_playback_leaves_a_piano_tail() {
        let mut player = Player::new();
        let mut recorder = Recorder::default();
        player.output = Output::Voice;
        player.snapshot.state = State::Playing;
        player.snapshot.held[60] = true;

        player.command(Command::Stop, &mut recorder);

        assert_eq!(recorder.finishes, 1);
        assert_eq!(recorder.resets, 0);
        assert!(!player.snapshot.held[60]);
        assert_eq!(player.snapshot.state, State::Stopped);
    }

    #[test]
    fn focus_lost_during_delivery_leaves_the_note_due() {
        let mut player = Player::new();
        let mut recorder = Recorder {
            focused: true,
            next_error: Some(OutputError::FocusLost),
            ..Default::default()
        };
        player.events = Arc::new([
            Event {
                time: 0.0,
                action: Action::Press,
                pitch: 60,
                velocity: 100,
                id: 0,
            },
            Event {
                time: 1.0,
                action: Action::Release,
                pitch: 60,
                velocity: 0,
                id: 0,
            },
        ]);
        player.snapshot.length = 1.0;
        player.command(Command::Play(0.0), &mut recorder);
        let start = player.last_tick;
        player.tick(start, &mut recorder);
        assert!(player.snapshot.waiting);
        assert!(!player.snapshot.held[60]);
        assert_eq!(player.snapshot.index, 0);
        player.tick(start + Duration::from_secs(1), &mut recorder);
        assert_eq!(recorder.presses, 1);
        assert_eq!(player.snapshot.index, 1);
    }

    #[test]
    fn preview_can_retry_a_note_after_an_output_failure() {
        let mut player = Player::new();
        let mut recorder = Recorder {
            next_error: Some(OutputError::PreviewUnavailable),
            ..Default::default()
        };
        player.output = Output::Preview;
        player.events = Arc::new([
            Event {
                time: 0.0,
                action: Action::Press,
                pitch: 60,
                velocity: 100,
                id: 0,
            },
            Event {
                time: 1.0,
                action: Action::Release,
                pitch: 60,
                velocity: 0,
                id: 0,
            },
        ]);
        player.snapshot.length = 1.0;
        player.command(Command::Play(0.0), &mut recorder);
        player.tick(player.last_tick, &mut recorder);
        assert_eq!(player.snapshot.state, State::Paused);
        assert_eq!(
            player.snapshot.output_error,
            Some(OutputError::PreviewUnavailable)
        );
        assert_eq!(player.snapshot.index, 0);
        player.command(Command::Play(0.0), &mut recorder);
        player.tick(player.last_tick, &mut recorder);
        assert_eq!(player.snapshot.output_error, None);
        assert_eq!(recorder.presses, 1);
    }

    #[test]
    fn resume_and_seek_restore_the_sustain_pedal() {
        let mut player = Player::new();
        let mut recorder = Recorder {
            focused: true,
            ..Default::default()
        };
        player.events = Arc::new([
            Event {
                time: 0.0,
                action: Action::PedalDown,
                pitch: 0,
                velocity: 0,
                id: 0,
            },
            Event {
                time: 5.0,
                action: Action::PedalUp,
                pitch: 0,
                velocity: 0,
                id: 1,
            },
        ]);
        player.snapshot.length = 5.0;
        player.command(Command::Play(0.0), &mut recorder);
        player.tick(player.last_tick, &mut recorder);
        player.command(Command::Pause, &mut recorder);
        player.command(Command::Play(0.0), &mut recorder);
        player.tick(player.last_tick, &mut recorder);
        assert_eq!(recorder.pedal, [true, true]);
        player.command(Command::Seek(3.0), &mut recorder);
        player.tick(player.last_tick, &mut recorder);
        assert_eq!(recorder.pedal, [true, true, true]);
    }
}
