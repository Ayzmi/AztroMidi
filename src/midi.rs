use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Clone, Copy, Debug)]
pub struct Note {
    pub track: usize,
    pub channel: u8,
    pub pitch: u8,
    pub velocity: u8,
    pub start: f64,
    pub end: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Pedal {
    pub track: usize,
    pub time: f64,
    pub down: bool,
}

#[derive(Default, Debug)]
pub struct Track {
    name: String,
    instrument: String,
    channels: u16,
    pub note_count: usize,
}

impl Track {
    pub fn drums(&self) -> bool {
        self.channels == (1 << 9)
    }

    pub fn name(&self, index: usize) -> String {
        if !self.name.is_empty() {
            self.name.clone()
        } else if !self.instrument.is_empty() {
            self.instrument.clone()
        } else {
            format!("Track {}", index + 1)
        }
    }
}

#[derive(Debug)]
pub struct Midi {
    pub notes: Vec<Note>,
    pub pedal: Vec<Pedal>,
    pub tracks: Vec<Track>,
    pub duration: f64,
    pub bpm: f64,
}

impl Midi {
    pub fn load(path: &Path) -> io::Result<Self> {
        Self::parse(&fs::read(path)?)
    }

    pub fn parse(data: &[u8]) -> io::Result<Self> {
        let header_index = data
            .windows(4)
            .position(|bytes| bytes == b"MThd")
            .ok_or_else(|| invalid("MIDI header is missing."))?;
        let mut input = Reader(&data[(header_index + 4)..]);
        let header_size = input.u32()? as usize;
        let mut header = Reader(input.take(header_size)?);
        let format = header.u16()?;
        let track_count = usize::from(header.u16()?);
        let division = header.u16()?;
        if format > 1 {
            return Err(invalid(format!("Unsupported MIDI format {format}.")));
        }
        let timebase = Timebase::parse(division)?;
        let mut tracks = Vec::with_capacity(track_count.min(data.len() / 8));
        let mut events = Vec::new();
        while tracks.len() < track_count {
            let name = input.take(4)?;
            let length = input.u32()? as usize;
            let bytes = input.take(length)?;
            if name == b"MTrk" {
                tracks.push(parse_track(Reader(bytes), tracks.len(), &mut events)?);
            }
        }
        events.sort_unstable_by_key(|e| (e.tick, e.order));
        Ok(assemble(tracks, events, timebase))
    }
}

fn invalid(reason: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, reason.into())
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> io::Result<&'a [u8]> {
        let (value, rest) = self
            .0
            .split_at_checked(count)
            .ok_or_else(|| invalid("MIDI data is truncated or has an invalid chunk length."))?;
        self.0 = rest;
        Ok(value)
    }

    fn byte(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn variable(&mut self) -> io::Result<u32> {
        let mut value = 0;
        for _ in 0..4 {
            let b = self.byte()?;
            value = value << 7 | u32::from(b & 0x7f);
            if b & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err(invalid("MIDI variable-length value exceeds four bytes."))
    }
}

enum Timebase {
    Quarter(f64),
    Fixed(f64),
}

impl Timebase {
    fn parse(division: u16) -> io::Result<Self> {
        if division == 0 {
            return Err(invalid("Invalid MIDI time division."));
        }
        if division & 0x8000 == 0 {
            return Ok(Self::Quarter(f64::from(division)));
        }
        let [rate, ticks] = division.to_be_bytes();
        let fps = match rate as i8 {
            -24 => 24.0,
            -25 => 25.0,
            -29 => 30_000.0 / 1001.0,
            -30 => 30.0,
            _ => return Err(invalid("Invalid SMPTE frame rate.")),
        };
        if ticks == 0 {
            return Err(invalid("Invalid SMPTE time division."));
        }
        Ok(Self::Fixed(1.0 / (fps * f64::from(ticks))))
    }

    fn seconds(&self, ticks: u64, tempo: u32) -> f64 {
        let ticks = ticks as f64;
        match self {
            Self::Quarter(division) => ticks * (f64::from(tempo) / 1_000_000.0) / division,
            Self::Fixed(seconds) => ticks * seconds,
        }
    }
}

struct Raw {
    tick: u64,
    order: usize,
    track: usize,
    kind: RawKind,
}

enum RawKind {
    Tempo(u32),
    Pedal(bool),
    Note {
        channel: u8,
        pitch: u8,
        velocity: u8,
    },
}

fn parse_track(mut input: Reader<'_>, track: usize, events: &mut Vec<Raw>) -> io::Result<Track> {
    let mut info = Track::default();
    let mut tick = 0;
    let mut running = None;
    while !input.0.is_empty() {
        tick += u64::from(input.variable()?);
        let status = *input
            .0
            .first()
            .ok_or_else(|| invalid("Missing MIDI status byte."))?;
        let status = if status & 0x80 == 0 {
            running.ok_or_else(|| invalid("Running status has no preceding channel event."))?
        } else {
            input.byte()?;
            if status < 0xf0 {
                running = Some(status);
            }
            status
        };
        let event = match status {
            0xff => {
                let meta = input.byte()?;
                let count = input.variable()? as usize;
                let data = input.take(count)?;
                match meta {
                    0x2f => break,
                    0x03 if info.name.is_empty() => info.name = read_text(data),
                    0x04 if info.instrument.is_empty() => info.instrument = read_text(data),
                    0x51 if data.len() >= 3 => {
                        let tempo = u32::from_be_bytes([0, data[0], data[1], data[2]]);
                        events.push(Raw {
                            tick,
                            track,
                            order: events.len(),
                            kind: RawKind::Tempo(tempo),
                        });
                    }
                    _ => {}
                }
                continue;
            }
            0xf0 | 0xf7 => {
                running = None;
                let count = input.variable()? as usize;
                input.take(count)?;
                continue;
            }
            0xf1..=0xfe => return Err(invalid("Unsupported MIDI status byte.")),
            _ => {
                let (kind, channel) = (status & 0xf0, status & 0x0f);
                let count = if matches!(kind, 0xc0 | 0xd0) { 1 } else { 2 };
                let data = input.take(count)?;
                if data.iter().any(|byte| byte & 0x80 != 0) {
                    return Err(invalid("Invalid MIDI data byte."));
                }
                match kind {
                    0x80 | 0x90 => {
                        let velocity = if kind == 0x90 { data[1] } else { 0 };
                        RawKind::Note {
                            channel,
                            pitch: data[0],
                            velocity,
                        }
                    }
                    0xb0 if data[0] == 64 => RawKind::Pedal(data[1] >= 64),
                    0xc0 => {
                        info.channels |= 1 << channel;
                        if info.instrument.is_empty() && channel != 9 {
                            info.instrument = instrument(data[0]);
                        }
                        continue;
                    }
                    _ => continue,
                }
            }
        };
        info.channels |= 1 << (status & 0x0f);
        events.push(Raw {
            tick,
            track,
            order: events.len(),
            kind: event,
        });
    }
    Ok(info)
}

fn assemble(mut tracks: Vec<Track>, events: Vec<Raw>, timebase: Timebase) -> Midi {
    let mut notes: Vec<Note> = Vec::with_capacity(events.len() / 2);
    let mut pedal = Vec::new();
    let mut open: HashMap<(usize, u8, u8), Vec<usize>> = HashMap::new();
    let (mut time, mut last_tick, mut tempo, mut bpm) = (0.0, 0, 500_000, 120.0);
    let mut first_tempo = true;
    for event in events {
        time += timebase.seconds(event.tick - last_tick, tempo);
        last_tick = event.tick;
        match event.kind {
            RawKind::Tempo(value) => {
                if value > 0 {
                    tempo = value;
                }
                if first_tempo && event.tick == 0 {
                    bpm = (600_000_000.0 / f64::from(tempo)).round_ties_even() / 10.0;
                }
                first_tempo = false;
            }
            RawKind::Pedal(down) => pedal.push(Pedal {
                track: event.track,
                time,
                down,
            }),
            RawKind::Note {
                channel,
                pitch,
                velocity,
            } => {
                let key = (event.track, channel, pitch);
                if velocity > 0 {
                    open.entry(key).or_default().push(notes.len());
                    notes.push(Note {
                        track: event.track,
                        channel,
                        pitch,
                        velocity,
                        start: time,
                        end: time,
                    });
                    tracks[event.track].note_count += 1;
                } else if let Some(index) = open.get_mut(&key).and_then(Vec::pop) {
                    notes[index].end = time;
                }
            }
        }
    }
    for index in open.into_values().flatten() {
        let note = &mut notes[index];
        note.end = (note.start + 0.25).max(time);
    }
    let mut duration: f64 = 0.0;
    for note in &mut notes {
        if note.end <= note.start {
            note.end = note.start + 0.05;
        }
        duration = duration.max(note.end);
    }
    Midi {
        notes,
        pedal,
        tracks,
        duration,
        bpm,
    }
}

fn read_text(bytes: &[u8]) -> String {
    let value: String = bytes
        .iter()
        .copied()
        .map(char::from)
        .filter(|c| !c.is_control())
        .collect();
    value.trim().into()
}

fn instrument(program: u8) -> String {
    const FAMILIES: [&str; 16] = [
        "Piano",
        "Chromatic Perc.",
        "Organ",
        "Guitar",
        "Bass",
        "Strings",
        "Ensemble",
        "Brass",
        "Reed",
        "Pipe",
        "Synth Lead",
        "Synth Pad",
        "Synth FX",
        "Ethnic",
        "Percussive",
        "Sound FX",
    ];
    format!(
        "{} #{}",
        FAMILIES[program as usize / 8],
        u16::from(program) + 1
    )
}

#[cfg(test)]
pub fn fixture(format: u16, division: u16, tracks: &[&[u8]]) -> Vec<u8> {
    let mut bytes = b"MThd\0\0\0\x06".to_vec();
    bytes.extend(format.to_be_bytes());
    bytes.extend((tracks.len() as u16).to_be_bytes());
    bytes.extend(division.to_be_bytes());
    for track in tracks {
        bytes.extend(b"MTrk");
        bytes.extend((track.len() as u32).to_be_bytes());
        bytes.extend(*track);
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempo_map_and_running_status() {
        let tempo = [
            0, 0xff, 0x51, 3, 0x07, 0xa1, 0x20, 0x83, 0x60, 0xff, 0x51, 3, 0x0f, 0x42, 0x40,
        ];
        let notes = [
            0, 0x90, 60, 100, 0, 64, 70, 0x87, 0x40, 60, 0, 0, 64, 0, 0, 0xb0, 64, 127,
        ];
        let midi = Midi::parse(&fixture(1, 480, &[&tempo, &notes])).unwrap();
        assert_eq!(midi.tracks[1].note_count, 2);
        assert_eq!(midi.bpm, 120.0);
        assert_eq!(midi.duration, 1.5);
        assert_eq!(midi.notes[0].end, 1.5);
        assert!(midi.pedal[0].down);
    }

    #[test]
    fn rejects_invalid_and_cross_track_reads() {
        assert!(Midi::parse(&fixture(1, 480, &[&[0, 0x90, 60], &[0, 0xff, 0x2f, 0]])).is_err());
        assert!(Midi::parse(&fixture(0, 0, &[&[]])).is_err());
        assert!(Midi::parse(&fixture(2, 480, &[&[]])).is_err());
        assert!(Midi::parse(&fixture(0, 480, &[&[0x80, 0x80, 0x80, 0x80, 0]])).is_err());
    }

    #[test]
    fn supports_drop_frame_smpte() {
        let midi = Midi::parse(&fixture(
            0,
            0xe364,
            &[&[0, 0x90, 60, 100, 0x87, 0x68, 0x80, 60, 0]],
        ))
        .unwrap();
        assert!((midi.duration - 1001.0 / 3000.0).abs() < 1e-12);
    }
}
