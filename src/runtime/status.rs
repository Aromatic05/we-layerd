use std::sync::Arc;

use crate::config::ScaleMode;
use we_renderer::{DiagnosticSeverity, RendererDiagnostics};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresentBackend {
    Dmabuf,
    Shm,
}

impl PresentBackend {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Dmabuf => "dmabuf",
            Self::Shm => "shm",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct OptionsJsonDiagnostics {
    pub(crate) present: bool,
    pub(crate) len: usize,
    pub(crate) valid: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeDiagnostics {
    pub(crate) wayland_connected: bool,
    pub(crate) dmabuf_global_available: bool,
    pub(crate) dmabuf_global_version: u32,
    pub(crate) dmabuf_formats_known: bool,
    pub(crate) dmabuf_format_count: usize,
    pub(crate) shm_available: bool,
    pub(crate) viewporter_available: bool,
    pub(crate) fractional_scale_available: bool,
    pub(crate) prefer_dmabuf_configured: bool,
    pub(crate) prefer_dmabuf_effective: bool,
    pub(crate) allow_shm_fallback: bool,
    pub(crate) nvidia_prime_offload_detected: bool,
    pub(crate) options_json: OptionsJsonDiagnostics,
    pub(crate) media_integration_supported: bool,
    pub(crate) audio_integration_supported: bool,
    pub(crate) rule_muted: bool,
    pub(crate) renderer_diagnostics: Option<Arc<RendererDiagnostics>>,
    pub(crate) renderer_diagnostics_error: Option<Arc<str>>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FrameStats {
    pub(crate) acquired: u64,
    pub(crate) presented: u64,
    pub(crate) skipped_by_backpressure: u64,
    pub(crate) no_frame_polls: u64,
    pub(crate) released_buffers: u64,
    pub(crate) wayland_buffers_created: u64,
    pub(crate) wayland_buffers_reused: u64,
    pub(crate) dmabuf_fdless_acquires: u64,
    pub(crate) in_flight_count: usize,
    pub(crate) last_present_backend: Option<PresentBackend>,
    pub(crate) last_frame_width: u32,
    pub(crate) last_frame_height: u32,
    pub(crate) last_error: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PresentationStatus {
    pub(crate) configured: bool,
    pub(crate) logical_width: u32,
    pub(crate) logical_height: u32,
    pub(crate) render_width: u32,
    pub(crate) render_height: u32,
    pub(crate) output_mode_width: u32,
    pub(crate) output_mode_height: u32,
    pub(crate) output_scale: u32,
    pub(crate) fractional_scale: f64,
    pub(crate) scale_mode: ScaleMode,
    pub(crate) paused: bool,
    pub(crate) viewport_width: u32,
    pub(crate) viewport_height: u32,
    pub(crate) viewport_source: Option<(f64, f64, f64, f64)>,
}

impl Default for PresentationStatus {
    fn default() -> Self {
        Self {
            configured: false,
            logical_width: 0,
            logical_height: 0,
            render_width: 0,
            render_height: 0,
            output_mode_width: 0,
            output_mode_height: 0,
            output_scale: 1,
            fractional_scale: 1.0,
            scale_mode: ScaleMode::Cover,
            paused: false,
            viewport_width: 0,
            viewport_height: 0,
            viewport_source: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeStatusSnapshot {
    pub(crate) output_name: String,
    pub(crate) output_source: String,
    pub(crate) output_playlist_active: Option<String>,
    pub(crate) output_playlist_index: Option<usize>,
    pub(crate) remove_output: bool,
    pub(crate) runtime: RuntimeDiagnostics,
    pub(crate) presentation: PresentationStatus,
    pub(crate) frame_stats: FrameStats,
}

impl RuntimeStatusSnapshot {
    pub(crate) fn removed_output(output_name: String) -> Self {
        Self { output_name, remove_output: true, ..Self::default() }
    }

    pub(crate) fn render_toml(&self) -> String {
        self.render_toml_with_headers("[runtime]".to_string(), "[presentation]".to_string())
    }

    pub(crate) fn render_output_toml(&self, output_name: &str) -> String {
        let output_key = toml::Value::String(output_name.to_string()).to_string();
        self.render_toml_with_headers(
            format!("[output_runtime.{output_key}.runtime]"),
            format!("[output_runtime.{output_key}.presentation]"),
        )
    }

    fn render_toml_with_headers(
        &self,
        runtime_header: String,
        presentation_header: String,
    ) -> String {
        let mut lines = vec![
            runtime_header,
            format!("output_name = {}", toml::Value::String(self.output_name.clone())),
            format!("source = {}", toml::Value::String(self.output_source.clone())),
            format!(
                "playlist_active = {}",
                toml::Value::String(self.output_playlist_active.clone().unwrap_or_default())
            ),
            format!(
                "playlist_index = {}",
                self.output_playlist_index.map(|index| index as i64).unwrap_or(-1)
            ),
            format!("wayland_connected = {}", self.runtime.wayland_connected),
            format!("dmabuf_global_available = {}", self.runtime.dmabuf_global_available),
            format!("dmabuf_global_version = {}", self.runtime.dmabuf_global_version),
            format!("dmabuf_formats_known = {}", self.runtime.dmabuf_formats_known),
            format!("dmabuf_format_count = {}", self.runtime.dmabuf_format_count),
            format!("shm_available = {}", self.runtime.shm_available),
            format!("viewporter_available = {}", self.runtime.viewporter_available),
            format!("fractional_scale_available = {}", self.runtime.fractional_scale_available),
            format!("prefer_dmabuf_configured = {}", self.runtime.prefer_dmabuf_configured),
            format!("prefer_dmabuf_effective = {}", self.runtime.prefer_dmabuf_effective),
            format!("allow_shm_fallback = {}", self.runtime.allow_shm_fallback),
            format!(
                "nvidia_prime_offload_detected = {}",
                self.runtime.nvidia_prime_offload_detected
            ),
            format!("options_json_present = {}", self.runtime.options_json.present),
            format!("options_json_len = {}", self.runtime.options_json.len),
            format!("options_json_valid = {}", self.runtime.options_json.valid),
            format!("media_integration_supported = {}", self.runtime.media_integration_supported),
            format!("audio_integration_supported = {}", self.runtime.audio_integration_supported),
            format!("rule_muted = {}", self.runtime.rule_muted),
            format!(
                "renderer_diagnostic_count = {}",
                self.runtime
                    .renderer_diagnostics
                    .as_ref()
                    .map(|diagnostics| diagnostics.entries.len())
                    .unwrap_or(0)
            ),
            format!(
                "renderer_warning_count = {}",
                renderer_diagnostic_count(&self.runtime, DiagnosticSeverity::Warning)
            ),
            format!(
                "renderer_error_count = {}",
                renderer_diagnostic_count(&self.runtime, DiagnosticSeverity::Error)
            ),
            format!(
                "renderer_diagnostics_json = {}",
                toml::Value::String(
                    self.runtime
                        .renderer_diagnostics
                        .as_ref()
                        .and_then(|diagnostics| serde_json::to_string(diagnostics.as_ref()).ok())
                        .unwrap_or_default()
                )
            ),
            format!(
                "renderer_diagnostics_error = {}",
                toml::Value::String(
                    self.runtime.renderer_diagnostics_error.as_deref().unwrap_or("").to_string()
                )
            ),
            String::new(),
            presentation_header,
            format!("configured = {}", self.presentation.configured),
            format!("logical_width = {}", self.presentation.logical_width),
            format!("logical_height = {}", self.presentation.logical_height),
            format!("render_width = {}", self.presentation.render_width),
            format!("render_height = {}", self.presentation.render_height),
            format!("output_mode_width = {}", self.presentation.output_mode_width),
            format!("output_mode_height = {}", self.presentation.output_mode_height),
            format!("output_scale = {}", self.presentation.output_scale),
            format!("fractional_scale = {:.3}", self.presentation.fractional_scale),
            format!("scale_mode = \"{}\"", scale_mode_name(self.presentation.scale_mode)),
            format!("paused = {}", self.presentation.paused),
            format!("viewport_width = {}", self.presentation.viewport_width),
            format!("viewport_height = {}", self.presentation.viewport_height),
        ];

        if let Some((x, y, width, height)) = self.presentation.viewport_source {
            lines.push(format!("viewport_source_x = {:.3}", x));
            lines.push(format!("viewport_source_y = {:.3}", y));
            lines.push(format!("viewport_source_width = {:.3}", width));
            lines.push(format!("viewport_source_height = {:.3}", height));
        }

        lines.extend([
            format!(
                "last_present_backend = \"{}\"",
                self.frame_stats.last_present_backend.map(PresentBackend::as_str).unwrap_or("")
            ),
            format!("last_frame_width = {}", self.frame_stats.last_frame_width),
            format!("last_frame_height = {}", self.frame_stats.last_frame_height),
            format!("in_flight_buffers = {}", self.frame_stats.in_flight_count),
            format!("acquired = {}", self.frame_stats.acquired),
            format!("presented = {}", self.frame_stats.presented),
            format!("skipped_by_backpressure = {}", self.frame_stats.skipped_by_backpressure),
            format!("no_frame_polls = {}", self.frame_stats.no_frame_polls),
            format!("released_buffers = {}", self.frame_stats.released_buffers),
            format!("wayland_buffers_created = {}", self.frame_stats.wayland_buffers_created),
            format!("wayland_buffers_reused = {}", self.frame_stats.wayland_buffers_reused),
            format!("dmabuf_fdless_acquires = {}", self.frame_stats.dmabuf_fdless_acquires),
            format!("last_error = {:?}", self.frame_stats.last_error.as_deref().unwrap_or("")),
        ]);

        lines.join("\n")
    }
}

fn renderer_diagnostic_count(runtime: &RuntimeDiagnostics, severity: DiagnosticSeverity) -> usize {
    runtime
        .renderer_diagnostics
        .as_ref()
        .map(|diagnostics| {
            diagnostics.entries.iter().filter(|entry| entry.severity == severity).count()
        })
        .unwrap_or(0)
}

fn scale_mode_name(scale_mode: ScaleMode) -> &'static str {
    match scale_mode {
        ScaleMode::Fit => "fit",
        ScaleMode::Cover => "cover",
        ScaleMode::Stretch => "stretch",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use we_renderer::{DiagnosticEntry, DiagnosticSeverity, RendererDiagnostics};

    use super::{RuntimeDiagnostics, RuntimeStatusSnapshot};

    #[test]
    fn renderer_diagnostics_are_valid_toml_and_preserve_json_payload() {
        let snapshot = RuntimeStatusSnapshot {
            runtime: RuntimeDiagnostics {
                renderer_diagnostics: Some(Arc::new(RendererDiagnostics {
                    version: 1,
                    entries: vec![DiagnosticEntry {
                        severity: DiagnosticSeverity::Warning,
                        source: "abi.render-config.msaa".to_string(),
                        message: "scene only".to_string(),
                    }],
                })),
                ..RuntimeDiagnostics::default()
            },
            ..RuntimeStatusSnapshot::default()
        };

        let rendered = snapshot.render_toml();
        let document = toml::from_str::<toml::Value>(&rendered).expect("valid status TOML");
        let json = document["runtime"]["renderer_diagnostics_json"]
            .as_str()
            .expect("diagnostics JSON string");
        let payload =
            serde_json::from_str::<serde_json::Value>(json).expect("valid diagnostics JSON");
        assert_eq!(payload["entries"][0]["source"], "abi.render-config.msaa");
        assert_eq!(document["runtime"]["renderer_warning_count"].as_integer(), Some(1));
    }

    #[test]
    fn output_scoped_status_round_trips_named_output_source_and_playlist_cursor() {
        let snapshot = RuntimeStatusSnapshot {
            output_name: "DP-1".to_string(),
            output_source: "/wallpapers/42".to_string(),
            output_playlist_active: Some("Focus".to_string()),
            output_playlist_index: Some(3),
            ..RuntimeStatusSnapshot::default()
        };

        let rendered = snapshot.render_output_toml("DP-1");
        let document = toml::from_str::<toml::Value>(&rendered).expect("valid output status TOML");
        let runtime = &document["output_runtime"]["DP-1"]["runtime"];
        assert_eq!(runtime["output_name"].as_str(), Some("DP-1"));
        assert_eq!(runtime["source"].as_str(), Some("/wallpapers/42"));
        assert_eq!(runtime["playlist_active"].as_str(), Some("Focus"));
        assert_eq!(runtime["playlist_index"].as_integer(), Some(3));
    }
}
