use super::{Message, wide};
use crate::appearance::*;
use crate::settings::Settings;
use eframe::egui;
use std::cell::{Cell, RefCell};
use std::io;
use std::ptr::{null, null_mut};
use std::sync::mpsc::Sender;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::{
    HiDpi::{GetDpiForSystem, GetDpiForWindow},
    Input::KeyboardAndMouse::{ReleaseCapture, SetCapture},
    WindowsAndMessaging::*,
};

const CLASS: &str = "AztroMidi.Rust.Mini";
const WIDTH: i32 = 300;
const HEIGHT: i32 = 190;

#[derive(Clone, Copy, Debug)]
pub enum Action {
    Toggle,
    Stop,
    Previous,
    Next,
    Slower,
    Faster,
    Snap,
    Expand,
    Moved(f64, f64),
}

#[derive(PartialEq)]
pub struct View {
    pub title: String,
    pub status: &'static str,
    pub output_problem: bool,
    pub elapsed: String,
    pub duration: String,
    pub speed: String,
    pub next: String,
    pub progress: f64,
    pub playing: bool,
    pub snapped: bool,
}

pub struct Mini {
    hwnd: HWND,
    state: Box<State>,
}

struct State {
    send: Sender<Message>,
    ctx: egui::Context,
    view: RefCell<Option<View>>,
    drag: Cell<Option<(POINT, POINT)>>,
    clicked: Cell<Option<usize>>,
    opacity: Cell<u8>,
}

impl State {
    fn notify(&self, action: Action) {
        let _ = self.send.send(Message::Mini(action));
        self.ctx.request_repaint();
    }
}

impl Mini {
    pub(super) fn new(send: Sender<Message>, ctx: &egui::Context) -> io::Result<Self> {
        let class = wide(CLASS);
        let module = unsafe { GetModuleHandleW(null()) };
        let mut state = Box::new(State {
            send,
            ctx: ctx.clone(),
            view: RefCell::new(None),
            drag: Cell::new(None),
            clicked: Cell::new(None),
            opacity: Cell::new(0),
        });
        let hwnd = unsafe {
            let definition = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: module,
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                lpszClassName: class.as_ptr(),
                ..std::mem::zeroed()
            };
            RegisterClassW(&definition);
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
                class.as_ptr(),
                wide("AztroMidi Mini Player").as_ptr(),
                WS_POPUP,
                0,
                0,
                WIDTH,
                HEIGHT,
                null_mut(),
                null_mut(),
                module,
                (&mut *state as *mut State).cast(),
            )
        };
        if hwnd.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { hwnd, state })
    }

    pub fn show(&self, settings: &Settings) {
        self.place(settings, true);
        self.opacity(settings.mini_opacity);
        unsafe {
            ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
        }
    }

    pub fn update(&self, view: View, settings: &Settings) {
        self.place(settings, false);
        self.opacity(settings.mini_opacity);
        let mut contents = self.state.view.borrow_mut();
        if contents.as_ref() != Some(&view) {
            *contents = Some(view);
            unsafe {
                InvalidateRect(self.hwnd, null(), 0);
            }
        }
    }

    fn opacity(&self, value: f64) {
        let opacity = (value * 255.0).round() as u8;
        if self.state.opacity.get() != opacity {
            unsafe {
                SetLayeredWindowAttributes(self.hwnd, 0, opacity, LWA_ALPHA);
            }
            self.state.opacity.set(opacity);
        }
    }

    fn place(&self, settings: &Settings, initial: bool) {
        if self.state.drag.get().is_some() {
            return;
        }
        unsafe {
            let mut current: RECT = std::mem::zeroed();
            GetWindowRect(self.hwnd, &mut current);
            let (x, y, scale) = if let Some((roblox, rect)) = settings
                .mini_snap_to_roblox
                .then(super::roblox_rect)
                .flatten()
            {
                let scale = f64::from(GetDpiForWindow(roblox).max(96)) / 96.0;
                (
                    rect.right - physical(WIDTH + 6, scale),
                    rect.top + physical(46, scale),
                    scale,
                )
            } else if initial {
                let scale = f64::from(GetDpiForSystem().max(96)) / 96.0;
                let mut work: RECT = std::mem::zeroed();
                SystemParametersInfoW(SPI_GETWORKAREA, 0, (&mut work as *mut RECT).cast(), 0);
                let restored = settings
                    .mini_left
                    .zip(settings.mini_top)
                    .map(|(x, y)| POINT {
                        x: (x * scale) as i32,
                        y: (y * scale) as i32,
                    })
                    .filter(|point| {
                        let probe = POINT {
                            x: point.x + physical(40, scale),
                            y: point.y + physical(20, scale),
                        };
                        !MonitorFromPoint(probe, MONITOR_DEFAULTTONULL).is_null()
                    });
                let (x, y) = restored.map_or(
                    (
                        work.right - physical(WIDTH + 12, scale),
                        work.top + physical(60, scale),
                    ),
                    |point| (point.x, point.y),
                );
                (x, y, scale)
            } else {
                (current.left, current.top, dpi(self.hwnd))
            };
            let (width, height) = (physical(WIDTH, scale), physical(HEIGHT, scale));
            if current.left != x
                || current.top != y
                || current.right - current.left != width
                || current.bottom - current.top != height
            {
                SetWindowPos(self.hwnd, HWND_TOPMOST, x, y, width, height, SWP_NOACTIVATE);
            }
        }
    }
}

impl Drop for Mini {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.hwnd);
        }
    }
}

fn dpi(hwnd: HWND) -> f64 {
    f64::from(unsafe { GetDpiForWindow(hwnd) }.max(96)) / 96.0
}

fn physical(value: i32, scale: f64) -> i32 {
    (f64::from(value) * scale).round() as i32
}

#[derive(Clone, Copy)]
struct Button {
    label: &'static str,
    rect: [i32; 4],
    action: Action,
}

const BUTTONS: [Button; 8] = [
    Button {
        label: "Snap",
        rect: [177, 10, 221, 35],
        action: Action::Snap,
    },
    Button {
        label: "Expand",
        rect: [225, 10, 289, 35],
        action: Action::Expand,
    },
    Button {
        label: "Prev",
        rect: [11, 112, 51, 142],
        action: Action::Previous,
    },
    Button {
        label: "Play",
        rect: [55, 112, 116, 142],
        action: Action::Toggle,
    },
    Button {
        label: "Stop",
        rect: [120, 112, 166, 142],
        action: Action::Stop,
    },
    Button {
        label: "Next",
        rect: [170, 112, 211, 142],
        action: Action::Next,
    },
    Button {
        label: "-",
        rect: [222, 112, 253, 142],
        action: Action::Slower,
    },
    Button {
        label: "+",
        rect: [258, 112, 289, 142],
        action: Action::Faster,
    },
];

fn button_at(x: i32, y: i32) -> Option<usize> {
    BUTTONS.iter().position(|button| {
        let [left, top, right, bottom] = button.rect;
        x >= left && x < right && y >= top && y < bottom
    })
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> isize {
    unsafe {
        if msg == WM_NCCREATE {
            let create = &*(lparam as *const CREATESTRUCTW);
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        let state = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const State;
        if let Some(state) = state.as_ref() {
            return match msg {
                WM_MOUSEACTIVATE => MA_NOACTIVATE as isize,
                WM_ERASEBKGND => 1,
                WM_CLOSE => {
                    state.notify(Action::Expand);
                    0
                }
                WM_PAINT => {
                    paint(hwnd, state);
                    0
                }
                WM_LBUTTONDOWN => {
                    let (x, y) = mouse(hwnd, lparam);
                    state.clicked.set(button_at(x, y));
                    if state.clicked.get().is_none() && y < 43 {
                        let mut at = POINT { x: 0, y: 0 };
                        let mut rect: RECT = std::mem::zeroed();
                        GetCursorPos(&mut at);
                        GetWindowRect(hwnd, &mut rect);
                        state.drag.set(Some((
                            at,
                            POINT {
                                x: rect.left,
                                y: rect.top,
                            },
                        )));
                    }
                    SetCapture(hwnd);
                    0
                }
                WM_MOUSEMOVE => {
                    if let Some((start, corner)) = state.drag.get() {
                        let mut now = POINT { x: 0, y: 0 };
                        GetCursorPos(&mut now);
                        SetWindowPos(
                            hwnd,
                            null_mut(),
                            corner.x + now.x - start.x,
                            corner.y + now.y - start.y,
                            0,
                            0,
                            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                        );
                    }
                    0
                }
                WM_LBUTTONUP => {
                    let clicked = state.clicked.take();
                    let drag = state.drag.take();
                    ReleaseCapture();
                    let (x, y) = mouse(hwnd, lparam);
                    if let Some(index) = clicked.filter(|index| button_at(x, y) == Some(*index)) {
                        state.notify(BUTTONS[index].action);
                    } else if let Some((_, before)) = drag {
                        let mut rect: RECT = std::mem::zeroed();
                        GetWindowRect(hwnd, &mut rect);
                        if (rect.left - before.x).abs() > 2 || (rect.top - before.y).abs() > 2 {
                            let scale = dpi(hwnd);
                            state.notify(Action::Moved(
                                f64::from(rect.left) / scale,
                                f64::from(rect.top) / scale,
                            ));
                        }
                    }
                    0
                }
                WM_CAPTURECHANGED => {
                    state.drag.set(None);
                    state.clicked.set(None);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, wparam, lparam),
            };
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }
}

fn mouse(hwnd: HWND, lparam: isize) -> (i32, i32) {
    let scale = dpi(hwnd);
    (
        (f64::from(lparam as i16) / scale) as i32,
        (f64::from((lparam >> 16) as i16) / scale) as i32,
    )
}

fn paint(hwnd: HWND, state: &State) {
    unsafe {
        let mut paint: PAINTSTRUCT = std::mem::zeroed();
        let target = BeginPaint(hwnd, &mut paint);
        let scale = dpi(hwnd);
        let (width, height) = (physical(WIDTH, scale), physical(HEIGHT, scale));
        let dc = CreateCompatibleDC(target);
        let bitmap = CreateCompatibleBitmap(target, width, height);
        let old_bitmap = SelectObject(dc, bitmap);
        let face = wide("Segoe UI");
        let font = CreateFontW(
            -physical(15, scale),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            1,
            0,
            0,
            5,
            0,
            face.as_ptr(),
        );
        let old_font = SelectObject(dc, font);
        SetBkMode(dc, TRANSPARENT as i32);
        fill(dc, [0, 0, WIDTH, HEIGHT], scale, SURFACE);
        outline(dc, [0, 0, WIDTH, HEIGHT], scale, OUTLINE);
        fill(dc, [1, 1, WIDTH - 1, 43], scale, NAVIGATION);
        fill(dc, [1, 43, WIDTH - 1, 44], scale, SEPARATOR);
        let contents = state.view.borrow();
        if let Some(view) = contents.as_ref() {
            let status_color = if view.output_problem {
                WARNING
            } else if view.playing {
                ACCENT
            } else {
                MUTED
            };
            text(dc, &view.title, [11, 10, 172, 35], scale, TEXT, false);
            text(
                dc,
                view.status,
                [11, 45, 186, 67],
                scale,
                status_color,
                false,
            );
            text(dc, &view.speed, [224, 45, 290, 67], scale, TEXT, true);
            fill(dc, [11, 72, 289, 77], scale, OUTLINE);
            fill(
                dc,
                [11, 72, 11 + (278.0 * view.progress) as i32, 77],
                scale,
                BRAND,
            );
            text(dc, &view.elapsed, [11, 81, 96, 102], scale, MUTED, false);
            text(dc, &view.duration, [229, 81, 289, 102], scale, MUTED, true);
            fill(dc, [11, 153, 289, 179], scale, NAVIGATION);
            outline(dc, [11, 153, 289, 179], scale, SEPARATOR);
            text(dc, &view.next, [18, 153, 283, 179], scale, ACCENT, false);
            for button in BUTTONS {
                let primary = matches!(button.action, Action::Toggle);
                let selected = matches!(button.action, Action::Snap) && view.snapped;
                let (background, border, foreground) = if primary {
                    (BRAND, ACCENT, BASE)
                } else if selected {
                    (SELECTION, ACCENT, ACCENT)
                } else {
                    (CONTROL, OUTLINE, TEXT)
                };
                fill(dc, button.rect, scale, background);
                outline(dc, button.rect, scale, border);
                let label = if primary && view.playing {
                    "Pause"
                } else {
                    button.label
                };
                text(dc, label, button.rect, scale, foreground, true);
            }
        }
        BitBlt(target, 0, 0, width, height, dc, 0, 0, SRCCOPY);
        SelectObject(dc, old_font);
        DeleteObject(font);
        SelectObject(dc, old_bitmap);
        DeleteObject(bitmap);
        DeleteDC(dc);
        EndPaint(hwnd, &paint);
    }
}

fn colorref(color: egui::Color32) -> COLORREF {
    u32::from(color.r()) | u32::from(color.g()) << 8 | u32::from(color.b()) << 16
}

fn scaled([left, top, right, bottom]: [i32; 4], dpi: f64) -> RECT {
    RECT {
        left: physical(left, dpi),
        top: physical(top, dpi),
        right: physical(right, dpi),
        bottom: physical(bottom, dpi),
    }
}

fn fill(dc: HDC, rect: [i32; 4], scale: f64, color: egui::Color32) {
    unsafe {
        let brush = CreateSolidBrush(colorref(color));
        FillRect(dc, &scaled(rect, scale), brush);
        DeleteObject(brush);
    }
}

fn outline(dc: HDC, rect: [i32; 4], scale: f64, color: egui::Color32) {
    unsafe {
        let brush = CreateSolidBrush(colorref(color));
        FrameRect(dc, &scaled(rect, scale), brush);
        DeleteObject(brush);
    }
}

fn text(dc: HDC, label: &str, rect: [i32; 4], scale: f64, color: egui::Color32, centered: bool) {
    let mut rect = scaled(rect, scale);
    let wide = wide(label);
    let align = if centered { DT_CENTER } else { DT_LEFT };
    unsafe {
        SetTextColor(dc, colorref(color));
        DrawTextW(
            dc,
            wide.as_ptr(),
            (wide.len() - 1) as i32,
            &mut rect,
            align | DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
    }
}
