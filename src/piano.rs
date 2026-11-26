use std::{io::Read, sync::OnceLock};

pub const RATE: u32 = 48_000;
const SAMPLE_RATE: u32 = 24_000;
const MAX_VOICES: usize = 64;
const LAYERS: usize = 3;

struct Bank {
    samples: Vec<Vec<i16>>,
    pitches: [(usize, f32); 88],
}

impl Bank {
    fn load() -> Option<Self> {
        let mut data = include_bytes!("../assets/piano.flacbank").strip_prefix(b"AP01")?;
        let (&count, rest) = data.split_first()?;
        data = rest;
        let mut roots = Vec::with_capacity(count.into());
        let mut samples = Vec::with_capacity(usize::from(count) * LAYERS);
        for _ in 0..count {
            let (&root, rest) = data.split_first()?;
            roots.push(root);
            data = rest;
            for _ in 0..LAYERS {
                let mut size = [0; 4];
                data.read_exact(&mut size).ok()?;
                let (bytes, rest) = data.split_at_checked(u32::from_le_bytes(size) as usize)?;
                let mut reader = claxon::FlacReader::new(bytes).ok()?;
                let info = reader.streaminfo();
                if info.channels != 1
                    || info.bits_per_sample != 16
                    || info.sample_rate != SAMPLE_RATE
                {
                    return None;
                }
                samples.push(
                    reader
                        .samples()
                        .map(|sample| sample.map(|n| n as i16))
                        .collect::<Result<Vec<_>, _>>()
                        .ok()?,
                );
                data = rest;
            }
        }
        if roots.is_empty() || !data.is_empty() {
            return None;
        }
        let pitches = std::array::from_fn(|index| {
            let pitch = index as u8 + 21;
            let (group, &root) = roots
                .iter()
                .enumerate()
                .min_by_key(|(_, root)| pitch.abs_diff(**root))
                .unwrap();
            let rate = 2.0_f32.powf((f32::from(pitch) - f32::from(root)) / 12.0);
            (group * LAYERS, rate * SAMPLE_RATE as f32 / RATE as f32)
        });
        Some(Self { samples, pitches })
    }
}

struct Note {
    pitch: u8,
    sample: usize,
    step: f32,
    position: f32,
    level: f32,
    released: bool,
}

pub struct Piano {
    bank: &'static Bank,
    notes: Vec<Note>,
    pedal: bool,
    room: [Reflection; 4],
}

impl Piano {
    pub fn new() -> Option<Self> {
        static BANK: OnceLock<Option<Bank>> = OnceLock::new();
        Some(Self {
            bank: BANK.get_or_init(Bank::load).as_ref()?,
            notes: Vec::with_capacity(MAX_VOICES),
            pedal: false,
            room: [29, 37, 43, 53].map(Reflection::new),
        })
    }

    pub fn reset(&mut self) {
        self.notes.clear();
        self.pedal = false;
        for reflection in &mut self.room {
            reflection.buffer.fill(0.0);
            reflection.index = 0;
            reflection.damped = 0.0;
        }
    }

    pub fn press(&mut self, pitch: u8, velocity: u8) {
        let (group, step) = self.bank.pitches[(pitch - 21) as usize];
        if self.notes.len() == MAX_VOICES {
            let oldest = self
                .notes
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.position.total_cmp(&b.position))
                .map_or(0, |(index, _)| index);
            self.notes.swap_remove(oldest);
        }
        let layer = match velocity {
            0..=52 => 0,
            53..=91 => 1,
            _ => 2,
        };
        let strength = f32::from(velocity) / 127.0;
        self.notes.push(Note {
            pitch,
            sample: group + layer,
            step,
            position: 0.0,
            level: strength.powf(1.35) * 0.27,
            released: false,
        });
    }

    pub fn release(&mut self, pitch: u8) {
        for note in &mut self.notes {
            if note.pitch == pitch {
                note.released = true;
            }
        }
    }

    pub fn sustain(&mut self, down: bool) {
        self.pedal = down;
    }

    pub fn finish(&mut self) {
        self.pedal = false;
        for note in &mut self.notes {
            note.released = true;
        }
    }

    pub fn frame(&mut self, room: f32) -> f32 {
        let mut dry = 0.0;
        self.notes.retain_mut(|note| {
            let samples = &self.bank.samples[note.sample];
            let index = note.position as usize;
            let Some(pair) = samples.get(index..index + 2) else {
                return false;
            };
            let fraction = note.position - index as f32;
            let sample = f32::from(pair[0]) * (1.0 - fraction) + f32::from(pair[1]) * fraction;
            dry += sample * (note.level / 32_768.0);
            note.position += note.step;
            if note.released && !self.pedal {
                note.level *= 0.9995;
            }
            note.level > 0.000_08
        });
        let reflection = self
            .room
            .iter_mut()
            .map(|delay| delay.process(dry))
            .sum::<f32>();
        dry + reflection * (room * 0.25)
    }
}

struct Reflection {
    buffer: Vec<f32>,
    index: usize,
    damped: f32,
}

impl Reflection {
    fn new(delay_ms: usize) -> Self {
        Self {
            buffer: vec![0.0; RATE as usize * delay_ms / 1000],
            index: 0,
            damped: 0.0,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let delayed = self.buffer[self.index];
        self.damped += (delayed - self.damped) * 0.20;
        let feedback = self.damped * 0.64;
        self.buffer[self.index] = input + feedback;
        self.index = (self.index + 1) % self.buffer.len();
        delayed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(piano: &mut Piano, frames: usize, room: f32) -> f32 {
        (0..frames).fold(0.0_f32, |peak, _| peak.max(piano.frame(room).abs()))
    }

    #[test]
    fn renders_the_sampled_keyboard_with_velocity_and_sustain() {
        let mut piano = Piano::new().expect("bundled acoustic sample bank");
        for pitch in [21, 36, 60, 84, 108] {
            piano.press(pitch, 90);
            assert!(peak(&mut piano, 4_800, 0.0) > 0.02);
            piano.finish();
            peak(&mut piano, 24_000, 0.0);
        }
        piano.press(60, 35);
        let soft = peak(&mut piano, 4_800, 0.0);
        piano.finish();
        peak(&mut piano, 24_000, 0.0);
        piano.press(60, 115);
        assert!(peak(&mut piano, 4_800, 0.0) > soft * 2.0);
        piano.sustain(true);
        piano.release(60);
        assert!(peak(&mut piano, 9_600, 0.18) > 0.01);
        piano.sustain(false);
        peak(&mut piano, 48_000, 0.18);
        assert!(peak(&mut piano, 4_800, 0.18) < 0.0001);
    }
}
