use crate::{
    humanize::Humanizer,
    keys,
    midi::{Midi, Note},
    settings::{OutOfRange, Output, Settings, Sustain},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    Release,
    PedalUp,
    PedalDown,
    Press,
}

#[derive(Clone, Copy, Debug)]
pub struct Event {
    pub time: f64,
    pub action: Action,
    pub pitch: u8,
    pub velocity: u8,
    pub id: usize,
}

pub fn included(note: &Note, tracks: &[bool], settings: &Settings) -> bool {
    tracks.get(note.track).copied().unwrap_or(false) && !(settings.skip_drums && note.channel == 9)
}

pub fn build(
    midi: &Midi,
    tracks: &[bool],
    transpose: i32,
    settings: &Settings,
    human: &mut Humanizer,
) -> Vec<Event> {
    let voice = settings.output == Output::Voice;
    let (hold_min, hold_max) = if voice {
        (0.005, f64::INFINITY)
    } else {
        let min = settings.min_hold_ms / 1000.0;
        (min, (settings.max_hold_ms / 1000.0).max(min))
    };
    let layout = settings.playback_layout();
    let mut breathing = vec![(0.0, 0.0)];
    let mut notes = midi
        .notes
        .iter()
        .filter(|note| included(note, tracks, settings))
        .peekable();
    let mut events = Vec::with_capacity(midi.notes.len() * 2);
    let mut chord = Vec::with_capacity(16);
    let mut offsets = [0.0; 128];
    let (mut shift, mut latest_end): (f64, f64) = (0.0, 0.0);
    while let Some(first) = notes.next() {
        let group_start = first.start;
        let mut used = 0u128;
        chord.clear();
        let mut next = Some(first);
        while let Some(note) = next {
            if let Some(pitch) = keys::fit(
                i32::from(note.pitch) + transpose,
                layout,
                settings.out_of_range,
            ) {
                let bit = 1u128 << pitch;
                if used & bit == 0 {
                    chord.push((pitch, note));
                    used |= bit;
                }
            }
            next = notes.next_if(|n| n.start - group_start < 0.025);
        }
        chord.sort_unstable_by_key(|(pitch, _)| *pitch);

        if latest_end > 0.0 && group_start > latest_end {
            let pause = human.breath(group_start - latest_end);
            if pause > 0.0 {
                shift += pause;
                breathing.push((group_start, shift));
            }
        }

        human.chord_roll(&mut offsets[..chord.len()]);
        let jitter = human.timing_offset();
        for (&(pitch, note), roll) in chord.iter().zip(offsets) {
            let mut start =
                (note.start + shift + jitter + human.timing_offset() * 0.35 + roll).max(0.0);
            if let Some(direction) = human.mistake()
                && let Some(wrong) =
                    keys::fit(i32::from(pitch) + direction, layout, OutOfRange::Skip)
            {
                let duration = human.mistake_length();
                note_events(
                    &mut events,
                    start,
                    duration * 0.7,
                    wrong,
                    note.velocity / 2 + 1,
                );
                start += duration;
            }
            let duration = ((note.end - note.start) * human.hold_scale()).clamp(hold_min, hold_max);
            note_events(
                &mut events,
                start,
                duration,
                pitch,
                human.velocity(note.velocity),
            );
            latest_end = latest_end.max(note.end);
        }
    }

    if voice || settings.sustain == Sustain::FollowPedal {
        for pedal in &midi.pedal {
            if tracks.get(pedal.track) == Some(&true) {
                let mark = breathing.partition_point(|&(time, _)| time <= pedal.time) - 1;
                events.push(Event {
                    time: pedal.time + breathing[mark].1,
                    action: if pedal.down {
                        Action::PedalDown
                    } else {
                        Action::PedalUp
                    },
                    pitch: 0,
                    velocity: 0,
                    id: events.len(),
                });
            }
        }
    }
    events.sort_unstable_by(|a, b| {
        a.time
            .total_cmp(&b.time)
            .then(a.action.cmp(&b.action))
            .then(a.id.cmp(&b.id))
    });
    events
}

fn note_events(events: &mut Vec<Event>, start: f64, duration: f64, pitch: u8, velocity: u8) {
    let id = events.len();
    events.push(Event {
        time: start,
        action: Action::Press,
        pitch,
        velocity,
        id,
    });
    events.push(Event {
        time: start + duration,
        action: Action::Release,
        pitch,
        velocity: 0,
        id,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi::{Pedal, fixture};

    #[test]
    fn disabled_humanize_keeps_chords_together_and_filters_drums() {
        let track = [
            0, 0x90, 60, 90, 0, 64, 90, 0, 0x99, 62, 80, 24, 0x90, 60, 0, 0, 64, 0,
        ];
        let midi = Midi::parse(&fixture(0, 480, &[&track])).unwrap();
        let mut settings = Settings::default();
        settings.humanize.enabled = false;
        let mut human = Humanizer::new(settings.humanize.clone(), 7);
        let events = build(&midi, &[true], 0, &settings, &mut human);
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].pitch, 60);
        assert_eq!(events[1].pitch, 64);
        assert_eq!(events[0].time, events[1].time);
        assert_eq!(events[2].time, 0.025);
        assert_eq!(events[0].id, events[2].id);
    }

    #[test]
    fn voice_keeps_the_full_piano_range_and_midi_pedal() {
        let midi = Midi {
            notes: [21, 108]
                .map(|pitch| Note {
                    track: 0,
                    channel: 0,
                    pitch,
                    velocity: 90,
                    start: 0.0,
                    end: 6.0,
                })
                .to_vec(),
            pedal: vec![
                Pedal {
                    track: 0,
                    time: 1.0,
                    down: true,
                },
                Pedal {
                    track: 0,
                    time: 7.0,
                    down: false,
                },
            ],
            tracks: Vec::new(),
            duration: 7.0,
            bpm: 120.0,
        };
        let mut settings = Settings {
            output: Output::Voice,
            layout: crate::settings::Layout::Keys61,
            ..Settings::default()
        };
        settings.humanize.enabled = false;
        let mut human = Humanizer::new(settings.humanize.clone(), 7);
        let events = build(&midi, &[true], 0, &settings, &mut human);
        assert_eq!(events[0].pitch, 21);
        assert_eq!(events[1].pitch, 108);
        assert_eq!(events[2].action, Action::PedalDown);
        assert_eq!(events[3].time, 6.0);
        assert_eq!(events[5].action, Action::PedalUp);
    }
}
