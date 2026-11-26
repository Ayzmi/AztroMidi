use crate::{
    humanize::Humanizer,
    keys,
    library::{self, Library},
    midi::Midi,
    playback::{self, Command, Engine, Snapshot, State},
    schedule::{self, Event},
    settings::{self, Output, Settings},
    win::{self, Message, Native, mini},
};
use eframe::egui::{self, Key, Pos2, Vec2, ViewportBuilder, ViewportCommand, WindowLevel};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::HWND;

mod ui;

const APP: &str = "AztroMidi";
const SAVE_DELAY: Duration = Duration::from_millis(350);
const MINI_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, Eq, PartialEq)]
enum Page {
    Player,
    Humanize,
    Settings,
}

impl Page {
    fn parse(page: &str) -> Self {
        match page {
            "Humanize" => Self::Humanize,
            "Settings" => Self::Settings,
            _ => Self::Player,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Player => "Player",
            Self::Humanize => "Humanize",
            Self::Settings => "Settings",
        }
    }
}

pub fn run() {
    if let Err(error) = launch() {
        win::error_box(&error.to_string());
    }
}

fn launch() -> Result<(), Box<dyn std::error::Error>> {
    let current = std::env::current_dir()?;
    let args: Vec<_> = std::env::args_os()
        .skip(1)
        .map(|arg| current.join(arg))
        .collect();
    let Some(instance) = win::Instance::acquire(&args)? else {
        return Ok(());
    };
    let exe = std::env::current_exe()?;
    let dir = exe.parent().unwrap_or(&current);
    let data_dir = settings::storage(dir);
    let settings_path = data_dir.join("settings.json");
    let settings = Settings::load(&settings_path);
    let fallback_songs = data_dir.join("songs");
    let mut viewport = ViewportBuilder::default()
        .with_title(APP)
        .with_inner_size(Vec2::new(
            settings.window_width.unwrap_or(1000.0).clamp(820.0, 3500.0) as f32,
            settings.window_height.unwrap_or(700.0).clamp(600.0, 2200.0) as f32,
        ))
        .with_min_inner_size([820.0, 600.0])
        .with_maximized(settings.window_maximized)
        .with_window_level(if settings.always_on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        })
        .with_drag_and_drop(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../docs/images/logo.png")) {
        viewport = viewport.with_icon(icon);
    }
    if let Some((left, top)) = settings.window_left.zip(settings.window_top)
        && left.abs() < 20_000.0
        && top.abs() < 20_000.0
    {
        viewport = viewport.with_position(Pos2::new(left as f32, top as f32));
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        APP,
        options,
        Box::new(move |cc| {
            let native = Native::new(&cc.egui_ctx, instance)?;
            let hwnd = cc
                .window_handle()
                .ok()
                .and_then(|handle| match handle.as_raw() {
                    RawWindowHandle::Win32(raw) => Some(raw.hwnd.get() as HWND),
                    _ => None,
                })
                .unwrap_or_else(null_mut);
            win::style_window(hwnd);
            ui::setup(&cc.egui_ctx);
            let mut app = Desktop::new(
                cc.egui_ctx.clone(),
                hwnd,
                native,
                settings_path,
                fallback_songs,
                settings,
            )?;
            if !args.is_empty() {
                app.enqueue_import(args);
            }
            Ok(Box::new(app))
        }),
    )?;
    Ok(())
}

struct SongState {
    path: PathBuf,
    title: String,
    midi: Midi,
    enabled: Vec<bool>,
    transpose: i32,
}

struct Desktop {
    ctx: egui::Context,
    hwnd: HWND,
    native: Native,
    engine: Engine,
    settings: Settings,
    saved: Settings,
    settings_path: PathBuf,
    fallback_songs: PathBuf,
    page: Page,
    library: Library,
    filter: String,
    favorites_only: bool,
    visible_filter: Option<(String, bool)>,
    visible_songs: Vec<usize>,
    selected: Option<PathBuf>,
    song: Option<SongState>,
    events: Arc<[Event]>,
    generation: u64,
    finished: u64,
    import_queue: VecDeque<Vec<PathBuf>>,
    hotkey_errors: Vec<String>,
    hint: String,
    mini: Option<mini::Mini>,
    mini_update_at: Instant,
    seek_preview: Option<f64>,
    save_at: Option<Instant>,
    save_worker: Option<JoinHandle<(Settings, std::io::Result<()>)>>,
    save_error: Option<String>,
    rebuild_at: Option<Instant>,
}

impl Desktop {
    fn new(
        ctx: egui::Context,
        hwnd: HWND,
        native: Native,
        settings_path: PathBuf,
        fallback_songs: PathBuf,
        settings: Settings,
    ) -> std::io::Result<Self> {
        let saved = settings.clone();
        let page = Page::parse(&settings.last_page);
        let library = Library::new(library_folder(&settings, &fallback_songs));
        let engine = Engine::spawn(ctx.clone())?;
        let mut app = Self {
            ctx,
            hwnd,
            native,
            engine,
            settings,
            saved,
            settings_path,
            fallback_songs,
            page,
            library,
            filter: String::new(),
            favorites_only: false,
            visible_filter: None,
            visible_songs: Vec::new(),
            selected: None,
            song: None,
            events: Arc::new([]),
            generation: 0,
            finished: 0,
            import_queue: VecDeque::new(),
            hotkey_errors: Vec::new(),
            hint: "Open a MIDI file or choose a library folder.".into(),
            mini: None,
            mini_update_at: Instant::now(),
            seek_preview: None,
            save_at: None,
            save_worker: None,
            save_error: None,
            rebuild_at: None,
        };
        app.register_hotkeys();
        app.configure();
        let last = PathBuf::from(&app.settings.last_song);
        let restore_song = last.is_file();
        if restore_song {
            app.library.add([last.clone()]);
        }
        app.reload_library();
        if restore_song {
            app.load_song(&last, false);
        }
        Ok(app)
    }

    fn configure(&self) {
        self.engine.send(Command::Configure {
            output: self.settings.output,
            speed: self.settings.speed,
            velocity: self.settings.velocity_keys,
        });
    }

    fn register_hotkeys(&mut self) {
        self.hotkey_errors = self
            .native
            .register_hotkeys(self.settings.hotkeys.bindings());
    }

    fn reload_library(&mut self) {
        self.visible_filter = None;
        if let Err(error) = self.library.reload(&self.ctx) {
            self.hint = format!("Could not read the library: {error}");
        }
    }

    fn set_library(&mut self, dir: PathBuf) {
        self.settings.library_folder = dir.to_string_lossy().into_owned();
        self.library.root = dir;
        self.reload_library();
    }

    fn choose_folder(&mut self) {
        let dialog = rfd::FileDialog::new().set_title("Choose MIDI library");
        let dialog = if self.library.root.is_dir() {
            dialog.set_directory(&self.library.root)
        } else {
            dialog
        };
        if let Some(folder) = dialog.pick_folder() {
            self.set_library(folder);
        }
    }

    fn choose_files(&mut self) {
        let dialog = rfd::FileDialog::new()
            .set_title("Open MIDI files")
            .add_filter("MIDI", &["mid", "midi", "rmi"]);
        if let Some(paths) = dialog.pick_files() {
            self.enqueue_import(paths);
        }
    }

    fn open_folder(&mut self, path: &Path) {
        if let Err(error) = std::process::Command::new("explorer.exe")
            .arg("/select,")
            .arg(path)
            .spawn()
        {
            self.hint = format!("Could not open Explorer: {error}");
        }
    }

    fn enqueue_import(&mut self, files: Vec<PathBuf>) {
        if !files.is_empty() {
            self.import_queue.push_back(files);
            self.show_page(Page::Player);
        }
    }

    fn update_import(&mut self) {
        if let Some(result) = self.library.imported() {
            if let Some(first) = result.paths.first().cloned() {
                self.library.add(result.paths);
                self.visible_filter = None;
                self.library.scan_durations(&self.ctx);
                self.favorites_only = false;
                self.filter.clear();
                if self.snapshot().playing() {
                    self.selected = Some(first);
                } else {
                    self.load_song(&first, false);
                }
                if let Some(error) = result.error {
                    self.hint = format!("Some files could not be imported: {error}");
                }
            } else {
                self.hint = result
                    .error
                    .unwrap_or_else(|| "No MIDI files found.".into());
            }
        }
        if !self.library.importing()
            && let Some(paths) = self.import_queue.pop_front()
        {
            self.library.root = library_folder(&self.settings, &self.fallback_songs);
            if let Err(error) = self
                .library
                .import(&self.ctx, paths, self.settings.drop_action)
            {
                self.hint = format!("Could not start importing MIDI files: {error}");
                self.ctx.request_repaint();
            }
        }
    }

    fn load_song(&mut self, path: &Path, start: bool) -> bool {
        let midi = match Midi::load(path) {
            Ok(midi) => midi,
            Err(error) => {
                self.hint = format!("Could not open {}: {error}", path.display());
                return false;
            }
        };
        let title = path
            .file_stem()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        let enabled = midi
            .tracks
            .iter()
            .map(|track| track.note_count > 0)
            .collect();
        let mut song = SongState {
            path: path.to_path_buf(),
            title,
            midi,
            enabled,
            transpose: 0,
        };
        if self.settings.auto_transpose {
            song.transpose = auto_transpose(&song, &self.settings);
        }
        self.ctx
            .send_viewport_cmd(ViewportCommand::Title(format!("{APP} — {}", song.title)));
        self.song = Some(song);
        self.selected = Some(path.to_path_buf());
        self.seek_preview = None;
        self.settings.last_song = path.to_string_lossy().into_owned();
        self.generation += 1;
        self.finished = 0;
        self.rebuild(false);
        self.hint = if self.settings.output == Output::Preview {
            "Loaded. Press Play to listen on this PC.".into()
        } else {
            "Loaded. Press Play when your Roblox piano is ready.".into()
        };
        if start {
            self.engine.send(Command::Play(0.0));
        }
        true
    }

    fn rebuild(&mut self, preserve_position: bool) {
        self.rebuild_at = None;
        let Some(song) = &self.song else { return };
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let mut humanizer = Humanizer::new(self.settings.humanize.clone(), seed);
        self.events = schedule::build(
            &song.midi,
            &song.enabled,
            song.transpose,
            &self.settings,
            &mut humanizer,
        )
        .into();
        self.engine.send(Command::Load {
            events: self.events.clone(),
            humanizer,
            generation: self.generation,
            preserve_position,
        });
    }

    fn defer_rebuild(&mut self) {
        self.rebuild_at = Some(Instant::now() + Duration::from_millis(160));
        self.ctx.request_repaint_after(Duration::from_millis(160));
    }

    fn snapshot(&self) -> Snapshot {
        let snapshot = self.engine.snapshot();
        if snapshot.generation == self.generation {
            snapshot
        } else {
            Snapshot {
                length: self.events.last().map_or(0.0, |event| event.time),
                ..Default::default()
            }
        }
    }

    fn play(&mut self, countdown: bool) {
        if self.events.is_empty() {
            self.hint = if self.song.is_some() {
                "Enable a playable track before starting playback."
            } else {
                "Load a MIDI with playable tracks first."
            }
            .into();
            return;
        }
        if self.settings.output == Output::Keyboard
            && self.settings.auto_focus_roblox
            && !win::roblox_focused()
            && !win::focus_roblox()
        {
            self.hint = "Switch to Roblox to start playback.".into();
        }
        let use_countdown = countdown && self.settings.output == Output::Keyboard;
        self.engine.send(Command::Play(if use_countdown {
            f64::from(self.settings.countdown_seconds)
        } else {
            0.0
        }));
    }

    fn toggle_play(&mut self, from_hotkey: bool) {
        let snapshot = self.snapshot();
        if from_hotkey && snapshot.waiting && snapshot.state == State::Playing {
            let focused = win::focus_roblox();
            self.hint = if focused {
                "Returned to Roblox."
            } else {
                "Could not focus Roblox."
            }
            .into();
        } else if snapshot.playing() {
            self.engine.send(Command::Pause);
        } else {
            self.play(snapshot.state != State::Paused);
        }
    }

    fn select(&mut self, path: PathBuf) {
        self.selected = Some(path.clone());
        if !self.snapshot().playing() && !self.is_current(&path) {
            self.load_song(&path, false);
        }
    }

    fn play_selected(&mut self) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        let playing = self.snapshot().playing();
        if !self.is_current(&path) {
            if self.load_song(&path, playing) && !playing {
                self.play(true);
            }
        } else if !playing {
            self.play(true);
        }
    }

    fn is_current(&self, path: &Path) -> bool {
        self.song
            .as_ref()
            .is_some_and(|song| library::same(&song.path, path))
    }

    fn refresh_visible_songs(&mut self) {
        if self
            .visible_filter
            .as_ref()
            .is_some_and(|(filter, favorites)| {
                filter == &self.filter && *favorites == self.favorites_only
            })
        {
            return;
        }
        self.visible_filter = Some((self.filter.clone(), self.favorites_only));
        let query = self.filter.trim().to_lowercase();
        self.visible_songs.clear();
        self.visible_songs.extend(
            self.library
                .songs
                .iter()
                .enumerate()
                .filter(|(_, song)| {
                    song.matches(&query)
                        && (!self.favorites_only || library::favorite(&self.settings, &song.path))
                })
                .map(|(index, _)| index),
        );
    }

    fn step(&mut self, direction: isize, auto_play: bool) {
        self.refresh_visible_songs();
        if self.visible_songs.is_empty() {
            return;
        }
        let index = self
            .visible_songs
            .iter()
            .position(|index| self.is_current(&self.library.songs[*index].path));
        let start = index.map_or(if direction > 0 { -1 } else { 0 }, |index| index as isize);
        let count = self.visible_songs.len() as isize;
        for offset in 1..=count {
            let next = (start + direction * offset).rem_euclid(count) as usize;
            let song = &self.library.songs[self.visible_songs[next]];
            if song.duration == library::SongDuration::Unreadable {
                continue;
            }
            let path = song.path.clone();
            if self.load_song(&path, auto_play) {
                return;
            }
        }
        self.hint = "No readable MIDI files match this view.".into();
    }

    fn favorite(&mut self, path: &Path) {
        self.visible_filter = None;
        if library::favorite(&self.settings, path) {
            self.settings
                .favorites
                .retain(|saved| !library::same(Path::new(saved), path));
        } else {
            self.settings
                .favorites
                .push(path.to_string_lossy().into_owned());
        }
    }

    fn remove(&mut self, path: &Path) {
        self.library.remove(path);
        self.visible_filter = None;
        if self
            .selected
            .as_deref()
            .is_some_and(|selected| library::same(selected, path))
        {
            self.selected = None;
        }
    }

    fn speed(&mut self, change: f64) {
        self.settings.speed = ((self.settings.speed + change) * 100.0).round() / 100.0;
        self.settings.speed = self.settings.speed.clamp(0.25, 2.0);
        self.configure();
    }

    fn show_page(&mut self, page: Page) {
        self.page = page;
        self.settings.last_page = page.title().into();
    }

    fn update_window_level(&self) {
        let level = if self.settings.always_on_top && self.mini.is_none() {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        };
        self.ctx
            .send_viewport_cmd(ViewportCommand::WindowLevel(level));
    }

    fn toggle_mini(&mut self) {
        if self.mini.take().is_some() {
            self.update_window_level();
            win::restore_window(self.hwnd);
        } else {
            match self.native.mini() {
                Ok(mini) => {
                    mini.show(&self.settings);
                    self.mini = Some(mini);
                    self.mini_update_at = Instant::now();
                    self.update_window_level();
                }
                Err(error) => self.hint = format!("Could not open the mini player: {error}"),
            }
        }
    }

    fn native_messages(&mut self) {
        while let Ok(message) = self.native.messages.try_recv() {
            match message {
                Message::Open(paths) => {
                    win::restore_window(self.hwnd);
                    self.enqueue_import(paths);
                }
                Message::Hotkey(index) => match index {
                    0 => self.toggle_play(true),
                    1 => self.engine.send(Command::Stop),
                    2 => self.speed(-0.05),
                    3 => self.speed(0.05),
                    4 => self.step(1, self.snapshot().playing()),
                    5 => {
                        self.settings.humanize.enabled = !self.settings.humanize.enabled;
                        self.rebuild(true);
                    }
                    6 => self.toggle_mini(),
                    _ => {}
                },
                Message::Mini(action) => match action {
                    mini::Action::Toggle => self.toggle_play(false),
                    mini::Action::Stop => self.engine.send(Command::Stop),
                    mini::Action::Previous => self.step(-1, self.snapshot().playing()),
                    mini::Action::Next => self.step(1, self.snapshot().playing()),
                    mini::Action::Slower => self.speed(-0.05),
                    mini::Action::Faster => self.speed(0.05),
                    mini::Action::Snap => {
                        self.settings.mini_snap_to_roblox = !self.settings.mini_snap_to_roblox
                    }
                    mini::Action::Expand => self.toggle_mini(),
                    mini::Action::Moved(x, y) => {
                        self.settings.mini_snap_to_roblox = false;
                        self.settings.mini_left = Some(x);
                        self.settings.mini_top = Some(y);
                    }
                },
            }
        }
    }

    fn shortcuts(&mut self) {
        let ctx = self.ctx.clone();
        for (key, page) in [
            (Key::Num1, Page::Player),
            (Key::Num2, Page::Humanize),
            (Key::Num3, Page::Settings),
        ] {
            if ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, key)) {
                self.show_page(page);
            }
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::O)) {
            self.choose_files();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::M)) {
            self.toggle_mini();
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::F)) {
            self.show_page(Page::Player);
            ctx.memory_mut(|memory| memory.request_focus(egui::Id::new("library-search")));
        }
        if ctx.wants_keyboard_input() {
            return;
        }
        let command = ctx.input(|i| {
            [
                Key::Space,
                Key::Enter,
                Key::Delete,
                Key::ArrowLeft,
                Key::ArrowRight,
                Key::Minus,
                Key::Plus,
                Key::Equals,
            ]
            .into_iter()
            .find(|key| i.key_pressed(*key))
        });
        match command {
            Some(Key::Space) => self.toggle_play(false),
            Some(Key::Enter) => self.play_selected(),
            Some(Key::Delete) => {
                if let Some(path) = self.selected.clone() {
                    self.remove(&path);
                }
            }
            Some(Key::ArrowLeft) => self
                .engine
                .send(Command::Seek(self.snapshot().position - 5.0)),
            Some(Key::ArrowRight) => self
                .engine
                .send(Command::Seek(self.snapshot().position + 5.0)),
            Some(Key::Minus) => self.speed(-0.05),
            Some(Key::Plus | Key::Equals) => self.speed(0.05),
            _ => {}
        }
    }

    fn save_later(&mut self, now: Instant) {
        let previous = window_placement(&self.settings);
        self.ctx.input(|input| {
            let viewport = input.viewport();
            if let Some(maximized) = viewport.maximized {
                self.settings.window_maximized = maximized;
            }
            if viewport.minimized == Some(false) && viewport.maximized == Some(false) {
                if let Some(rect) = viewport.inner_rect
                    && rect.width() >= 820.0
                    && rect.height() >= 600.0
                {
                    self.settings.window_width = Some(f64::from(rect.width()));
                    self.settings.window_height = Some(f64::from(rect.height()));
                }
                if let Some(rect) = viewport.outer_rect {
                    self.settings.window_left = Some(f64::from(rect.left()));
                    self.settings.window_top = Some(f64::from(rect.top()));
                }
            }
        });
        if previous != window_placement(&self.settings) {
            self.save_at = Some(now + SAVE_DELAY);
        }

        if self
            .save_worker
            .as_ref()
            .is_some_and(|worker| !worker.is_finished())
        {
            self.ctx.request_repaint_after(Duration::from_millis(40));
            return;
        }
        if let Some(worker) = self.save_worker.take() {
            match worker.join() {
                Ok((saved, Ok(()))) => {
                    self.saved = saved;
                    if self.save_error.take().is_some() {
                        self.ctx.request_repaint();
                    }
                }
                Ok((_, Err(error))) => self.save_failed(now, &error.to_string()),
                Err(_) => self.save_failed(now, "The settings writer stopped."),
            }
        }

        if self.settings == self.saved {
            self.save_at = None;
            if self.save_error.is_some() {
                self.ctx.request_repaint();
            }
            self.save_error = None;
            return;
        }

        let save_at = *self.save_at.get_or_insert(now + SAVE_DELAY);
        if now < save_at {
            self.ctx.request_repaint_after(save_at - now);
            return;
        }

        self.save_at = None;
        let settings = self.settings.clone();
        let path = self.settings_path.clone();
        // Flushing the file can stall the window's event loop.
        match thread::Builder::new()
            .name("AztroMidi Settings".into())
            .spawn(move || {
                let result = settings.save(&path);
                (settings, result)
            }) {
            Ok(worker) => {
                self.save_worker = Some(worker);
                self.ctx.request_repaint_after(Duration::from_millis(40));
            }
            Err(error) => self.save_failed(now, &error.to_string()),
        }
    }

    fn save_failed(&mut self, now: Instant, error: &str) {
        self.save_error = Some(format!("Could not save settings: {error}"));
        let retry = Duration::from_secs(2);
        self.save_at = Some(now + retry);
        self.ctx.request_repaint();
        self.ctx.request_repaint_after(retry);
    }
}

impl eframe::App for Desktop {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        self.native_messages();
        let drops: Vec<_> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        self.enqueue_import(drops);
        self.shortcuts();
        self.update_import();
        self.library.update_durations();
        if self.rebuild_at.is_some_and(|at| now >= at) {
            self.rebuild(true);
        }
        let snapshot = self.snapshot();
        if snapshot.finished > self.finished {
            self.finished = snapshot.finished;
            if self.settings.repeat {
                self.engine.send(Command::Seek(0.0));
                self.engine.send(Command::Play(0.0));
            } else if self.settings.auto_next {
                self.step(1, true);
            }
        }
        self.render(ctx, snapshot);
        if let Some(mini) = &self.mini
            && now >= self.mini_update_at
        {
            let playing = snapshot.playing();
            mini.update(
                mini::View {
                    title: self
                        .song
                        .as_ref()
                        .map_or_else(|| "No song loaded".into(), |song| song.title.clone()),
                    status: snapshot.status(),
                    output_problem: snapshot.waiting || snapshot.output_error.is_some(),
                    elapsed: format_time(snapshot.position),
                    duration: format_time(snapshot.length),
                    speed: format!("{:.2}x", self.settings.speed),
                    next: playback::upcoming(&self.events, snapshot.index, 8),
                    progress: (snapshot.position / snapshot.length.max(0.1)).clamp(0.0, 1.0),
                    playing,
                    snapped: self.settings.mini_snap_to_roblox,
                },
                &self.settings,
            );
            self.mini_update_at = now + MINI_INTERVAL;
        }
        if self.mini.is_some() {
            ctx.request_repaint_after(
                self.mini_update_at
                    .saturating_duration_since(Instant::now()),
            );
        }
        self.save_later(now);
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        if let Some(worker) = self.save_worker.take()
            && let Ok((saved, Ok(()))) = worker.join()
        {
            self.saved = saved;
        }
        if self.settings != self.saved {
            let _ = self.settings.save(&self.settings_path);
        }
    }
}

fn window_placement(
    settings: &Settings,
) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>, bool) {
    (
        settings.window_left,
        settings.window_top,
        settings.window_width,
        settings.window_height,
        settings.window_maximized,
    )
}

fn library_folder(settings: &Settings, fallback: &Path) -> PathBuf {
    let path = Path::new(&settings.library_folder);
    if path.is_dir() {
        path.to_path_buf()
    } else {
        fallback.to_path_buf()
    }
}

fn auto_transpose(song: &SongState, settings: &Settings) -> i32 {
    keys::best_transpose(
        song.midi
            .notes
            .iter()
            .filter(|note| schedule::included(note, &song.enabled, settings))
            .map(|note| note.pitch),
        settings.layout,
    )
}

fn format_time(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    format!("{}:{:02}", total / 60, total % 60)
}
