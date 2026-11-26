use crate::midi::Midi;
use crate::settings::{DropAction, Settings};
use eframe::egui;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};
use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SongDuration {
    Pending,
    Known(f64),
    Unreadable,
}

pub struct Song {
    pub path: PathBuf,
    pub title: String,
    pub folder: String,
    pub duration: SongDuration,
    filter: String,
}

impl Song {
    fn new(path: PathBuf, root: &Path) -> Self {
        let title = path
            .file_stem()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned();
        let folder = path
            .parent()
            .and_then(|dir| dir.strip_prefix(root).ok())
            .map_or_else(
                || "(added)".into(),
                |dir| dir.to_string_lossy().into_owned(),
            );
        let filter = format!("{title}\n{folder}").to_lowercase();
        Self {
            path,
            title,
            folder,
            duration: SongDuration::Pending,
            filter,
        }
    }

    pub fn matches(&self, query: &str) -> bool {
        self.filter.contains(query)
    }
}

pub struct Library {
    pub root: PathBuf,
    pub songs: Vec<Song>,
    extras: Vec<PathBuf>,
    durations: Option<Receiver<(PathBuf, SongDuration)>>,
    imports: Option<Receiver<ImportResult>>,
}

impl Library {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            songs: Vec::new(),
            extras: Vec::new(),
            durations: None,
            imports: None,
        }
    }

    pub fn reload(&mut self, ctx: &egui::Context) -> io::Result<()> {
        let mut paths = Vec::new();
        let result = if self.root.is_dir() {
            collect_midi(&self.root, &mut paths)
        } else {
            Ok(())
        };
        let mut known: HashSet<_> = paths.iter().map(|path| path_key(path)).collect();
        for path in &self.extras {
            if path.is_file() && known.insert(path_key(path)) {
                paths.push(path.clone());
            }
        }
        self.songs = paths
            .into_iter()
            .map(|path| Song::new(path, &self.root))
            .collect();
        self.sort_songs();
        self.scan_durations(ctx);
        result
    }

    pub fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut known: HashSet<_> = self.songs.iter().map(|song| path_key(&song.path)).collect();
        for path in paths {
            if !known.insert(path_key(&path)) {
                continue;
            }
            if !path.starts_with(&self.root) {
                self.extras.push(path.clone());
            }
            self.songs.push(Song::new(path, &self.root));
        }
        self.sort_songs();
    }

    fn sort_songs(&mut self) {
        self.songs.sort_unstable_by(|left, right| {
            left.filter
                .cmp(&right.filter)
                .then(left.path.cmp(&right.path))
        });
    }

    pub fn remove(&mut self, path: &Path) {
        self.songs.retain(|song| !same(&song.path, path));
        self.extras.retain(|extra| !same(extra, path));
    }

    pub fn scan_durations(&mut self, ctx: &egui::Context) {
        let pending: Vec<_> = self
            .songs
            .iter()
            .filter(|song| song.duration == SongDuration::Pending)
            .map(|song| song.path.clone())
            .collect();
        if pending.is_empty() {
            self.durations = None;
            return;
        }
        let (send, receive) = mpsc::channel();
        self.durations = Some(receive);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut last_repaint = Instant::now();
            for path in pending {
                let length = Midi::load(&path).map_or(SongDuration::Unreadable, |midi| {
                    SongDuration::Known(midi.duration)
                });
                if send.send((path, length)).is_err() {
                    return;
                }
                if last_repaint.elapsed() >= Duration::from_millis(40) {
                    ctx.request_repaint();
                    last_repaint = Instant::now();
                }
            }
            drop(send);
            ctx.request_repaint();
        });
    }

    pub fn update_durations(&mut self) {
        let Some(durations) = &self.durations else {
            return;
        };
        let mut completed = HashMap::new();
        loop {
            match durations.try_recv() {
                Ok((path, length)) => {
                    completed.insert(path, length);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.durations = None;
                    break;
                }
            }
        }
        if !completed.is_empty() {
            for song in &mut self.songs {
                if let Some(&length) = completed.get(&song.path) {
                    song.duration = length;
                }
            }
        }
    }

    pub fn importing(&self) -> bool {
        self.imports.is_some()
    }

    pub fn import(
        &mut self,
        ctx: &egui::Context,
        paths: Vec<PathBuf>,
        action: DropAction,
    ) -> io::Result<()> {
        if self.imports.is_some() {
            return Err(io::Error::other("An import is already in progress."));
        }
        let (send, receive) = mpsc::channel();
        let ctx = ctx.clone();
        let root = self.root.clone();
        std::thread::Builder::new()
            .name("MIDI import".into())
            .spawn(move || {
                let mut result = ImportResult::default();
                let mut files = Vec::new();
                for path in paths {
                    if let Err(error) = collect_midi(&path, &mut files) {
                        result.error = Some(error.to_string());
                    }
                }
                let mut seen = HashSet::new();
                files.retain(|path| seen.insert(path_key(path)));
                for path in files {
                    match place(&path, &root, action) {
                        Ok(path) => result.paths.push(path),
                        Err(error) => {
                            result.error = Some(format!("{}: {error}", path.display()));
                            result.paths.push(path);
                        }
                    }
                }
                let _ = send.send(result);
                ctx.request_repaint();
            })?;
        self.imports = Some(receive);
        Ok(())
    }

    pub fn imported(&mut self) -> Option<ImportResult> {
        match self.imports.as_ref()?.try_recv() {
            Ok(result) => {
                self.imports = None;
                Some(result)
            }
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.imports = None;
                Some(ImportResult {
                    error: Some("The MIDI import was interrupted.".into()),
                    ..Default::default()
                })
            }
        }
    }
}

#[derive(Default)]
pub struct ImportResult {
    pub paths: Vec<PathBuf>,
    pub error: Option<String>,
}

pub fn same(a: &Path, b: &Path) -> bool {
    a == b || path_key(a) == path_key(b)
}

fn path_key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

pub fn favorite(settings: &Settings, path: &Path) -> bool {
    settings
        .favorites
        .iter()
        .any(|entry| same(Path::new(entry), path))
}

fn is_midi(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["mid", "midi", "rmi"]
                .iter()
                .any(|name| ext.eq_ignore_ascii_case(name))
        })
}

fn collect_midi(root: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    if root.is_file() && is_midi(root) {
        files.push(root.to_path_buf());
    }
    let mut pending = Vec::new();
    if root.is_dir() {
        pending.push(root.to_path_buf());
    }
    let mut last_error = None;
    while let Some(dir) = pending.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                last_error = Some(error);
                continue;
            }
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() && metadata.file_attributes() & 0x400 == 0 {
                pending.push(entry.path());
            } else if metadata.is_file() && is_midi(&entry.path()) {
                files.push(entry.path());
            }
        }
    }
    last_error.map_or(Ok(()), Err)
}

fn place(path: &Path, root: &Path, action: DropAction) -> io::Result<PathBuf> {
    if action == DropAction::ListOnly || root.as_os_str().is_empty() || under(path, root) {
        return Ok(path.to_path_buf());
    }
    fs::create_dir_all(root)?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("Missing MIDI filename."))?;
    let temporary = root.join(format!(
        ".aztromidi-{}.tmp",
        crate::settings::unique_suffix()
    ));
    let mut source = File::open(path)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let copied = (|| {
        io::copy(&mut source, &mut output)?;
        output.sync_all()?;
        drop(output);
        let from = crate::win::wide(temporary.canonicalize()?.as_os_str());
        let folder = root.canonicalize()?;
        let stem = path.file_stem().unwrap_or(name).to_string_lossy();
        let ext = path.extension().unwrap_or_default().to_string_lossy();
        for number in 1.. {
            let file_name = if number == 1 {
                name.to_owned()
            } else {
                format!("{stem} ({number}).{ext}").into()
            };
            let destination = folder.join(&file_name);
            let to = crate::win::wide(destination.as_os_str());
            // A rename publishes only complete files and cannot replace a competing import.
            if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } != 0 {
                return Ok(root.join(file_name));
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::AlreadyExists {
                return Err(error);
            }
        }
        unreachable!()
    })();
    if copied.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    let destination = copied?;
    drop(source);
    if action == DropAction::Move
        && let Err(error) = fs::remove_file(path)
    {
        return Err(io::Error::new(
            error.kind(),
            format!(
                "Copied to {}, original kept: {error}",
                destination.display()
            ),
        ));
    }
    Ok(destination)
}

fn under(path: &Path, root: &Path) -> bool {
    match (path.canonicalize(), root.canonicalize()) {
        (Ok(path), Ok(root)) => path.starts_with(root),
        _ => path.starts_with(root),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_import_does_not_block_the_queue() {
        let mut library = Library::new(PathBuf::new());
        let (send, receive) = mpsc::channel();
        library.imports = Some(receive);
        drop(send);
        assert!(library.imported().unwrap().error.is_some());
        assert!(!library.importing());
    }

    #[test]
    fn refresh_rechecks_a_song_replaced_on_disk() -> io::Result<()> {
        let folder = std::env::temp_dir().join(format!(
            "aztromidi-refresh-{}",
            crate::settings::unique_suffix()
        ));
        fs::create_dir_all(&folder)?;
        let path = folder.join("practice.mid");
        let track = [0, 0x90, 60, 100, 0x83, 0x60, 0x80, 60, 0];
        fs::write(&path, crate::midi::fixture(0, 480, &[&track]))?;
        let mut library = Library::new(folder.clone());
        let ctx = egui::Context::default();
        library.reload(&ctx)?;
        let (_, duration) = library
            .durations
            .as_ref()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(duration, SongDuration::Known(0.5));
        library.songs[0].duration = duration;
        fs::write(&path, b"not a MIDI file")?;
        library.reload(&ctx)?;
        let (_, duration) = library
            .durations
            .as_ref()
            .unwrap()
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(duration, SongDuration::Unreadable);
        fs::remove_dir_all(folder)
    }

    #[test]
    fn import_collision_preserves_existing_file() -> io::Result<()> {
        let temp = std::env::temp_dir().join(format!(
            "aztromidi-import-{}",
            crate::settings::unique_suffix()
        ));
        let dest = temp.join("songs");
        fs::create_dir_all(&dest)?;
        let path = temp.join("song.mid");
        fs::write(&path, b"new")?;
        fs::write(dest.join("song.mid"), b"existing")?;
        let copied = place(&path, &dest, DropAction::Copy)?;
        assert_eq!(copied.file_name().unwrap(), "song (2).mid");
        assert_eq!(fs::read(&path)?, b"new");
        assert_eq!(fs::read(dest.join("song.mid"))?, b"existing");
        let moved = place(&path, &dest, DropAction::Move)?;
        assert_eq!(moved.file_name().unwrap(), "song (3).mid");
        assert!(!path.exists());
        fs::remove_dir_all(temp)
    }
}
