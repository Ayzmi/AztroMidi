use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub enum Layout {
    Keys61,
    #[default]
    Keys88,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub enum OutOfRange {
    #[default]
    Fold,
    Skip,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub enum Sustain {
    #[default]
    Off,
    FollowPedal,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub enum Output {
    #[default]
    Keyboard,
    Preview,
    Voice,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Voice {
    pub destination: String,
    pub microphone: String,
    pub monitor: String,
    pub live_keyboard: bool,
    pub keep_mic_live: bool,
    pub piano_level: f32,
    pub mic_level: f32,
    pub monitor_level: f32,
    pub room: f32,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            destination: String::new(),
            microphone: String::new(),
            monitor: String::new(),
            live_keyboard: true,
            keep_mic_live: false,
            piano_level: 70.0,
            mic_level: 85.0,
            monitor_level: 70.0,
            room: 18.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Serialize)]
pub enum DropAction {
    #[default]
    Move,
    Copy,
    ListOnly,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Humanize {
    pub enabled: bool,
    pub timing_jitter_ms: f64,
    pub chord_roll_ms: f64,
    pub tempo_drift_percent: f64,
    pub hold_variance_percent: f64,
    pub velocity_variance: i32,
    pub phrase_breath_ms: f64,
    pub mistake_chance_percent: f64,
}

impl Default for Humanize {
    fn default() -> Self {
        Self {
            enabled: true,
            timing_jitter_ms: 8.0,
            chord_roll_ms: 14.0,
            tempo_drift_percent: 2.5,
            hold_variance_percent: 12.0,
            velocity_variance: 6,
            phrase_breath_ms: 35.0,
            mistake_chance_percent: 0.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Hotkeys {
    pub play_pause: String,
    pub stop: String,
    pub slow_down: String,
    pub speed_up: String,
    pub next_song: String,
    pub toggle_humanize: String,
    pub toggle_mini: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            play_pause: "F1".into(),
            stop: "F2".into(),
            slow_down: "F3".into(),
            speed_up: "F4".into(),
            next_song: "F5".into(),
            toggle_humanize: "F6".into(),
            toggle_mini: "F7".into(),
        }
    }
}

impl Hotkeys {
    pub fn bindings(&self) -> [&str; 7] {
        [
            &self.play_pause,
            &self.stop,
            &self.slow_down,
            &self.speed_up,
            &self.next_song,
            &self.toggle_humanize,
            &self.toggle_mini,
        ]
    }

    pub fn bindings_mut(&mut self) -> [&mut String; 7] {
        [
            &mut self.play_pause,
            &mut self.stop,
            &mut self.slow_down,
            &mut self.speed_up,
            &mut self.next_song,
            &mut self.toggle_humanize,
            &mut self.toggle_mini,
        ]
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct Settings {
    pub library_folder: String,
    pub layout: Layout,
    pub out_of_range: OutOfRange,
    pub sustain: Sustain,
    pub output: Output,
    pub voice: Voice,
    pub velocity_keys: bool,
    pub auto_transpose: bool,
    pub skip_drums: bool,
    pub auto_focus_roblox: bool,
    pub always_on_top: bool,
    pub repeat: bool,
    pub auto_next: bool,
    pub countdown_seconds: i32,
    pub speed: f64,
    pub min_hold_ms: f64,
    pub max_hold_ms: f64,
    pub drop_action: DropAction,
    pub last_song: String,
    pub last_page: String,
    pub window_left: Option<f64>,
    pub window_top: Option<f64>,
    pub window_width: Option<f64>,
    pub window_height: Option<f64>,
    pub window_maximized: bool,
    pub favorites: Vec<String>,
    pub mini_snap_to_roblox: bool,
    pub mini_opacity: f64,
    pub mini_left: Option<f64>,
    pub mini_top: Option<f64>,
    pub humanize: Humanize,
    pub hotkeys: Hotkeys,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            library_folder: String::new(),
            layout: Layout::default(),
            out_of_range: OutOfRange::default(),
            sustain: Sustain::default(),
            output: Output::default(),
            voice: Voice::default(),
            velocity_keys: false,
            auto_transpose: true,
            skip_drums: true,
            auto_focus_roblox: true,
            always_on_top: false,
            repeat: false,
            auto_next: false,
            countdown_seconds: 3,
            speed: 1.0,
            min_hold_ms: 25.0,
            max_hold_ms: 4000.0,
            drop_action: DropAction::default(),
            last_song: String::new(),
            last_page: "Player".into(),
            window_left: None,
            window_top: None,
            window_width: None,
            window_height: None,
            window_maximized: false,
            favorites: Vec::new(),
            mini_snap_to_roblox: true,
            mini_opacity: 0.94,
            mini_left: None,
            mini_top: None,
            humanize: Humanize::default(),
            hotkeys: Hotkeys::default(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        fs::read(path).map_or_else(
            |_| Self::default(),
            |bytes| Self::from_json(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes)),
        )
    }

    fn from_json(bytes: &[u8]) -> Self {
        let Ok(Value::Object(mut saved)) = serde_json::from_slice(bytes) else {
            return Self::default();
        };
        for (key, variants) in [
            ("Layout", &["Keys61", "Keys88"][..]),
            ("OutOfRange", &["Fold", "Skip"]),
            ("Sustain", &["Off", "FollowPedal"]),
            ("Output", &["Keyboard", "Background", "Preview", "Voice"]),
            ("DropAction", &["Move", "Copy", "ListOnly"]),
        ] {
            if let Some(value) = saved.get_mut(key) {
                let variant = value
                    .as_u64()
                    .and_then(|i| variants.get(i as usize).copied())
                    .or_else(|| {
                        let text = value.as_str()?;
                        variants
                            .iter()
                            .copied()
                            .find(|v| v.eq_ignore_ascii_case(text))
                    });
                *value = match variant {
                    Some("Background") => Value::from("Keyboard"),
                    Some(name) => Value::from(name),
                    None => Value::Null,
                };
            }
        }
        if let Some(repeat) = saved.remove("Loop") {
            saved.entry("Repeat").or_insert(repeat);
        }
        if let Some(Value::Array(favorites)) = saved.get_mut("Favorites") {
            favorites.retain(|v| v.as_str().is_some_and(|s| !s.trim().is_empty()));
        }
        if let Some(Value::Object(humanize)) = saved.get_mut("Humanize")
            && let Some(value) = humanize.get_mut("VelocityVariance")
            && value.as_i64().and_then(|n| i32::try_from(n).ok()).is_none()
        {
            *value = Value::Null;
        }
        if let Some(value) = saved.get_mut("CountdownSeconds")
            && value.as_i64().and_then(|n| i32::try_from(n).ok()).is_none()
        {
            *value = Value::Null;
        }
        let mut merged = serde_json::to_value(Self::default()).expect("serializable defaults");
        merge_settings(&mut merged, Value::Object(saved));
        let mut settings: Self = serde_json::from_value(merged).unwrap_or_default();
        settings.countdown_seconds = settings.countdown_seconds.clamp(0, 10);
        settings.speed = settings.speed.clamp(0.25, 2.0);
        settings.min_hold_ms = settings.min_hold_ms.clamp(5.0, 120.0);
        settings.max_hold_ms = settings.max_hold_ms.max(settings.min_hold_ms);
        settings.mini_opacity = settings.mini_opacity.clamp(0.4, 1.0);
        settings.voice.piano_level = settings.voice.piano_level.clamp(0.0, 100.0);
        settings.voice.mic_level = settings.voice.mic_level.clamp(0.0, 100.0);
        settings.voice.monitor_level = settings.voice.monitor_level.clamp(0.0, 100.0);
        settings.voice.room = settings.voice.room.clamp(0.0, 60.0);
        let h = &mut settings.humanize;
        h.timing_jitter_ms = h.timing_jitter_ms.clamp(0.0, 40.0);
        h.chord_roll_ms = h.chord_roll_ms.clamp(0.0, 60.0);
        h.tempo_drift_percent = h.tempo_drift_percent.clamp(0.0, 10.0);
        h.hold_variance_percent = h.hold_variance_percent.clamp(0.0, 40.0);
        h.velocity_variance = h.velocity_variance.clamp(0, 30);
        h.phrase_breath_ms = h.phrase_breath_ms.clamp(0.0, 150.0);
        h.mistake_chance_percent = h.mistake_chance_percent.clamp(0.0, 5.0);
        settings
    }

    pub fn playback_layout(&self) -> Layout {
        if self.output == Output::Voice {
            Layout::Keys88
        } else {
            self.layout
        }
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let mut json = serde_json::to_value(self)?;
        let fields = json.as_object_mut().expect("settings object");
        fields.remove("Repeat");
        fields.insert("Loop".into(), Value::from(self.repeat));
        fields.insert("OnlyWhenRobloxFocused".into(), Value::from(true));
        fields.retain(|_, value| !value.is_null());
        let temp = path.with_extension(format!("{}.tmp", unique_suffix()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        let result = (|| {
            file.write_all(&serde_json::to_vec_pretty(&json)?)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
}

fn merge_settings(default: &mut Value, saved: Value) {
    match (default, saved) {
        (Value::Object(defaults), Value::Object(fields)) => {
            for (key, value) in fields {
                if let Some(default) = defaults.get_mut(&key) {
                    merge_settings(default, value);
                }
            }
        }
        (target @ Value::Null, value @ Value::Number(_)) => *target = value,
        (target, value)
            if !value.is_null()
                && !value.is_object()
                && std::mem::discriminant(target) == std::mem::discriminant(&value) =>
        {
            *target = value;
        }
        _ => {}
    }
}

pub fn storage(exe_dir: &Path) -> PathBuf {
    let probe = exe_dir.join(format!(".write-test-{}", unique_suffix()));
    if let Ok(file) = OpenOptions::new().write(true).create_new(true).open(&probe) {
        drop(file);
        let _ = fs::remove_file(probe);
        return exe_dir.to_path_buf();
    }
    let dir = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("AztroMidi");
    let _ = fs::create_dir_all(&dir);
    dir
}

pub fn unique_suffix() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{}-{nanos}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_old_preferences_when_other_values_are_invalid() {
        let settings = Settings::from_json(
            br#"{"Humanize":{"TimingJitterMs":"NaN","TempoDriftPercent":200},
                "Hotkeys":null,"Favorites":[null,"","a.mid"],"LastSong":null,
                "Speed":1.5,"Layout":99,"MiniOpacity":"Infinity",
                "CountdownSeconds":-5,"Output":2,"Loop":true,"WindowLeft":20.5}"#,
        );
        assert_eq!(settings.speed, 1.5);
        assert!(settings.repeat);
        assert_eq!(settings.output, Output::Preview);
        assert_eq!(settings.layout, Layout::Keys88);
        assert_eq!(settings.favorites, ["a.mid"]);
        assert_eq!(settings.hotkeys.play_pause, "F1");
        assert_eq!(settings.humanize.timing_jitter_ms, 8.0);
        assert_eq!(settings.humanize.tempo_drift_percent, 10.0);
        assert_eq!(settings.countdown_seconds, 0);
        assert_eq!(settings.window_left, Some(20.5));
        assert_eq!(
            Settings::from_json(br#"{"Output":"Background"}"#).output,
            Output::Keyboard
        );
    }

    #[test]
    fn restores_voice_routing_without_changing_legacy_layout() {
        let settings = Settings::from_json(
            br#"{"Output":"voice","Layout":"Keys61","Voice":{"Destination":"output","Microphone":"input","Monitor":"headphones","MonitorLevel":42,"Room":100}}"#,
        );
        assert_eq!(settings.output, Output::Voice);
        assert_eq!(settings.layout, Layout::Keys61);
        assert_eq!(settings.playback_layout(), Layout::Keys88);
        assert_eq!(settings.voice.destination, "output");
        assert_eq!(settings.voice.microphone, "input");
        assert_eq!(settings.voice.monitor, "headphones");
        assert_eq!(settings.voice.monitor_level, 42.0);
        assert_eq!(settings.voice.room, 60.0);
    }
}
