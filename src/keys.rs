use crate::settings::{Layout, OutOfRange};

const STANDARD: &[u8] = b"1!2@34$5%6^78*9(0qQwWeErtTyYuiIoOpPasSdDfgGhHjJklLzZxcCvVbBnm";
const LOW: &[u8] = b"1234567890qwert";
const HIGH: &[u8] = b"yuiopasdfghj";
const VELOCITY: &[u8] = b"1234567890qwertyuiopasdfghjklzxc";
const SHIFTED_DIGITS: &[u8] = b")!@#$%^&*(";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Modifier {
    None,
    Shift,
    Ctrl,
    Alt,
}

#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub base: u8,
    pub label: u8,
    pub modifier: Modifier,
}

impl Key {
    pub fn display(self) -> String {
        if self.modifier == Modifier::Ctrl {
            format!("^{}", self.label as char)
        } else {
            (self.label as char).to_string()
        }
    }
}

pub fn lookup(pitch: u8) -> Option<Key> {
    let (base, label, modifier) = match pitch {
        21..=35 => {
            let key = LOW[(pitch - 21) as usize];
            (key, key, Modifier::Ctrl)
        }
        36..=96 => {
            let label = STANDARD[(pitch - 36) as usize];
            if let Some(index) = SHIFTED_DIGITS.iter().position(|&key| key == label) {
                (b'0' + index as u8, label, Modifier::Shift)
            } else if label.is_ascii_uppercase() {
                (label.to_ascii_lowercase(), label, Modifier::Shift)
            } else {
                (label, label, Modifier::None)
            }
        }
        97..=108 => {
            let key = HIGH[(pitch - 97) as usize];
            (key, key, Modifier::Ctrl)
        }
        _ => return None,
    };
    Some(Key {
        base,
        label,
        modifier,
    })
}

pub fn range(layout: Layout) -> (i32, i32) {
    match layout {
        Layout::Keys61 => (36, 96),
        Layout::Keys88 => (21, 108),
    }
}

pub fn fit(pitch: i32, layout: Layout, mode: OutOfRange) -> Option<u8> {
    let (low, high) = range(layout);
    if (low..=high).contains(&pitch) {
        return Some(pitch as u8);
    }
    match mode {
        OutOfRange::Skip => None,
        OutOfRange::Fold if pitch < low => {
            Some((low + (pitch.rem_euclid(12) - low).rem_euclid(12)) as u8)
        }
        OutOfRange::Fold => Some((high - (high - pitch.rem_euclid(12)).rem_euclid(12)) as u8),
    }
}

pub fn is_black(pitch: i32) -> bool {
    matches!(pitch.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

pub fn velocity_bucket(velocity: u8) -> usize {
    (velocity as usize * VELOCITY.len() / 128).min(VELOCITY.len() - 1)
}

pub fn velocity_key(bucket: usize) -> Key {
    let base = VELOCITY[bucket];
    Key {
        base,
        label: base,
        modifier: Modifier::Alt,
    }
}

pub fn best_transpose(pitches: impl Iterator<Item = u8>, layout: Layout) -> i32 {
    let mut counts = [0usize; 128];
    for pitch in pitches {
        counts[pitch as usize] += 1;
    }
    let (low, high) = range(layout);
    let mut best = (0, (0, 0));
    for shift in std::iter::once(0).chain((1..=12).flat_map(|n| [-n, n])) {
        let mut score = (0, 0);
        for (pitch, count) in counts.iter().enumerate() {
            let pitch = pitch as i32 + shift;
            if (low..=high).contains(&pitch) {
                score.0 += count;
                if !is_black(pitch) {
                    score.1 += count;
                }
            }
        }
        if score > best.1 {
            best = (shift, score);
        }
    }
    best.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_standard_and_extended_keys() {
        assert_eq!(lookup(37).unwrap().label, b'!');
        assert_eq!(lookup(37).unwrap().base, b'1');
        assert_eq!(lookup(108).unwrap().modifier, Modifier::Ctrl);
        assert_eq!(fit(108, Layout::Keys61, OutOfRange::Fold), Some(96));
        assert_eq!(fit(0, Layout::Keys61, OutOfRange::Fold), Some(36));
        assert_eq!(fit(0, Layout::Keys61, OutOfRange::Skip), None);
        assert_eq!(best_transpose([36, 38, 40].into_iter(), Layout::Keys61), 0);
    }
}
