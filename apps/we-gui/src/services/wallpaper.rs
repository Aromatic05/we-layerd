use std::{fs::File, io::BufReader, path::PathBuf, time::Duration};

use image_rs::AnimationDecoder;
use we_core::{
    steam,
    wallpaper::{self, WallpaperEntry},
};

use crate::domain::ui_state::GifFrame;

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
    let decoder = image_rs::codecs::gif::GifDecoder::new(BufReader::new(
        File::open(path).map_err(|error| error.to_string())?,
    ))
    .map_err(|error| error.to_string())?;
    decoder.into_frames().collect_frames().map_err(|error| error.to_string()).map(|frames| {
        frames
            .into_iter()
            .map(|frame| {
                let (numerator, denominator) = frame.delay().numer_denom_ms();
                let buffer = frame.into_buffer();
                GifFrame {
                    width: buffer.width(),
                    height: buffer.height(),
                    pixels: buffer.into_raw(),
                    delay: Duration::from_millis((numerator / denominator.max(1)).max(16).into()),
                }
            })
            .collect()
    })
}
