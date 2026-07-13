use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::Deserialize;

pub mod properties;
pub mod settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallpaperType {
    Video,
    Scene,
    Web,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct WallpaperEntry {
    pub id: String,
    pub project_json: PathBuf,
    pub title: String,
    pub ty: WallpaperType,
    pub preview: Option<PathBuf>,
    pub source_file: Option<PathBuf>,
}

#[derive(Debug, Deserialize)]
struct ProjectJson {
    #[serde(default)]
    title: String,
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    file: String,
}

pub fn scan_workshop_wallpapers(workshop_app_root: &Path) -> Result<Vec<WallpaperEntry>> {
    let mut result = Vec::new();

    for dir in fs::read_dir(workshop_app_root)
        .with_context(|| format!("failed to read {}", workshop_app_root.display()))?
    {
        let dir = dir?;
        let path = dir.path();
        if !path.is_dir() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let id = name.to_string();
        let project_json = path.join("project.json");
        if !project_json.is_file() {
            continue;
        }

        let meta = parse_project_json(&project_json)?;
        let ty = parse_type(&meta.r#type);
        let source_file =
            if meta.file.trim().is_empty() { None } else { Some(path.join(meta.file)) };
        let preview = detect_preview_image(&path);
        result.push(WallpaperEntry {
            id,
            project_json,
            title: if meta.title.trim().is_empty() { "Untitled".to_string() } else { meta.title },
            ty,
            preview,
            source_file,
        });
    }

    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(result)
}

fn parse_project_json(path: &Path) -> Result<ProjectJson> {
    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let parsed = serde_json::from_str::<ProjectJson>(&raw)
        .with_context(|| format!("invalid JSON: {}", path.display()))?;
    Ok(parsed)
}

fn parse_type(value: &str) -> WallpaperType {
    match value.to_ascii_lowercase().as_str() {
        "video" => WallpaperType::Video,
        "scene" => WallpaperType::Scene,
        "web" => WallpaperType::Web,
        _ => WallpaperType::Unknown,
    }
}

fn detect_preview_image(wallpaper_dir: &Path) -> Option<PathBuf> {
    let candidates = [
        "preview.jpg",
        "preview.jpeg",
        "preview.png",
        "preview.gif",
        "thumbnail.jpg",
        "thumbnail.png",
    ];

    for name in candidates {
        let path = wallpaper_dir.join(name);
        if path.is_file() {
            return Some(path);
        }
    }

    None
}
