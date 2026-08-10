use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::WallpaperType;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WallpaperSettings {
    #[serde(default = "default_fps")]
    pub fps: u32,
    #[serde(default = "default_speed")]
    pub speed: f32,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub muted: bool,
    #[serde(default = "default_msaa_samples")]
    pub msaa_samples: u32,
    #[serde(default)]
    pub render_resolution: RenderResolution,
    #[serde(default)]
    pub fill_mode: WallpaperFillMode,
    #[serde(default)]
    pub rotation_degrees: Rotation,
    #[serde(default)]
    pub user_properties: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RenderResolution {
    #[default]
    Automatic,
    Fixed {
        width: u32,
        height: u32,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum WallpaperFillMode {
    #[default]
    Cover,
    Fit,
    Stretch,
    Center,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Rotation {
    #[default]
    Deg0,
    Deg90,
    Deg180,
    Deg270,
}

impl Rotation {
    pub fn degrees(self) -> u32 {
        match self {
            Self::Deg0 => 0,
            Self::Deg90 => 90,
            Self::Deg180 => 180,
            Self::Deg270 => 270,
        }
    }
}

impl std::fmt::Display for WallpaperFillMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Cover => "Cover",
            Self::Fit => "Fit",
            Self::Stretch => "Stretch",
            Self::Center => "Center",
        })
    }
}

impl std::fmt::Display for Rotation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Deg0 => "0°",
            Self::Deg90 => "90°",
            Self::Deg180 => "180°",
            Self::Deg270 => "270°",
        })
    }
}

impl Default for WallpaperSettings {
    fn default() -> Self {
        Self {
            fps: default_fps(),
            speed: default_speed(),
            volume: default_volume(),
            muted: false,
            msaa_samples: default_msaa_samples(),
            render_resolution: RenderResolution::Automatic,
            fill_mode: WallpaperFillMode::Cover,
            rotation_degrees: Rotation::Deg0,
            user_properties: BTreeMap::new(),
        }
    }
}

fn default_fps() -> u32 {
    60
}
fn default_speed() -> f32 {
    1.0
}
fn default_volume() -> f32 {
    1.0
}
fn default_msaa_samples() -> u32 {
    1
}

pub fn supports_final_output_msaa(wallpaper_type: WallpaperType) -> bool {
    wallpaper_type == WallpaperType::Scene
}

pub fn inherited_final_output_msaa(global_samples: u32, wallpaper_type: WallpaperType) -> u32 {
    if supports_final_output_msaa(wallpaper_type) {
        global_samples.max(1)
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::{
        inherited_final_output_msaa, supports_final_output_msaa, RenderResolution,
        WallpaperFillMode, WallpaperSettings,
    };
    use crate::wallpaper::WallpaperType;

    #[test]
    fn settings_default_to_dynamic_neutral_rendering() {
        let settings = WallpaperSettings::default();
        assert_eq!(settings.render_resolution, RenderResolution::Automatic);
        assert_eq!(settings.fill_mode, WallpaperFillMode::Cover);
        assert_eq!(settings.rotation_degrees.degrees(), 0);
        assert_eq!(settings.fps, 60);
        assert_eq!(settings.msaa_samples, 1);
    }

    #[test]
    fn final_output_msaa_is_scene_only() {
        assert!(supports_final_output_msaa(WallpaperType::Scene));
        assert!(!supports_final_output_msaa(WallpaperType::Video));
        assert!(!supports_final_output_msaa(WallpaperType::Web));
        assert!(!supports_final_output_msaa(WallpaperType::Unknown));
        assert_eq!(inherited_final_output_msaa(8, WallpaperType::Scene), 8);
        assert_eq!(inherited_final_output_msaa(8, WallpaperType::Video), 1);
        assert_eq!(inherited_final_output_msaa(8, WallpaperType::Web), 1);
    }
}
