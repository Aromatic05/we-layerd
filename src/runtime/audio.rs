use std::sync::Arc;

pub(crate) const AUDIO_SPECTRUM_BINS: usize = 64;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StereoSpectrum {
    pub(crate) left: [f32; AUDIO_SPECTRUM_BINS],
    pub(crate) right: [f32; AUDIO_SPECTRUM_BINS],
}

impl StereoSpectrum {
    pub(crate) fn flattened(&self) -> Arc<[f32]> {
        todo!("implemented after the behavior tests are established")
    }
}

pub(crate) fn spectrum_from_interleaved_pcm(_samples: &[f32], _sample_rate: u32) -> StereoSpectrum {
    todo!("implemented after the behavior tests are established")
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
