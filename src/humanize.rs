use crate::settings::Humanize;
use std::f64::consts::TAU;

pub struct Humanizer {
    settings: Humanize,
    random_state: u64,
    drift: [(f64, f64); 3],
}

impl Humanizer {
    pub fn new(settings: Humanize, seed: u64) -> Self {
        let mut this = Self {
            settings,
            random_state: seed,
            drift: [(0.0, 0.0); 3],
        };
        for index in 0..this.drift.len() {
            this.drift[index] = ((0.02 + this.random() * 0.08) * TAU, this.random() * TAU);
        }
        this
    }

    fn random(&mut self) -> f64 {
        self.random_state = self.random_state.wrapping_add(0x9e3779b97f4a7c15);
        let mut n = self.random_state;
        n = (n ^ (n >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        n = (n ^ (n >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((n ^ (n >> 31)) >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    fn gaussian(&mut self) -> f64 {
        (-2.0 * (1.0 - self.random()).ln()).sqrt() * (TAU * self.random()).cos()
    }

    pub fn timing_offset(&mut self) -> f64 {
        let jitter = self.settings.timing_jitter_ms;
        if !self.settings.enabled || jitter <= 0.0 {
            return 0.0;
        }
        (self.gaussian() * 0.5).clamp(-1.0, 1.0) * jitter / 1000.0
    }

    pub fn chord_roll(&mut self, offsets: &mut [f64]) {
        offsets.fill(0.0);
        let spread = self.settings.chord_roll_ms;
        if !self.settings.enabled || spread <= 0.0 || offsets.len() < 2 {
            return;
        }
        let up = self.random() < 0.75;
        let step = spread * (0.35 + self.random() * 0.65) / (offsets.len() - 1) as f64;
        for index in 0..offsets.len() {
            let ms = (index as f64 + (self.random() - 0.5) * 0.6) * step;
            let target = if up { index } else { offsets.len() - 1 - index };
            offsets[target] = ms.max(0.0) / 1000.0;
        }
    }

    pub fn hold_scale(&mut self) -> f64 {
        let amount = self.settings.hold_variance_percent / 100.0;
        if !self.settings.enabled || amount <= 0.0 {
            return 1.0;
        }
        1.0 + (self.gaussian() * 0.5).clamp(-1.0, 1.0) * amount
    }

    pub fn velocity(&mut self, velocity: u8) -> u8 {
        let amount = self.settings.velocity_variance as f64;
        if !self.settings.enabled || amount <= 0.0 {
            return velocity;
        }
        (i32::from(velocity) + (self.gaussian() * amount * 0.5).round_ties_even() as i32)
            .clamp(1, 127) as u8
    }

    pub fn breath(&mut self, rest: f64) -> f64 {
        if !self.settings.enabled || self.settings.phrase_breath_ms <= 0.0 || rest < 0.45 {
            return 0.0;
        }
        let weight = ((rest - 0.45) / 1.5 + 0.3).min(1.0);
        self.random() * self.settings.phrase_breath_ms * weight / 1000.0
    }

    pub fn mistake(&mut self) -> Option<i32> {
        if !self.settings.enabled
            || self.settings.mistake_chance_percent <= 0.0
            || self.random() * 100.0 >= self.settings.mistake_chance_percent
        {
            return None;
        }
        Some(if self.random() < 0.5 { -1 } else { 1 })
    }

    pub fn mistake_length(&mut self) -> f64 {
        (18.0 + self.random() * 30.0) / 1000.0
    }

    pub fn tempo_factor(&self, time: f64) -> f64 {
        if !self.settings.enabled || self.settings.tempo_drift_percent <= 0.0 {
            return 1.0;
        }
        let drift = self
            .drift
            .iter()
            .map(|(freq, phase)| (time * freq + phase).sin())
            .sum::<f64>();
        1.0 + drift * self.settings.tempo_drift_percent / 300.0
    }
}
