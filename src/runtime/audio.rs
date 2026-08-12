use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use libpulse_binding::{
    context::{Context as PulseContext, FlagSet as ContextFlagSet, State as ContextState},
    def::BufferAttr,
    mainloop::standard::{IterateResult, Mainloop},
    sample::{Format, Spec},
    stream::{FlagSet as StreamFlagSet, PeekResult, State as StreamState, Stream},
};

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

fn append_recent_pcm_bytes(retained: &mut Vec<f32>, bytes: &[u8], max_samples: usize) {
    if max_samples == 0 {
        retained.clear();
        return;
    }

    let chunks = bytes.chunks_exact(std::mem::size_of::<f32>());
    let incoming_samples = chunks.len();
    if incoming_samples >= max_samples {
        retained.clear();
        retained.extend(
            chunks
                .skip(incoming_samples - max_samples)
                .map(|bytes| f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])),
        );
        return;
    }

    let overflow = retained.len().saturating_add(incoming_samples).saturating_sub(max_samples);
    if overflow > 0 {
        retained.drain(..overflow.min(retained.len()));
    }
    retained
        .extend(chunks.map(|bytes| f32::from_ne_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])));
}

fn append_recent_silence(retained: &mut Vec<f32>, incoming_samples: usize, max_samples: usize) {
    if max_samples == 0 {
        retained.clear();
        return;
    }
    if incoming_samples >= max_samples {
        retained.clear();
        retained.resize(max_samples, 0.0);
        return;
    }

    let overflow = retained.len().saturating_add(incoming_samples).saturating_sub(max_samples);
    if overflow > 0 {
        retained.drain(..overflow.min(retained.len()));
    }
    retained.resize(retained.len() + incoming_samples, 0.0);
}

pub(crate) struct PulseAudioCapture {
    // Keep the dependency objects in destruction order: stream -> context -> mainloop.
    stream: Stream,
    context: PulseContext,
    mainloop: Mainloop,
    sample_rate: u32,
    frame_count: usize,
    samples: Vec<f32>,
}

impl PulseAudioCapture {
    pub(crate) fn connect(
        source: &str,
        sample_rate: u32,
        update_hz: u32,
        mut cancelled: impl FnMut() -> bool,
    ) -> Result<Option<Self>> {
        if cancelled() {
            return Ok(None);
        }
        let sample_rate = sample_rate.clamp(8_000, 192_000);
        let update_hz = update_hz.clamp(5, 60);
        let spec = Spec { format: Format::FLOAT32NE, channels: 2, rate: sample_rate };
        if !spec.is_valid() {
            anyhow::bail!("invalid PulseAudio sample specification");
        }
        let frame_count = (sample_rate / update_hz).clamp(128, 4096) as usize;
        let fragment_bytes = frame_count
            .checked_mul(2)
            .and_then(|samples| samples.checked_mul(std::mem::size_of::<f32>()))
            .and_then(|bytes| u32::try_from(bytes).ok())
            .context("PulseAudio fragment size overflow")?;

        let mut mainloop = Mainloop::new().context("failed to create PulseAudio mainloop")?;
        let mut context = PulseContext::new(&mainloop, "we-layerd")
            .context("failed to create PulseAudio context")?;
        context
            .connect(None, ContextFlagSet::NOFLAGS, None)
            .context("failed to start PulseAudio context connection")?;
        if !drive_pulse_until(&mut mainloop, Duration::from_secs(2), &mut cancelled, || {
            match context.get_state() {
                ContextState::Ready => Ok(true),
                ContextState::Failed | ContextState::Terminated => {
                    Err(anyhow::anyhow!("PulseAudio context failed: {}", context.errno()))
                }
                _ => Ok(false),
            }
        })? {
            return Ok(None);
        }

        let mut stream = Stream::new(&mut context, "Wallpaper audio spectrum", &spec, None)
            .context("failed to create PulseAudio record stream")?;
        let buffer_attr = BufferAttr {
            maxlength: fragment_bytes.saturating_mul(4),
            tlength: u32::MAX,
            prebuf: u32::MAX,
            minreq: u32::MAX,
            fragsize: fragment_bytes,
        };
        stream
            .connect_record(Some(source), Some(&buffer_attr), StreamFlagSet::ADJUST_LATENCY)
            .context("failed to connect PulseAudio monitor source")?;
        if !drive_pulse_until(
            &mut mainloop,
            Duration::from_secs(2),
            &mut cancelled,
            || match stream.get_state() {
                StreamState::Ready => Ok(true),
                StreamState::Failed | StreamState::Terminated => {
                    Err(anyhow::anyhow!("PulseAudio record stream failed"))
                }
                _ => Ok(false),
            },
        )? {
            return Ok(None);
        }

        Ok(Some(Self {
            stream,
            context,
            mainloop,
            sample_rate,
            frame_count,
            samples: Vec::with_capacity(frame_count * 2),
        }))
    }

    pub(crate) fn poll_spectrum(&mut self) -> Result<Option<Arc<[f32]>>> {
        match self.mainloop.iterate(false) {
            IterateResult::Success(_) => {}
            IterateResult::Quit(_) => anyhow::bail!("PulseAudio mainloop quit"),
            IterateResult::Err(error) => anyhow::bail!("PulseAudio mainloop failed: {error}"),
        }
        match self.context.get_state() {
            ContextState::Failed | ContextState::Terminated => {
                anyhow::bail!("PulseAudio context disconnected: {}", self.context.errno())
            }
            _ => {}
        }
        match self.stream.get_state() {
            StreamState::Failed | StreamState::Terminated => {
                anyhow::bail!("PulseAudio record stream disconnected")
            }
            _ => {}
        }

        let required_samples = self.frame_count * 2;
        loop {
            match self.stream.peek().context("failed to peek PulseAudio samples")? {
                PeekResult::Empty => break,
                PeekResult::Hole(bytes) => append_recent_silence(
                    &mut self.samples,
                    bytes / std::mem::size_of::<f32>(),
                    required_samples,
                ),
                PeekResult::Data(bytes) => {
                    append_recent_pcm_bytes(&mut self.samples, bytes, required_samples)
                }
            }
            self.stream.discard().context("failed to consume PulseAudio samples")?;
        }

        if self.samples.len() < required_samples {
            return Ok(None);
        }
        debug_assert_eq!(self.samples.len(), required_samples);
        let spectrum = spectrum_from_interleaved_pcm(&self.samples, self.sample_rate).flattened();
        self.samples.clear();
        Ok(Some(spectrum))
    }
}

impl Drop for PulseAudioCapture {
    fn drop(&mut self) {
        let _ = self.stream.disconnect();
        self.context.disconnect();
    }
}

fn drive_pulse_until(
    mainloop: &mut Mainloop,
    timeout: Duration,
    cancelled: &mut impl FnMut() -> bool,
    mut ready: impl FnMut() -> Result<bool>,
) -> Result<bool> {
    let deadline = Instant::now() + timeout;
    loop {
        if cancelled() {
            return Ok(false);
        }
        match mainloop.iterate(false) {
            IterateResult::Success(_) => {}
            IterateResult::Quit(_) => anyhow::bail!("PulseAudio mainloop quit during setup"),
            IterateResult::Err(error) => {
                anyhow::bail!("PulseAudio mainloop failed during setup: {error}")
            }
        }
        if ready()? {
            return Ok(true);
        }
        if Instant::now() >= deadline {
            anyhow::bail!("timed out while connecting PulseAudio capture");
        }
        thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::{
        append_recent_pcm_bytes, append_recent_silence, spectrum_from_interleaved_pcm,
        AUDIO_SPECTRUM_BINS,
    };

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

    #[test]
    fn pulse_backlog_retains_only_the_latest_bounded_sample_window() {
        let mut retained = vec![1.0_f32, 2.0];
        let incoming =
            (3_u32..=12).flat_map(|value| (value as f32).to_ne_bytes()).collect::<Vec<_>>();

        append_recent_pcm_bytes(&mut retained, &incoming, 4);
        assert_eq!(retained, vec![9.0, 10.0, 11.0, 12.0]);

        append_recent_silence(&mut retained, 10_000, 4);
        assert_eq!(retained, vec![0.0; 4]);
    }
}
