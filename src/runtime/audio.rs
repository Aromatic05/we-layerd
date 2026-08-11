use std::sync::Arc;

use anyhow::{Context, Result};
use libpulse_binding::{
    sample::{Format, Spec},
    stream::Direction,
};
use libpulse_simple_binding::Simple;

pub(crate) const AUDIO_SPECTRUM_BINS: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StereoSpectrum {
    pub(crate) left: [f32; AUDIO_SPECTRUM_BINS],
    pub(crate) right: [f32; AUDIO_SPECTRUM_BINS],
}

impl StereoSpectrum {
    pub(crate) fn flattened(&self) -> Arc<[f32]> {
        let mut values = Vec::with_capacity(AUDIO_SPECTRUM_BINS * 2);
        values.extend_from_slice(&self.left);
        values.extend_from_slice(&self.right);
        values.into()
    }
}

pub(crate) fn spectrum_from_interleaved_pcm(samples: &[f32], sample_rate: u32) -> StereoSpectrum {
    let mut left = [0.0; AUDIO_SPECTRUM_BINS];
    let mut right = [0.0; AUDIO_SPECTRUM_BINS];
    if sample_rate == 0 || samples.len() < 4 {
        return StereoSpectrum { left, right };
    }

    let frame_count = (samples.len() / 2).min(4096);
    if frame_count < 2 {
        return StereoSpectrum { left, right };
    }
    let start_frame = samples.len() / 2 - frame_count;
    let mut window_sum = 0.0_f32;
    let mut window = Vec::with_capacity(frame_count);
    for index in 0..frame_count {
        let phase = std::f32::consts::TAU * index as f32 / (frame_count - 1) as f32;
        let value = 0.5 - 0.5 * phase.cos();
        window.push(value);
        window_sum += value;
    }
    if window_sum <= f32::EPSILON {
        return StereoSpectrum { left, right };
    }

    let nyquist = sample_rate as f32 * 0.5;
    for exported_bin in 0..AUDIO_SPECTRUM_BINS {
        let frequency = nyquist * exported_bin as f32 / AUDIO_SPECTRUM_BINS as f32;
        let omega = std::f32::consts::TAU * frequency / sample_rate as f32;
        let coefficient = 2.0 * omega.cos();
        let mut left_prev = 0.0_f32;
        let mut left_prev2 = 0.0_f32;
        let mut right_prev = 0.0_f32;
        let mut right_prev2 = 0.0_f32;

        for (offset, weight) in window.iter().copied().enumerate() {
            let frame = start_frame + offset;
            let left_sample = samples[frame * 2] * weight;
            let right_sample = samples[frame * 2 + 1] * weight;
            let left_current = left_sample + coefficient * left_prev - left_prev2;
            left_prev2 = left_prev;
            left_prev = left_current;
            let right_current = right_sample + coefficient * right_prev - right_prev2;
            right_prev2 = right_prev;
            right_prev = right_current;
        }

        let normalize = 2.0 / window_sum;
        let left_power =
            left_prev2 * left_prev2 + left_prev * left_prev - coefficient * left_prev * left_prev2;
        let right_power = right_prev2 * right_prev2 + right_prev * right_prev
            - coefficient * right_prev * right_prev2;
        left[exported_bin] = (left_power.max(0.0).sqrt() * normalize).clamp(0.0, 1.0);
        right[exported_bin] = (right_power.max(0.0).sqrt() * normalize).clamp(0.0, 1.0);
    }

    StereoSpectrum { left, right }
}

pub(crate) struct PulseAudioCapture {
    simple: Simple,
    sample_rate: u32,
    frame_count: usize,
    bytes: Vec<u8>,
}

impl PulseAudioCapture {
    pub(crate) fn connect(source: &str, sample_rate: u32, update_hz: u32) -> Result<Self> {
        let sample_rate = sample_rate.clamp(8_000, 192_000);
        let update_hz = update_hz.clamp(5, 60);
        let spec = Spec { format: Format::FLOAT32NE, channels: 2, rate: sample_rate };
        if !spec.is_valid() {
            anyhow::bail!("invalid PulseAudio sample specification");
        }
        let simple = Simple::new(
            None,
            "we-layerd",
            Direction::Record,
            Some(source),
            "Wallpaper audio spectrum",
            &spec,
            None,
            None,
        )
        .context("failed to open PulseAudio monitor source")?;
        let frame_count = (sample_rate / update_hz).clamp(128, 4096) as usize;
        Ok(Self {
            simple,
            sample_rate,
            frame_count,
            bytes: vec![0; frame_count * 2 * std::mem::size_of::<f32>()],
        })
    }

    pub(crate) fn read_spectrum(&mut self) -> Result<Arc<[f32]>> {
        self.simple.read(&mut self.bytes).context("failed to read PulseAudio monitor samples")?;
        let mut samples = Vec::with_capacity(self.frame_count * 2);
        for bytes in self.bytes.chunks_exact(4) {
            samples.push(f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
        }
        Ok(spectrum_from_interleaved_pcm(&samples, self.sample_rate).flattened())
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::{spectrum_from_interleaved_pcm, AUDIO_SPECTRUM_BINS};

    fn stereo_sine(frequency_hz: f32, sample_rate: u32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|index| {
                let sample = (TAU * frequency_hz * index as f32 / sample_rate as f32).sin();
                [sample, sample]
            })
            .collect()
    }

    #[test]
    fn silence_produces_zero_spectrum_in_renderer_channel_order() {
        let spectrum = spectrum_from_interleaved_pcm(&vec![0.0; 2048 * 2], 48_000);
        assert!(spectrum.left.iter().all(|value| *value == 0.0));
        assert!(spectrum.right.iter().all(|value| *value == 0.0));

        let flattened = spectrum.flattened();
        assert_eq!(flattened.len(), AUDIO_SPECTRUM_BINS * 2);
        assert_eq!(&flattened[..AUDIO_SPECTRUM_BINS], spectrum.left.as_slice());
        assert_eq!(&flattened[AUDIO_SPECTRUM_BINS..], spectrum.right.as_slice());
    }

    #[test]
    fn sine_energy_is_concentrated_near_its_expected_frequency_bin() {
        let sample_rate = 48_000;
        let spectrum =
            spectrum_from_interleaved_pcm(&stereo_sine(1_500.0, sample_rate, 2048), sample_rate);
        let peak = spectrum
            .left
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| left.total_cmp(right))
            .map(|(index, _)| index)
            .expect("spectrum has bins");

        // The 64 exported bins cover 0..Nyquist linearly, so 1.5 kHz lands near bin 4.
        assert!((3..=5).contains(&peak), "unexpected 1.5 kHz peak bin {peak}");
        assert!(spectrum.left[peak] > 0.5);
        assert!((spectrum.left[peak] - spectrum.right[peak]).abs() < 0.001);
    }
}
