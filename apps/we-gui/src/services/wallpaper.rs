use std::{fs::File, io::BufReader, path::PathBuf, time::Duration};

use iced::widget::image;
use image_rs::{
    imageops::{self, FilterType},
    AnimationDecoder, ImageDecoder, Limits,
};
use we_core::{
    steam,
    wallpaper::{self, WallpaperEntry},
};

use crate::domain::ui_state::GifFrame;

const GIF_PREVIEW_MAX_WIDTH: u32 = 480;
const GIF_PREVIEW_MAX_HEIGHT: u32 = 270;
const GIF_PREVIEW_MAX_SOURCE_DIMENSION: u32 = 8_192;
const GIF_PREVIEW_DECODER_MAX_ALLOC: u64 = 128 * 1024 * 1024;
const GIF_PREVIEW_DECODED_BUDGET: usize = 12 * 1024 * 1024;
const GIF_PREVIEW_MAX_RETAINED_FRAMES: usize = 120;
const GIF_PREVIEW_MAX_DECODED_FRAMES: usize = 600;
const GIF_PREVIEW_MAX_DECODED_SOURCE_PIXELS: u64 = 512 * 1024 * 1024;

pub async fn scan(workshop_path: String) -> Result<Vec<WallpaperEntry>, String> {
    let root = if workshop_path.trim().is_empty() {
        steam::discover_workshop_wallpaper_root()
            .ok_or_else(|| "cannot find Steam workshop path for app 431960".to_string())?
    } else {
        PathBuf::from(workshop_path)
    };
    wallpaper::scan_workshop_wallpapers(&root).map_err(|error| error.to_string())
}

pub async fn decode_gif(path: PathBuf) -> Result<Vec<GifFrame>, String> {
    let mut decoder = image_rs::codecs::gif::GifDecoder::new(BufReader::new(
        File::open(path).map_err(|error| error.to_string())?,
    ))
    .map_err(|error| error.to_string())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(GIF_PREVIEW_MAX_SOURCE_DIMENSION);
    limits.max_image_height = Some(GIF_PREVIEW_MAX_SOURCE_DIMENSION);
    limits.max_alloc = Some(GIF_PREVIEW_DECODER_MAX_ALLOC);
    decoder.set_limits(limits).map_err(|error| error.to_string())?;

    let (source_width, source_height) = decoder.dimensions();
    let (target_width, target_height) = preview_dimensions(source_width, source_height);
    let mut bounded =
        BoundedGifFrames::new(GIF_PREVIEW_DECODED_BUDGET, GIF_PREVIEW_MAX_RETAINED_FRAMES);

    for frame in decoder.into_frames().take(max_gif_decode_frames(source_width, source_height)) {
        let frame = frame.map_err(|error| error.to_string())?;
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let delay = Duration::from_millis((numerator / denominator.max(1)).max(16).into());
        let source = frame.into_buffer();
        let buffer = if source.width() == target_width && source.height() == target_height {
            source
        } else {
            imageops::resize(&source, target_width, target_height, FilterType::Triangle)
        };
        let width = buffer.width();
        let height = buffer.height();
        let pixels = buffer.into_raw();
        let decoded_bytes = pixels.len();
        bounded.push(GifFrame {
            handle: image::Handle::from_rgba(width, height, pixels),
            decoded_bytes,
            delay,
        });
    }

    Ok(bounded.finish())
}

fn preview_dimensions(width: u32, height: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (1, 1);
    }
    let scale = (GIF_PREVIEW_MAX_WIDTH as f64 / width as f64)
        .min(GIF_PREVIEW_MAX_HEIGHT as f64 / height as f64)
        .min(1.0);
    (
        (width as f64 * scale).round().max(1.0) as u32,
        (height as f64 * scale).round().max(1.0) as u32,
    )
}

fn max_gif_decode_frames(width: u32, height: u32) -> usize {
    let source_pixels = u64::from(width.max(1)).saturating_mul(u64::from(height.max(1)));
    let by_pixel_work = (GIF_PREVIEW_MAX_DECODED_SOURCE_PIXELS / source_pixels).max(1) as usize;
    GIF_PREVIEW_MAX_DECODED_FRAMES.min(by_pixel_work)
}

struct BoundedGifFrames {
    max_bytes: usize,
    max_frames: usize,
    frames: Vec<GifFrame>,
    retained_bytes: usize,
    stride: usize,
    seen: usize,
}

impl BoundedGifFrames {
    fn new(max_bytes: usize, max_frames: usize) -> Self {
        Self {
            max_bytes,
            max_frames: max_frames.max(1),
            frames: Vec::new(),
            retained_bytes: 0,
            stride: 1,
            seen: 0,
        }
    }

    fn push(&mut self, frame: GifFrame) {
        let index = self.seen;
        self.seen = self.seen.saturating_add(1);
        let frame_bytes = frame.decoded_bytes;

        if self.frames.is_empty() {
            if frame_bytes <= self.max_bytes {
                self.retained_bytes = frame_bytes;
                self.frames.push(frame);
            }
            return;
        }

        if index % self.stride != 0 {
            self.frames.last_mut().expect("retained frame").delay += frame.delay;
            return;
        }

        while (self.retained_bytes.saturating_add(frame_bytes) > self.max_bytes
            || self.frames.len() >= self.max_frames)
            && self.frames.len() > 1
        {
            self.compact_once();
        }

        if index % self.stride != 0
            || self.retained_bytes.saturating_add(frame_bytes) > self.max_bytes
            || self.frames.len() >= self.max_frames
        {
            self.frames.last_mut().expect("retained frame").delay += frame.delay;
            return;
        }

        self.retained_bytes += frame_bytes;
        self.frames.push(frame);
    }

    fn compact_once(&mut self) {
        let previous = std::mem::take(&mut self.frames);
        let mut compacted = Vec::with_capacity(previous.len().div_ceil(2));
        let mut iter = previous.into_iter();
        while let Some(mut keep) = iter.next() {
            if let Some(skipped) = iter.next() {
                keep.delay += skipped.delay;
            }
            compacted.push(keep);
        }
        self.retained_bytes = compacted.iter().map(|frame| frame.decoded_bytes).sum();
        self.frames = compacted;
        self.stride = self.stride.saturating_mul(2).max(1);
    }

    fn finish(self) -> Vec<GifFrame> {
        self.frames
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use iced::widget::image;

    use super::{max_gif_decode_frames, BoundedGifFrames, GifFrame};

    fn retain_frames_with_budget(
        frames: Vec<GifFrame>,
        budget_bytes: usize,
        max_frames: usize,
    ) -> Vec<GifFrame> {
        let mut bounded = BoundedGifFrames::new(budget_bytes, max_frames);
        for frame in frames {
            bounded.push(frame);
        }
        bounded.finish()
    }

    #[test]
    fn gif_frame_budget_is_hard_and_preserves_total_animation_duration() {
        let frames = (0..16)
            .map(|index| GifFrame {
                handle: image::Handle::from_rgba(2, 2, vec![index; 16]),
                decoded_bytes: 16,
                delay: Duration::from_millis(20),
            })
            .collect::<Vec<_>>();
        let expected_duration = frames.iter().map(|frame| frame.delay).sum::<Duration>();

        let retained = retain_frames_with_budget(frames, 64, 120);
        let retained_bytes = retained.iter().map(|frame| frame.decoded_bytes).sum::<usize>();
        let retained_duration = retained.iter().map(|frame| frame.delay).sum::<Duration>();

        assert!(retained_bytes <= 64, "retained {retained_bytes} decoded bytes");
        assert_eq!(retained_duration, expected_duration);
        assert!(!retained.is_empty());
    }

    #[test]
    fn gif_frame_count_is_bounded_even_when_frames_are_tiny() {
        let frames = (0..1_000)
            .map(|index| GifFrame {
                handle: image::Handle::from_rgba(1, 1, vec![index as u8; 4]),
                decoded_bytes: 4,
                delay: Duration::from_millis(16),
            })
            .collect::<Vec<_>>();
        let expected_duration = frames.iter().map(|frame| frame.delay).sum::<Duration>();

        let retained = retain_frames_with_budget(frames, usize::MAX, 8);

        assert!(retained.len() <= 8, "retained {} tiny frames", retained.len());
        assert_eq!(retained.iter().map(|frame| frame.delay).sum::<Duration>(), expected_duration);
    }

    #[test]
    fn gif_decode_work_is_reduced_for_large_source_frames() {
        assert_eq!(max_gif_decode_frames(1, 1), 600);
        assert!(max_gif_decode_frames(3_840, 2_160) <= 64);
        assert!(max_gif_decode_frames(8_192, 8_192) <= 8);
    }
}
