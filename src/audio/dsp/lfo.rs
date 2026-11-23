use rand::{RngExt, rngs::StdRng};

#[allow(clippy::upper_case_acronyms)]
pub struct LFO {
    phase: f64,
    phase_increment: f64,
    base_phase_increment: f64,
    jitter_amount: f64,
    sample_rate: u32,
    rng: StdRng,
    frames_until_jitter: u32,
    // Amplitude jitter for tremolo effect
    amplitude_jitter_amount: f64,
    frames_until_amplitude_jitter: u32,
    current_amplitude_scale: f64,
}

impl LFO {
    pub fn new(sample_rate: u32, frequency: f32, amplitude_jitter_amount: f32) -> Self {
        let phase_increment = (frequency as f64) / (sample_rate as f64);
        Self {
            phase: 0.0,
            phase_increment,
            base_phase_increment: phase_increment,
            jitter_amount: 0.7,
            sample_rate,
            rng: rand::make_rng(),
            frames_until_jitter: 0,
            amplitude_jitter_amount: amplitude_jitter_amount as f64,
            frames_until_amplitude_jitter: 0,
            current_amplitude_scale: 1.0,
        }
    }

    pub fn set_frequency(&mut self, frequency: f32) {
        self.base_phase_increment = (frequency as f64) / (self.sample_rate as f64);
        self.phase_increment = self.base_phase_increment;
    }

    pub fn set_amplitude_jitter(&mut self, amount: f32) {
        self.amplitude_jitter_amount = amount as f64;
    }

    pub fn next(&mut self) -> f32 {
        // Apply frequency jitter
        if self.frames_until_jitter == 0 {
            let jitter_range = self.base_phase_increment * self.jitter_amount;
            let random_offset = (self.rng.random::<f64>() - 0.5) * 2.0 * jitter_range;
            self.phase_increment = self.base_phase_increment + random_offset;

            let jitter_interval_ms = self.rng.random_range(50.0..=200.0);
            self.frames_until_jitter =
                (jitter_interval_ms / 1000.0 * self.sample_rate as f64) as u32;
        } else {
            self.frames_until_jitter -= 1;
        }

        // Apply amplitude jitter
        if self.frames_until_amplitude_jitter == 0 {
            // Vary the amplitude scale randomly
            let random_variation =
                (self.rng.random::<f64>() - 0.5) * 2.0 * self.amplitude_jitter_amount;
            self.current_amplitude_scale = (1.0 + random_variation).clamp(0.5, 1.5);

            // Update amplitude jitter at a different rate (30-150ms intervals)
            let amplitude_jitter_interval_ms = self.rng.random_range(30.0..=150.0);
            self.frames_until_amplitude_jitter =
                (amplitude_jitter_interval_ms / 1000.0 * self.sample_rate as f64) as u32;
        } else {
            self.frames_until_amplitude_jitter -= 1;
        }

        // Calculate base LFO value (triangle wave from -1 to 1)
        let value = if self.phase < 0.5 {
            self.phase * 4.0 - 1.0
        } else {
            3.0 - self.phase * 4.0
        };

        self.phase += self.phase_increment;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        // Apply amplitude jitter to the output
        (value * self.current_amplitude_scale) as f32
    }
}
