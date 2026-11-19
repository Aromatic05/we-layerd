//! Wallpaper-vs-display shape checks for the "aspect ratio doesn't match"
//! warning shown before applying a wallpaper.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use we_core::wallpaper::{WallpaperEntry, WallpaperType};

/// Canvas size of a scene wallpaper (`general.orthogonalprojection` in
/// scene.json, read from scene.pkg or an unpacked scene.json). `None` for
/// videos, web wallpapers and anything unreadable: their size is not stored
/// anywhere cheap to read.
pub fn scene_canvas_size(entry: &WallpaperEntry) -> Option<(u32, u32)> {
    if entry.ty != WallpaperType::Scene {
        return None;
    }
    let dir = entry.project_json.parent()?;
    let scene_json = read_pkg_entry(&dir.join("scene.pkg"), "scene.json")
        .or_else(|| std::fs::read(dir.join("scene.json")).ok())?;
    canvas_size_from_scene_json(&scene_json)
}

fn canvas_size_from_scene_json(bytes: &[u8]) -> Option<(u32, u32)> {
    let json: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let ortho = json.get("general")?.get("orthogonalprojection")?;
    let width = ortho.get("width")?.as_u64()?;
    let height = ortho.get("height")?.as_u64()?;
    (width > 0 && height > 0).then_some((width as u32, height as u32))
}

/// Reads one file out of a Wallpaper Engine PKGV package: a length-prefixed
/// version string, an entry count, then (name, offset, size) records followed
/// by the packed data.
fn read_pkg_entry(path: &Path, wanted: &str) -> Option<Vec<u8>> {
    const MAX_ENTRIES: u32 = 100_000;
    const MAX_NAME: u32 = 4096;
    const MAX_ENTRY_SIZE: u32 = 64 * 1024 * 1024;

    let mut file = File::open(path).ok()?;
    let read_u32 = |file: &mut File| -> Option<u32> {
        let mut buf = [0u8; 4];
        file.read_exact(&mut buf).ok()?;
        Some(u32::from_le_bytes(buf))
    };
    let read_string = |file: &mut File, max: u32| -> Option<String> {
        let len = read_u32(file)?;
        if len > max {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        file.read_exact(&mut buf).ok()?;
        String::from_utf8(buf).ok()
    };

    let version = read_string(&mut file, 64)?;
    if !version.starts_with("PKGV") {
        return None;
    }
    let count = read_u32(&mut file)?;
    if count > MAX_ENTRIES {
        return None;
    }
    let mut found = None;
    for _ in 0..count {
        let name = read_string(&mut file, MAX_NAME)?;
        let offset = read_u32(&mut file)?;
        let size = read_u32(&mut file)?;
        if found.is_none() && name == wanted {
            found = Some((offset, size));
        }
    }
    let (offset, size) = found?;
    if size > MAX_ENTRY_SIZE {
        return None;
    }
    let data_start = file.stream_position().ok()?;
    file.seek(SeekFrom::Start(data_start + u64::from(offset))).ok()?;
    let mut data = vec![0u8; size as usize];
    file.read_exact(&mut data).ok()?;
    Some(data)
}

/// A scene canvas at least this much taller than wide is a portrait wallpaper.
const PORTRAIT_MIN_RATIO: f64 = 1.2;

/// Whether a wallpaper is meant for a portrait (vertical) screen. The scene
/// canvas decides when it is known; otherwise (videos, web) the title has to
/// say so, e.g. "Demon Slayer - Zenitsu [Portrait/Mobile]".
pub fn is_portrait_wallpaper(canvas: Option<(u32, u32)>, title: &str) -> bool {
    match canvas {
        Some((width, height)) if width > 0 && height > 0 => {
            f64::from(height) / f64::from(width) >= PORTRAIT_MIN_RATIO
        }
        _ => title_says_portrait(title),
    }
}

fn title_says_portrait(title: &str) -> bool {
    let title = title.to_lowercase();
    ["portrait", "vertical", "竖屏", "竖版"].iter().any(|word| title.contains(word))
}

/// Only a portrait wallpaper on a landscape display is worth a warning: it
/// loses most of its picture. Landscape wallpapers on a portrait monitor and
/// small ratio differences (16:10 vs 16:9) are normal and stay silent.
pub fn should_warn_orientation(portrait_wallpaper: bool, display: (u32, u32)) -> bool {
    portrait_wallpaper && display.0 > display.1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_portrait_wallpapers_on_landscape_displays_warn() {
        let landscape = (2560, 1440);
        let portrait = (1080, 1920);
        let tall = is_portrait_wallpaper(Some((1080, 1920)), "Final resting place");
        assert!(should_warn_orientation(tall, landscape));
        assert!(!should_warn_orientation(tall, portrait));
        // Landscape and ultra-wide wallpapers never warn, on either display.
        for canvas in [(3840, 2160), (1560, 960), (5120, 1440), (2560, 1600)] {
            let wide = is_portrait_wallpaper(Some(canvas), "Lost Valley");
            assert!(!should_warn_orientation(wide, landscape));
            assert!(!should_warn_orientation(wide, portrait));
        }
        // Nearly square is not portrait.
        assert!(!is_portrait_wallpaper(Some((1000, 1100)), ""));
    }

    #[test]
    fn titles_mark_portrait_only_when_the_canvas_is_unknown() {
        assert!(is_portrait_wallpaper(None, "Demon Slayer - Zenitsu Agatsuma [Portrait/Mobile]"));
        assert!(is_portrait_wallpaper(None, "Rainy street | Vertical"));
        assert!(is_portrait_wallpaper(None, "雨夜 竖屏"));
        assert!(!is_portrait_wallpaper(None, "YoRHa Terminal - NieR:Automata UI"));
        // A landscape canvas beats a title that merely mentions a portrait.
        assert!(!is_portrait_wallpaper(Some((3840, 2160)), "Portrait of a Lady"));
    }

    #[test]
    fn reads_scene_json_out_of_a_pkg() {
        let dir = std::env::temp_dir().join(format!("we-gui-aspect-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let scene = br#"{"general":{"orthogonalprojection":{"width":1080,"height":1920}}}"#;
        let other = b"xx";
        let mut pkg = Vec::new();
        let push_str = |pkg: &mut Vec<u8>, s: &[u8]| {
            pkg.extend_from_slice(&(s.len() as u32).to_le_bytes());
            pkg.extend_from_slice(s);
        };
        push_str(&mut pkg, b"PKGV0024");
        pkg.extend_from_slice(&2u32.to_le_bytes());
        push_str(&mut pkg, b"other.bin");
        pkg.extend_from_slice(&0u32.to_le_bytes());
        pkg.extend_from_slice(&(other.len() as u32).to_le_bytes());
        push_str(&mut pkg, b"scene.json");
        pkg.extend_from_slice(&(other.len() as u32).to_le_bytes());
        pkg.extend_from_slice(&(scene.len() as u32).to_le_bytes());
        pkg.extend_from_slice(other);
        pkg.extend_from_slice(scene);
        std::fs::write(dir.join("scene.pkg"), &pkg).unwrap();

        let data = read_pkg_entry(&dir.join("scene.pkg"), "scene.json").unwrap();
        assert_eq!(canvas_size_from_scene_json(&data), Some((1080, 1920)));
        assert_eq!(read_pkg_entry(&dir.join("scene.pkg"), "missing.json"), None);
        let _ = std::fs::remove_dir_all(dir);
    }
}
