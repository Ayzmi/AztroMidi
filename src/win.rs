use crate::{
    keys::{Key, Modifier},
    settings::Output,
};
use eframe::egui;
use std::cell::Cell;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::{Dwm::DwmSetWindowAttribute, Gdi::ClientToScreen};
use windows_sys::Win32::Media::{Audio::*, timeBeginPeriod, timeEndPeriod};
use windows_sys::Win32::System::{
    DataExchange::COPYDATASTRUCT, LibraryLoader::GetModuleHandleW, Threading::*,
};
use windows_sys::Win32::UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*};

pub mod mini;

const INSTANCE: &str = if cfg!(debug_assertions) {
    "Local\\AztroMidi.Rust.Debug"
} else {
    "Local\\AztroMidi.Rust"
};
const CLASS: &str = if cfg!(debug_assertions) {
    "AztroMidi.Rust.Messages.Debug"
} else {
    "AztroMidi.Rust.Messages"
};
const HANDOFF: usize = 0x415a544d;

pub const HOTKEYS: [&str; 19] = [
    "None", "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12", "Insert",
    "Home", "End", "PageUp", "PageDown", "Pause",
];

#[derive(Debug)]
pub enum Message {
    Open(Vec<PathBuf>),
    Hotkey(usize),
    Mini(mini::Action),
}

pub struct Native {
    mutex: HANDLE,
    window: HWND,
    state: Box<Bridge>,
    pub messages: Receiver<Message>,
}

struct Bridge {
    send: Sender<Message>,
    ctx: egui::Context,
}

impl Bridge {
    fn notify(&self, message: Message) {
        let _ = self.send.send(message);
        self.ctx.request_repaint();
    }
}

pub struct Instance(HANDLE);

impl Instance {
    pub fn acquire(args: &[PathBuf]) -> io::Result<Option<Self>> {
        let mutex_name = wide(INSTANCE);
        let handle = unsafe { CreateMutexW(null(), 0, mutex_name.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        let exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let instance = Self(handle);
        if exists {
            forward(args)?;
            Ok(None)
        } else {
            Ok(Some(instance))
        }
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn forward(args: &[PathBuf]) -> io::Result<()> {
    let class = wide(CLASS);
    let end = Instant::now() + Duration::from_secs(4);
    let hwnd = loop {
        let hwnd = unsafe { FindWindowExW(HWND_MESSAGE, null_mut(), class.as_ptr(), null()) };
        if !hwnd.is_null() {
            break hwnd;
        }
        if Instant::now() >= end {
            return Err(io::Error::other(
                "The running AztroMidi window did not respond.",
            ));
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let data: Vec<u16> = args
        .iter()
        .flat_map(|arg| wide(arg.as_os_str()))
        .chain([0])
        .collect();
    let packet = COPYDATASTRUCT {
        dwData: HANDOFF,
        cbData: (data.len() * 2) as u32,
        lpData: data.as_ptr().cast_mut().cast(),
    };
    let mut received = 0;
    let delivered = unsafe {
        SendMessageTimeoutW(
            hwnd,
            WM_COPYDATA,
            0,
            &packet as *const _ as isize,
            SMTO_BLOCK | SMTO_ABORTIFHUNG,
            3_000,
            &mut received,
        )
    };
    if delivered == 0 || received == 0 {
        Err(io::Error::other(
            "The running AztroMidi window did not accept the files.",
        ))
    } else {
        Ok(())
    }
}

impl Native {
    pub fn new(ctx: &egui::Context, instance: Instance) -> io::Result<Self> {
        let class = wide(CLASS);
        let module = unsafe { GetModuleHandleW(null()) };
        let (send, messages) = mpsc::channel();
        let mut state = Box::new(Bridge {
            send,
            ctx: ctx.clone(),
        });
        let window = unsafe {
            let definition = WNDCLASSW {
                lpfnWndProc: Some(window_proc),
                hInstance: module,
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&definition);
            CreateWindowExW(
                0,
                class.as_ptr(),
                null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                null_mut(),
                module,
                (&mut *state as *mut Bridge).cast(),
            )
        };
        if window.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mutex = instance.0;
        std::mem::forget(instance);
        Ok(Self {
            mutex,
            window,
            state,
            messages,
        })
    }

    pub fn register_hotkeys(&self, bindings: [&str; 7]) -> Vec<String> {
        let mut failed = Vec::new();
        for id in 0..7 {
            unsafe {
                UnregisterHotKey(self.window, id);
            }
        }
        for (id, name) in bindings.into_iter().enumerate() {
            if name.is_empty() || name.eq_ignore_ascii_case("None") {
                continue;
            }
            let vk = HOTKEYS
                .iter()
                .position(|key| key.eq_ignore_ascii_case(name))
                .and_then(|index| match index {
                    1..=12 => Some(u32::from(VK_F1) + index as u32 - 1),
                    13..=18 => Some(u32::from(
                        [VK_INSERT, VK_HOME, VK_END, VK_PRIOR, VK_NEXT, VK_PAUSE][index - 13],
                    )),
                    _ => None,
                });
            if !vk.is_some_and(|vk| unsafe {
                RegisterHotKey(self.window, id as i32, MOD_NOREPEAT, vk) != 0
            }) {
                failed.push(name.to_owned());
            }
        }
        failed
    }

    pub fn mini(&self) -> io::Result<mini::Mini> {
        mini::Mini::new(self.state.send.clone(), &self.state.ctx)
    }
}

impl Drop for Native {
    fn drop(&mut self) {
        unsafe {
            for id in 0..7 {
                UnregisterHotKey(self.window, id);
            }
            DestroyWindow(self.window);
            CloseHandle(self.mutex);
        }
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> isize {
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const Bridge;
        if let Some(state) = state.as_ref() {
            match msg {
                WM_HOTKEY => {
                    state.notify(Message::Hotkey(wparam));
                    return 0;
                }
                WM_COPYDATA if lparam != 0 => {
                    let packet = &*(lparam as *const COPYDATASTRUCT);
                    if packet.dwData == HANDOFF
                        && !packet.lpData.is_null()
                        && (2..=262_144).contains(&packet.cbData)
                        && packet.cbData.is_multiple_of(2)
                    {
                        let words = std::slice::from_raw_parts(
                            packet.lpData.cast::<u16>(),
                            packet.cbData as usize / 2,
                        );
                        let paths = words
                            .split(|word| *word == 0)
                            .filter(|words| !words.is_empty())
                            .map(|part| PathBuf::from(OsString::from_wide(part)))
                            .collect();
                        state.notify(Message::Open(paths));
                        return 1;
                    }
                    return 0;
                }
                _ => {}
            }
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

pub fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain([0]).collect()
}

pub fn error_box(text: &str) {
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(text).as_ptr(),
            wide("AztroMidi").as_ptr(),
            MB_ICONERROR,
        );
    }
}

pub fn style_window(hwnd: HWND) {
    unsafe {
        let dark = 1i32;
        DwmSetWindowAttribute(hwnd, 20, (&dark as *const i32).cast(), 4);
        let background = 0x001a1714u32;
        DwmSetWindowAttribute(hwnd, 35, (&background as *const u32).cast(), 4);
    }
}

pub fn restore_window(hwnd: HWND) {
    if !hwnd.is_null() {
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
        }
    }
}

fn process_is_roblox(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }
    let mut name = [0u16; 1024];
    let mut length = name.len() as u32;
    let ok = unsafe {
        let ok = QueryFullProcessImageNameW(handle, 0, name.as_mut_ptr(), &mut length);
        CloseHandle(handle);
        ok != 0
    };
    if !ok {
        return false;
    }
    let path = OsString::from_wide(&name[..length as usize]);
    Path::new(&path)
        .file_stem()
        .and_then(OsStr::to_str)
        .is_some_and(|name| {
            let name = name.to_ascii_lowercase();
            name.starts_with("robloxplayer") || name == "windows10universal" || name == "roblox"
        })
}

thread_local! {
    static FOREGROUND: Cell<(HWND, u32, bool)> = const { Cell::new((null_mut(), 0, false)) };
    static ROBLOX: Cell<(HWND, Option<Instant>)> = const { Cell::new((null_mut(), None)) };
}

pub fn roblox_focused() -> bool {
    !focused_roblox_window().is_null()
}

fn focused_roblox_window() -> HWND {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return hwnd;
    }
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
    }
    let is_roblox = FOREGROUND.with(|cache| {
        let (last, previous_pid, matched) = cache.get();
        if hwnd == last && pid == previous_pid {
            return matched;
        }
        let matched = process_is_roblox(pid);
        cache.set((hwnd, pid, matched));
        matched
    });
    if is_roblox { hwnd } else { null_mut() }
}

pub fn roblox_window() -> HWND {
    let focused = focused_roblox_window();
    if !focused.is_null() {
        return focused;
    }
    ROBLOX.with(|cache| {
        let (previous, checked) = cache.get();
        if checked.is_some_and(|time| time.elapsed() < Duration::from_secs(1)) {
            return previous;
        }
        let mut result: HWND = null_mut();
        unsafe {
            EnumWindows(Some(find_roblox), (&mut result as *mut HWND) as isize);
        }
        cache.set((result, Some(Instant::now())));
        result
    })
}

unsafe extern "system" fn find_roblox(hwnd: HWND, out: isize) -> i32 {
    unsafe {
        if IsWindowVisible(hwnd) == 0 || !GetWindow(hwnd, GW_OWNER).is_null() {
            return 1;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if process_is_roblox(pid) {
            *(out as *mut HWND) = hwnd;
            return 0;
        }
        1
    }
}

pub fn focus_roblox() -> bool {
    let hwnd = roblox_window();
    if hwnd.is_null() {
        return false;
    }
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindow(hwnd, SW_RESTORE);
        }
        let foreground = GetWindowThreadProcessId(GetForegroundWindow(), null_mut());
        let this = GetCurrentThreadId();
        let attached =
            foreground != 0 && foreground != this && AttachThreadInput(this, foreground, 1) != 0;
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
        if attached {
            AttachThreadInput(this, foreground, 0);
        }
        GetForegroundWindow() == hwnd
    }
}

pub fn roblox_rect() -> Option<(HWND, RECT)> {
    let hwnd = roblox_window();
    if hwnd.is_null() {
        return None;
    }
    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        let mut origin = POINT { x: 0, y: 0 };
        if IsIconic(hwnd) != 0
            || GetClientRect(hwnd, &mut rect) == 0
            || ClientToScreen(hwnd, &mut origin) == 0
        {
            return None;
        }
        rect.left += origin.x;
        rect.right += origin.x;
        rect.top += origin.y;
        rect.bottom += origin.y;
        Some((hwnd, rect))
    }
}

pub struct TimerResolution(bool);

impl TimerResolution {
    pub fn new() -> Self {
        Self(unsafe { timeBeginPeriod(1) } == 0)
    }
}

impl Drop for TimerResolution {
    fn drop(&mut self) {
        if self.0 {
            unsafe {
                timeEndPeriod(1);
            }
        }
    }
}

const SHIFT: u16 = 0x2a;
const CTRL: u16 = 0x1d;
const ALT: u16 = 0x38;
const SPACE: u16 = 0x39;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputError {
    FocusLost,
    KeyboardUnavailable,
    PreviewUnavailable,
}

impl OutputError {
    pub fn message(self) -> &'static str {
        match self {
            Self::FocusLost => "Switch back to Roblox to continue playback.",
            Self::KeyboardUnavailable => {
                "Windows couldn't send the notes. Return to Roblox and press Play to retry."
            }
            Self::PreviewUnavailable => {
                "Windows MIDI preview is unavailable. Press Play to try the audio device again."
            }
        }
    }
}

pub struct Outputs {
    scan: [u16; 128],
    held: [Option<u8>; 256],
    cleanup: [bool; 256],
    pedal: bool,
    pending: bool,
    midi: HMIDIOUT,
}

impl Outputs {
    pub fn new() -> Self {
        let mut scan = [0; 128];
        for letter in (b'a'..=b'z').chain(b'0'..=b'9') {
            scan[letter as usize] = unsafe {
                let key = VkKeyScanW(u16::from(letter));
                let vk = if key == -1 {
                    u32::from(letter.to_ascii_uppercase())
                } else {
                    (key as u16 & 0xff).into()
                };
                MapVirtualKeyW(vk, MAPVK_VK_TO_VSC) as u16
            };
        }
        Self {
            scan,
            held: [None; 256],
            cleanup: [false; 256],
            pedal: false,
            pending: false,
            midi: null_mut(),
        }
    }

    pub fn waiting_for_release(&self) -> bool {
        self.pending
    }

    pub fn retry_release(&mut self) {
        if self.pending {
            self.reset(Output::Keyboard);
        }
    }

    pub fn press(
        &mut self,
        mode: Output,
        pitch: u8,
        key: Key,
        velocity: u8,
    ) -> Result<(), OutputError> {
        if mode == Output::Preview {
            return self.midi_message(0x90, pitch, velocity);
        }
        let scan = usize::from(self.scan[key.base as usize]);
        if scan == 0 || scan >= self.held.len() {
            return Err(OutputError::KeyboardUnavailable);
        }
        let mut input: [INPUT; 4] = unsafe { std::mem::zeroed() };
        let mut pos = 0;
        if self.held[scan].is_some() {
            input[pos] = keyboard(scan as u16, true);
            pos += 1;
        }
        pos += key_events(&mut input[pos..], scan as u16, key.modifier, false);
        self.send_keyboard(&input[..pos])?;
        self.held[scan] = Some(pitch);
        Ok(())
    }

    pub fn release(&mut self, mode: Output, pitch: u8, key: Key) -> Result<(), OutputError> {
        if mode == Output::Preview {
            return self.midi_message(0x80, pitch, 0);
        }
        let scan = usize::from(self.scan[key.base as usize]);
        if self.held.get(scan) == Some(&Some(pitch)) {
            self.send_keyboard(&[keyboard(scan as u16, true)])?;
            self.held[scan] = None;
        }
        Ok(())
    }

    pub fn velocity(&mut self, key: Key) -> Result<(), OutputError> {
        let scan = self.scan[key.base as usize];
        if scan == 0 || usize::from(scan) >= self.held.len() {
            return Err(OutputError::KeyboardUnavailable);
        }
        let mut input: [INPUT; 4] = unsafe { std::mem::zeroed() };
        let count = key_events(&mut input, scan, key.modifier, true);
        self.send_keyboard(&input[..count])?;
        self.held[scan as usize] = None;
        Ok(())
    }

    pub fn sustain(&mut self, mode: Output, down: bool) -> Result<(), OutputError> {
        if mode == Output::Preview {
            return self.midi_message(0xb0, 64, if down { 127 } else { 0 });
        }
        self.send_keyboard(&[keyboard(SPACE, !down)])?;
        self.pedal = down;
        Ok(())
    }

    pub fn reset(&mut self, mode: Output) {
        if mode == Output::Preview {
            if !self.midi.is_null() {
                unsafe {
                    midiOutReset(self.midi);
                }
                let _ = self.midi_message(0xc0, 0, 0);
            }
            return;
        }
        let mut input: [INPUT; 256] = unsafe { std::mem::zeroed() };
        let mut count = 0;
        for (scan, owner) in self.held.iter().enumerate() {
            if owner.is_some() || self.cleanup[scan] || (scan == usize::from(SPACE) && self.pedal) {
                input[count] = keyboard(scan as u16, true);
                count += 1;
            }
        }
        self.pending = count > 0 && self.send_keyboard(&input[..count]).is_err();
        if !self.pending {
            self.held.fill(None);
            self.cleanup.fill(false);
            self.pedal = false;
        }
    }

    fn send_keyboard(&mut self, input: &[INPUT]) -> Result<(), OutputError> {
        if !roblox_focused() {
            return Err(OutputError::FocusLost);
        }
        let sent = unsafe {
            SendInput(
                input.len() as u32,
                input.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        } as usize;
        if sent == input.len() {
            return Ok(());
        }
        // A short write may have sent a key-down without its matching key-up.
        for event in input.iter().take(sent) {
            let key = unsafe { event.Anonymous.ki };
            let scan = usize::from(key.wScan);
            let down = key.dwFlags & KEYEVENTF_KEYUP == 0;
            self.cleanup[scan] = down;
            if !down {
                self.held[scan] = None;
                if scan == usize::from(SPACE) {
                    self.pedal = false;
                }
            }
        }
        self.pending |= sent > 0;
        Err(if roblox_focused() {
            OutputError::KeyboardUnavailable
        } else {
            OutputError::FocusLost
        })
    }

    fn midi_message(&mut self, status: u32, first: u8, second: u8) -> Result<(), OutputError> {
        unsafe {
            if self.midi.is_null() {
                let mut midi = null_mut();
                if midiOutOpen(&mut midi, u32::MAX, 0, 0, 0) != 0 || midi.is_null() {
                    return Err(OutputError::PreviewUnavailable);
                }
                self.midi = midi;
                if midiOutShortMsg(self.midi, 0xc0) != 0 {
                    self.close_preview();
                    return Err(OutputError::PreviewUnavailable);
                }
            }
            let message = status | u32::from(first) << 8 | u32::from(second) << 16;
            if midiOutShortMsg(self.midi, message) != 0 {
                self.close_preview();
                return Err(OutputError::PreviewUnavailable);
            }
        }
        Ok(())
    }

    fn close_preview(&mut self) {
        if !self.midi.is_null() {
            unsafe {
                midiOutReset(self.midi);
                midiOutClose(self.midi);
            }
            self.midi = null_mut();
        }
    }
}

impl Drop for Outputs {
    fn drop(&mut self) {
        self.reset(Output::Keyboard);
        self.close_preview();
    }
}

fn keyboard(scan: u16, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: scan,
                dwFlags: KEYEVENTF_SCANCODE | if up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn key_events(out: &mut [INPUT], scan: u16, modifier: Modifier, tap: bool) -> usize {
    let modifier = match modifier {
        Modifier::None => None,
        Modifier::Shift => Some(SHIFT),
        Modifier::Ctrl => Some(CTRL),
        Modifier::Alt => Some(ALT),
    };
    let mut count = 0;
    if let Some(modifier) = modifier {
        out[count] = keyboard(modifier, false);
        count += 1;
    }
    out[count] = keyboard(scan, false);
    count += 1;
    if tap {
        out[count] = keyboard(scan, true);
        count += 1;
    }
    if let Some(modifier) = modifier {
        out[count] = keyboard(modifier, true);
        count += 1;
    }
    count
}
