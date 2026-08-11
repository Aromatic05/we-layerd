use std::sync::Arc;

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
        let radians_per_sample = std::f32::consts::TAU * frequency / sample_rate as f32;
        let mut left_re = 0.0_f32;
        let mut left_im = 0.0_f32;
        let mut right_re = 0.0_f32;
        let mut right_im = 0.0_f32;

        for (offset, weight) in window.iter().copied().enumerate() {
            let frame = start_frame + offset;
            let angle = radians_per_sample * offset as f32;
            let cos = angle.cos();
            let sin = angle.sin();
            let left_sample = samples[frame * 2] * weight;
            let right_sample = samples[frame * 2 + 1] * weight;
            left_re += left_sample * cos;
            left_im -= left_sample * sin;
            right_re += right_sample * cos;
            right_im -= right_sample * sin;
        }

        let normalize = 2.0 / window_sum;
        left[exported_bin] = (left_re.hypot(left_im) * normalize).clamp(0.0, 1.0);
        right[exported_bin] = (right_re.hypot(right_im) * normalize).clamp(0.0, 1.0);
    }

    StereoSpectrum { left, right }
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
