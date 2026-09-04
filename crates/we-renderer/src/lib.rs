use std::{
    ffi::CString,
    os::fd::{FromRawFd, OwnedFd, RawFd},
    path::Path,
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use we_renderer_sys::{
    self as sys, we_fill_mode_v1, we_frame_kind_v1, we_input_event_type_v2, we_media_state_v1,
    we_render_config_v1, we_runtime_settings_v1, we_session_t, we_source_v1,
    RendererLibrary as SysRendererLibrary,
};

#[derive(Debug, Clone)]
pub struct Source {
    pub uri: String,
    pub assets_uri: String,
    pub fps: i32,
    pub speed: f32,
    pub volume: f32,
    pub muted: bool,
    pub options_json: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct RenderConfig {
    pub width: u32,
    pub height: u32,
    pub enable_valid_layer: bool,
    pub prefer_dmabuf: bool,
    pub allow_shm_fallback: bool,
    pub msaa_samples: u32,
    pub fill_mode: FillMode,
    pub rotation_degrees: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillMode {
    Cover,
    Stretch,
    Fit,
    Center,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RuntimeSettings {
    pub fps: Option<i32>,
    pub speed: Option<f32>,
    pub volume: Option<f32>,
    pub muted: Option<bool>,
    pub fill_mode: Option<FillMode>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MediaPlaybackState {
    #[default]
    Stopped,
    Playing,
    Paused,
    Other,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MediaImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MediaState {
    pub playback: MediaPlaybackState,
    pub primary_color: [f32; 3],
    pub secondary_color: [f32; 3],
    pub tertiary_color: [f32; 3],
    pub text_color: [f32; 3],
    pub high_contrast_color: [f32; 3],
    pub title: String,
    pub artist: String,
    pub album_title: String,
    pub album_artist: String,
    pub sub_title: String,
    pub genres: String,
    pub content_type: String,
    pub thumbnail: Option<MediaImage>,
    pub previous_thumbnail: Option<MediaImage>,
}

impl Default for MediaState {
    fn default() -> Self {
        Self {
            playback: MediaPlaybackState::Stopped,
            primary_color: [0.0, 0.0, 0.0],
            secondary_color: [1.0, 1.0, 1.0],
            tertiary_color: [1.0, 1.0, 1.0],
            text_color: [1.0, 1.0, 1.0],
            high_contrast_color: [1.0, 1.0, 1.0],
            title: String::new(),
            artist: String::new(),
            album_title: String::new(),
            album_artist: String::new(),
            sub_title: String::new(),
            genres: String::new(),
            content_type: String::new(),
            thumbnail: None,
            previous_thumbnail: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RendererDiagnostics {
    pub version: u32,
    pub entries: Vec<DiagnosticEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticEntry {
    pub severity: DiagnosticSeverity,
    pub source: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputEvent {
    PointerMove { x: f32, y: f32 },
    PointerDown { x: f32, y: f32, button: i32 },
    PointerUp { x: f32, y: f32, button: i32 },
    PointerWheel { x: f32, y: f32, delta_x: i32, delta_y: i32 },
    KeyDown { key_code: i32, native_key_code: i32, modifiers: i32, unicode_char: u32 },
    KeyUp { key_code: i32, native_key_code: i32, modifiers: i32, unicode_char: u32 },
    Focus { focused: bool },
}

#[derive(Debug)]
pub struct RendererLibrary {
    inner: Arc<SysRendererLibrary>,
}

#[derive(Debug)]
pub struct Session {
    library: Arc<SysRendererLibrary>,
    raw: *mut we_session_t,
    _cache_path: Option<CString>,
    source_state: Option<OwnedSourceState>,
}

#[derive(Debug)]
pub struct DmabufPlane {
    pub fd: OwnedFd,
    pub offset: u32,
    pub stride: u32,
}

#[derive(Debug)]
pub struct DmabufFrame {
    pub width: u32,
    pub height: u32,
    pub drm_fourcc: u32,
    pub drm_modifier: u64,
    pub flags: u32,
    pub serial: u64,
    pub pts_ns: u64,
    pub buffer_id: Option<u32>,
    pub fds_omitted: bool,
    pub planes: Vec<DmabufPlane>,
}

#[derive(Debug)]
pub struct ShmFrame {
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub size: u32,
    pub serial: u64,
    pub pts_ns: u64,
    pub fd: OwnedFd,
}

#[derive(Debug)]
pub enum Frame {
    Dmabuf(DmabufFrame),
    Shm(ShmFrame),
}

#[derive(Debug)]
struct OwnedSourceState {
    uri: CString,
    assets_uri: CString,
    options_json: Option<CString>,
}

#[derive(Debug)]
struct EncodedMediaState {
    raw: we_media_state_v1,
    _title: CString,
    _artist: CString,
    _album_title: CString,
    _album_artist: CString,
    _sub_title: CString,
    _genres: CString,
    _content_type: CString,
    _thumbnail: Option<Vec<u8>>,
    _previous_thumbnail: Option<Vec<u8>>,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Load(#[from] sys::LoadError),
    #[error("renderer returned a null session pointer")]
    NullSession,
    #[error("renderer source contains an interior NUL byte")]
    InvalidSourceString,
    #[error("renderer returned status {0} for {1}")]
    Status(i32, &'static str),
    #[error("renderer returned an unsupported frame kind {0}")]
    UnsupportedFrameKind(u32),
    #[error("renderer reported {0} planes, which exceeds the ABI limit")]
    InvalidPlaneCount(u32),
    #[error("DMA-BUF format list contains too many entries: {0}")]
    TooManyDmabufFormats(usize),
    #[error("failed to duplicate frame fd")]
    DuplicateFd(#[source] std::io::Error),
    #[error("renderer diagnostics payload is too large: {0} bytes")]
    DiagnosticsTooLarge(u32),
    #[error("loaded renderer library does not expose diagnostics")]
    DiagnosticsUnavailable,
    #[error("renderer diagnostics payload did not contain the required trailing NUL")]
    DiagnosticsMissingNul,
    #[error("renderer diagnostics changed size repeatedly while being read")]
    DiagnosticsUnstable,
    #[error("invalid renderer diagnostics JSON: {0}")]
    DiagnosticsJson(#[from] serde_json::Error),
    #[error("unsupported renderer diagnostics version {0}")]
    UnsupportedDiagnosticsVersion(u32),
    #[error("renderer media metadata contains an interior NUL byte")]
    InvalidMediaString,
    #[error("renderer media image dimensions require {expected} RGBA bytes but received {actual}")]
    InvalidMediaImageLength { expected: usize, actual: usize },
    #[error("loaded renderer library does not expose media-state integration")]
    MediaUnavailable,
    #[error("loaded renderer library does not expose audio-spectrum integration")]
    AudioSamplesUnavailable,
    #[error("audio-spectrum payload must contain 1..=4096 finite samples")]
    InvalidAudioSamples,
}

impl RendererLibrary {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Error> {
        let inner = SysRendererLibrary::load(path.as_ref().as_os_str())?;
        Ok(Self { inner: Arc::new(inner) })
    }

    pub fn supports_media_state(&self) -> bool {
        self.inner.supports_media_state()
    }

    pub fn supports_audio_samples(&self) -> bool {
        self.inner.supports_audio_samples()
    }

    pub fn create_session(&self, cache_path: Option<&Path>) -> Result<Session, Error> {
        let cache_path = match cache_path {
            Some(path) => Some(
                CString::new(path.to_string_lossy().as_bytes())
                    .map_err(|_| Error::InvalidSourceString)?,
            ),
            None => None,
        };

        let raw = match cache_path.as_ref() {
            Some(path) => unsafe { self.inner.session_create_with_cache_path(path.as_ptr()) },
            None => unsafe { self.inner.session_create() },
        };

        if raw.is_null() {
            return Err(Error::NullSession);
        }

        Ok(Session {
            library: Arc::clone(&self.inner),
            raw,
            _cache_path: cache_path,
            source_state: None,
        })
    }
}

impl Session {
    pub fn set_source(&mut self, source: &Source) -> Result<(), Error> {
        let source_state = OwnedSourceState {
            uri: CString::new(source.uri.as_bytes()).map_err(|_| Error::InvalidSourceString)?,
            assets_uri: CString::new(source.assets_uri.as_bytes())
                .map_err(|_| Error::InvalidSourceString)?,
            options_json: match source.options_json.as_ref() {
                Some(value) => {
                    Some(CString::new(value.as_bytes()).map_err(|_| Error::InvalidSourceString)?)
                }
                None => None,
            },
        };

        let raw = we_source_v1 {
            size: std::mem::size_of::<we_source_v1>() as u32,
            version: sys::WE_SOURCE_V1_VERSION,
            uri: source_state.uri.as_ptr(),
            assets_uri: source_state.assets_uri.as_ptr(),
            fps: source.fps,
            speed: source.speed,
            volume: source.volume,
            muted: source.muted,
            options_json: source_state
                .options_json
                .as_ref()
                .map_or(std::ptr::null(), |value| value.as_ptr()),
        };

        self.check_status(
            unsafe { self.library.session_set_source(self.raw, &raw) },
            "we_session_set_source",
        )?;
        self.source_state = Some(source_state);
        Ok(())
    }

    pub fn frame_ready_fd(&self) -> Result<RawFd, Error> {
        let fd = unsafe { self.library.session_get_frame_ready_fd(self.raw) };
        if fd < 0 {
            return Err(Error::Status(fd, "we_session_get_frame_ready_fd"));
        }
        Ok(fd)
    }

    pub fn configure(&mut self, config: RenderConfig) -> Result<(), Error> {
        let raw = we_render_config_v1 {
            size: std::mem::size_of::<we_render_config_v1>() as u32,
            version: sys::WE_RENDER_CONFIG_V1_VERSION,
            width: config.width,
            height: config.height,
            enable_valid_layer: config.enable_valid_layer,
            prefer_dmabuf: config.prefer_dmabuf,
            allow_shm_fallback: config.allow_shm_fallback,
            msaa_samples: config.msaa_samples,
            fill_mode: fill_mode_to_sys(config.fill_mode),
            rotation_degrees: config.rotation_degrees,
        };

        self.check_status(
            unsafe { self.library.session_set_render_config(self.raw, &raw) },
            "we_session_set_render_config",
        )
    }

    pub fn set_dmabuf_formats(&mut self, formats: &[(u32, u64)]) -> Result<(), Error> {
        let count =
            u32::try_from(formats.len()).map_err(|_| Error::TooManyDmabufFormats(formats.len()))?;
        let fourccs: Vec<u32> = formats.iter().map(|(fourcc, _)| *fourcc).collect();
        let modifiers: Vec<u64> = formats.iter().map(|(_, modifier)| *modifier).collect();
        let fourccs_ptr = if fourccs.is_empty() { std::ptr::null() } else { fourccs.as_ptr() };
        let modifiers_ptr =
            if modifiers.is_empty() { std::ptr::null() } else { modifiers.as_ptr() };
        self.check_status(
            unsafe {
                self.library.session_set_dmabuf_formats(self.raw, fourccs_ptr, modifiers_ptr, count)
            },
            "we_session_set_dmabuf_formats",
        )
    }

    pub fn resize_output(&mut self, width: u32, height: u32) -> Result<(), Error> {
        self.check_status(
            unsafe { self.library.session_resize_output(self.raw, width, height) },
            "we_session_resize_output",
        )
    }

    pub fn set_user_properties_json(&mut self, properties_json: &str) -> Result<(), Error> {
        let properties_json =
            CString::new(properties_json).map_err(|_| Error::InvalidSourceString)?;
        self.check_status(
            unsafe {
                self.library.session_set_user_properties_json(self.raw, properties_json.as_ptr())
            },
            "we_session_set_user_properties_json",
        )
    }

    pub fn apply_runtime_settings(&mut self, settings: RuntimeSettings) -> Result<(), Error> {
        let mut fields = 0;
        if settings.fps.is_some() {
            fields |= sys::WE_RUNTIME_SETTINGS_FPS;
        }
        if settings.speed.is_some() {
            fields |= sys::WE_RUNTIME_SETTINGS_SPEED;
        }
        if settings.volume.is_some() {
            fields |= sys::WE_RUNTIME_SETTINGS_VOLUME;
        }
        if settings.muted.is_some() {
            fields |= sys::WE_RUNTIME_SETTINGS_MUTED;
        }
        if settings.fill_mode.is_some() {
            fields |= sys::WE_RUNTIME_SETTINGS_FILL_MODE;
        }
        if fields == 0 {
            return Ok(());
        }

        let raw = we_runtime_settings_v1 {
            size: std::mem::size_of::<we_runtime_settings_v1>() as u32,
            version: sys::WE_RUNTIME_SETTINGS_V1_VERSION,
            fields,
            fps: settings.fps.unwrap_or_default(),
            speed: settings.speed.unwrap_or_default(),
            volume: settings.volume.unwrap_or_default(),
            muted: settings.muted.unwrap_or_default(),
            fill_mode: fill_mode_to_sys(settings.fill_mode.unwrap_or(FillMode::Cover)),
        };
        self.check_status(
            unsafe { self.library.session_apply_runtime_settings(self.raw, &raw) },
            "we_session_apply_runtime_settings",
        )
    }

    pub fn set_media_state(&mut self, state: &MediaState) -> Result<(), Error> {
        let encoded = encode_media_state(state)?;
        let Some(status) =
            (unsafe { self.library.session_set_media_state(self.raw, &encoded.raw) })
        else {
            return Err(Error::MediaUnavailable);
        };
        self.check_status(status, "we_session_set_media_state")
    }

    pub fn push_audio_samples(&mut self, samples: &[f32]) -> Result<(), Error> {
        if samples.is_empty()
            || samples.len() > 4096
            || !samples.iter().all(|value| value.is_finite())
        {
            return Err(Error::InvalidAudioSamples);
        }
        let count = u32::try_from(samples.len()).map_err(|_| Error::InvalidAudioSamples)?;
        let Some(status) =
            (unsafe { self.library.session_push_audio_samples(self.raw, samples.as_ptr(), count) })
        else {
            return Err(Error::AudioSamplesUnavailable);
        };
        self.check_status(status, "we_session_push_audio_samples")
    }

    pub fn play(&mut self) -> Result<(), Error> {
        self.check_status(unsafe { self.library.session_play(self.raw) }, "we_session_play")
    }

    pub fn pause(&mut self) -> Result<(), Error> {
        self.check_status(unsafe { self.library.session_pause(self.raw) }, "we_session_pause")
    }

    pub fn stop(&mut self) -> Result<(), Error> {
        self.check_status(unsafe { self.library.session_stop(self.raw) }, "we_session_stop")
    }

    pub fn tick(&mut self) -> Result<(), Error> {
        self.check_status(unsafe { self.library.session_tick(self.raw) }, "we_session_tick")
    }

    pub fn acquire_frame(&mut self) -> Result<Option<Frame>, Error> {
        self.acquire_frame_with_reusable_buffers(0)
    }

    pub fn acquire_frame_with_reusable_buffers(
        &mut self,
        reusable_buffer_mask: u32,
    ) -> Result<Option<Frame>, Error> {
        let mut raw = sys::we_frame_v1 {
            size: std::mem::size_of::<sys::we_frame_v1>() as u32,
            version: sys::WE_FRAME_V1_VERSION,
            kind: we_frame_kind_v1::WE_FRAME_KIND_SHM as u32,
            width: 0,
            height: 0,
            drm_fourcc: 0,
            drm_modifier: 0,
            n_planes: 0,
            flags: 0,
            serial: 0,
            pts_ns: 0,
            shm_stride: 0,
            shm_size: 0,
            planes: [sys::we_dmabuf_plane_v1 { fd: -1, offset: 0, stride: 0 }; 4],
            buffer_id: sys::WE_FRAME_BUFFER_ID_INVALID,
            reusable_buffer_mask,
        };

        let status = unsafe { self.library.session_acquire_frame(self.raw, &mut raw) };
        if status == 1 {
            return Ok(None);
        }
        if status != 0 {
            return Err(Error::Status(status, "we_session_acquire_frame"));
        }

        let owned = match frame_from_raw(&raw) {
            Ok(frame) => frame,
            Err(err) => {
                unsafe {
                    self.library.frame_release(&mut raw);
                }
                return Err(err);
            }
        };
        unsafe {
            self.library.frame_release(&mut raw);
        }
        Ok(Some(owned))
    }

    pub fn send_input_event(&mut self, event: InputEvent) -> Result<(), Error> {
        let raw = to_raw_input_event(event);
        self.check_status(
            unsafe { self.library.session_send_input_event(self.raw, &raw) },
            "we_session_send_input_event",
        )
    }

    pub fn diagnostics(&self) -> Result<RendererDiagnostics, Error> {
        const MAX_DIAGNOSTICS_BYTES: u32 = 16 * 1024 * 1024;

        let mut required = 0_u32;
        let Some(size_status) = (unsafe {
            self.library.session_get_diagnostics_json(self.raw, std::ptr::null_mut(), &mut required)
        }) else {
            return Err(Error::DiagnosticsUnavailable);
        };
        self.check_status(size_status, "we_session_get_diagnostics_json(size)")?;

        for _ in 0..3 {
            if required == 0 || required > MAX_DIAGNOSTICS_BYTES {
                return Err(Error::DiagnosticsTooLarge(required));
            }
            let mut bytes = vec![0_u8; required as usize];
            let mut actual = required;
            let Some(status) = (unsafe {
                self.library.session_get_diagnostics_json(
                    self.raw,
                    bytes.as_mut_ptr().cast(),
                    &mut actual,
                )
            }) else {
                return Err(Error::DiagnosticsUnavailable);
            };
            if status == -2 {
                required = actual;
                continue;
            }
            self.check_status(status, "we_session_get_diagnostics_json")?;
            if actual == 0 || actual > required || bytes.get(actual as usize - 1) != Some(&0) {
                return Err(Error::DiagnosticsMissingNul);
            }
            return parse_diagnostics_json(&bytes[..actual as usize - 1]);
        }

        Err(Error::DiagnosticsUnstable)
    }

    fn check_status(&self, status: i32, op: &'static str) -> Result<(), Error> {
        if status == 0 {
            Ok(())
        } else {
            Err(Error::Status(status, op))
        }
    }
}

fn encode_media_state(state: &MediaState) -> Result<EncodedMediaState, Error> {
    let title = CString::new(state.title.as_bytes()).map_err(|_| Error::InvalidMediaString)?;
    let artist = CString::new(state.artist.as_bytes()).map_err(|_| Error::InvalidMediaString)?;
    let album_title =
        CString::new(state.album_title.as_bytes()).map_err(|_| Error::InvalidMediaString)?;
    let album_artist =
        CString::new(state.album_artist.as_bytes()).map_err(|_| Error::InvalidMediaString)?;
    let sub_title =
        CString::new(state.sub_title.as_bytes()).map_err(|_| Error::InvalidMediaString)?;
    let genres = CString::new(state.genres.as_bytes()).map_err(|_| Error::InvalidMediaString)?;
    let content_type =
        CString::new(state.content_type.as_bytes()).map_err(|_| Error::InvalidMediaString)?;

    let thumbnail = validate_media_image(state.thumbnail.as_ref())?;
    let previous_thumbnail = validate_media_image(state.previous_thumbnail.as_ref())?;
    let (thumbnail_width, thumbnail_height, thumbnail_rgba) = state
        .thumbnail
        .as_ref()
        .zip(thumbnail.as_ref())
        .map(|(image, bytes)| (image.width, image.height, bytes.as_ptr()))
        .unwrap_or((0, 0, std::ptr::null()));
    let (previous_thumbnail_width, previous_thumbnail_height, previous_thumbnail_rgba) = state
        .previous_thumbnail
        .as_ref()
        .zip(previous_thumbnail.as_ref())
        .map(|(image, bytes)| (image.width, image.height, bytes.as_ptr()))
        .unwrap_or((0, 0, std::ptr::null()));

    let raw = we_media_state_v1 {
        size: std::mem::size_of::<we_media_state_v1>() as u32,
        version: sys::WE_MEDIA_STATE_V1_VERSION,
        playback_state: match state.playback {
            MediaPlaybackState::Stopped => 0,
            MediaPlaybackState::Playing => 1,
            MediaPlaybackState::Paused => 2,
            MediaPlaybackState::Other => 3,
        },
        has_thumbnail: state.thumbnail.is_some(),
        primary_color: state.primary_color,
        secondary_color: state.secondary_color,
        tertiary_color: state.tertiary_color,
        text_color: state.text_color,
        high_contrast_color: state.high_contrast_color,
        title: title.as_ptr(),
        artist: artist.as_ptr(),
        album_title: album_title.as_ptr(),
        album_artist: album_artist.as_ptr(),
        sub_title: sub_title.as_ptr(),
        genres: genres.as_ptr(),
        content_type: content_type.as_ptr(),
        thumbnail_width,
        thumbnail_height,
        thumbnail_rgba,
        previous_thumbnail_width,
        previous_thumbnail_height,
        previous_thumbnail_rgba,
    };
    Ok(EncodedMediaState {
        raw,
        _title: title,
        _artist: artist,
        _album_title: album_title,
        _album_artist: album_artist,
        _sub_title: sub_title,
        _genres: genres,
        _content_type: content_type,
        _thumbnail: thumbnail,
        _previous_thumbnail: previous_thumbnail,
    })
}

fn validate_media_image(image: Option<&MediaImage>) -> Result<Option<Vec<u8>>, Error> {
    let Some(image) = image else {
        return Ok(None);
    };
    let expected = (image.width as usize)
        .checked_mul(image.height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(Error::InvalidMediaImageLength { expected: usize::MAX, actual: image.rgba.len() })?;
    if image.rgba.len() != expected {
        return Err(Error::InvalidMediaImageLength { expected, actual: image.rgba.len() });
    }
    Ok(Some(image.rgba.clone()))
}

fn parse_diagnostics_json(bytes: &[u8]) -> Result<RendererDiagnostics, Error> {
    let diagnostics: RendererDiagnostics = serde_json::from_slice(bytes)?;
    if diagnostics.version != 1 {
        return Err(Error::UnsupportedDiagnosticsVersion(diagnostics.version));
    }
    Ok(diagnostics)
}

fn fill_mode_to_sys(value: FillMode) -> we_fill_mode_v1 {
    match value {
        FillMode::Cover => we_fill_mode_v1::WE_FILL_MODE_ASPECT_CROP,
        FillMode::Stretch => we_fill_mode_v1::WE_FILL_MODE_STRETCH,
        FillMode::Fit => we_fill_mode_v1::WE_FILL_MODE_ASPECT_FIT,
        FillMode::Center => we_fill_mode_v1::WE_FILL_MODE_CENTER,
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            self.library.session_destroy(self.raw);
        }
    }
}

fn to_raw_input_event(event: InputEvent) -> sys::we_input_event_v2 {
    let mut raw = sys::we_input_event_v2 {
        size: std::mem::size_of::<sys::we_input_event_v2>() as u32,
        version: sys::WE_INPUT_EVENT_V2_VERSION,
        type_: we_input_event_type_v2::WE_INPUT_FOCUS as u32,
        pointer_x: 0.0,
        pointer_y: 0.0,
        button: 0,
        wheel_delta_x: 0,
        wheel_delta_y: 0,
        key_code: 0,
        native_key_code: 0,
        modifiers: 0,
        unicode_char: 0,
        focused: false,
    };

    match event {
        InputEvent::PointerMove { x, y } => {
            raw.type_ = we_input_event_type_v2::WE_INPUT_POINTER_MOVE as u32;
            raw.pointer_x = x;
            raw.pointer_y = y;
        }
        InputEvent::PointerDown { x, y, button } => {
            raw.type_ = we_input_event_type_v2::WE_INPUT_POINTER_DOWN as u32;
            raw.pointer_x = x;
            raw.pointer_y = y;
            raw.button = button;
        }
        InputEvent::PointerUp { x, y, button } => {
            raw.type_ = we_input_event_type_v2::WE_INPUT_POINTER_UP as u32;
            raw.pointer_x = x;
            raw.pointer_y = y;
            raw.button = button;
        }
        InputEvent::PointerWheel { x, y, delta_x, delta_y } => {
            raw.type_ = we_input_event_type_v2::WE_INPUT_POINTER_WHEEL as u32;
            raw.pointer_x = x;
            raw.pointer_y = y;
            raw.wheel_delta_x = delta_x;
            raw.wheel_delta_y = delta_y;
        }
        InputEvent::KeyDown { key_code, native_key_code, modifiers, unicode_char } => {
            raw.type_ = we_input_event_type_v2::WE_INPUT_KEY_DOWN as u32;
            raw.key_code = key_code;
            raw.native_key_code = native_key_code;
            raw.modifiers = modifiers;
            raw.unicode_char = unicode_char;
        }
        InputEvent::KeyUp { key_code, native_key_code, modifiers, unicode_char } => {
            raw.type_ = we_input_event_type_v2::WE_INPUT_KEY_UP as u32;
            raw.key_code = key_code;
            raw.native_key_code = native_key_code;
            raw.modifiers = modifiers;
            raw.unicode_char = unicode_char;
        }
        InputEvent::Focus { focused } => {
            raw.type_ = we_input_event_type_v2::WE_INPUT_FOCUS as u32;
            raw.focused = focused;
        }
    }

    raw
}

fn frame_from_raw(raw: &sys::we_frame_v1) -> Result<Frame, Error> {
    match raw.kind {
        value if value == we_frame_kind_v1::WE_FRAME_KIND_DMABUF as u32 => {
            if raw.n_planes > raw.planes.len() as u32 {
                return Err(Error::InvalidPlaneCount(raw.n_planes));
            }
            let fds_omitted = raw.flags & sys::WE_FRAME_FLAG_FDS_OMITTED != 0;
            let mut planes = Vec::with_capacity(raw.n_planes as usize);
            if !fds_omitted {
                for plane in raw.planes.iter().take(raw.n_planes as usize) {
                    planes.push(DmabufPlane {
                        fd: duplicate_fd(plane.fd)?,
                        offset: plane.offset,
                        stride: plane.stride,
                    });
                }
            }
            Ok(Frame::Dmabuf(DmabufFrame {
                width: raw.width,
                height: raw.height,
                drm_fourcc: raw.drm_fourcc,
                drm_modifier: raw.drm_modifier,
                flags: raw.flags,
                serial: raw.serial,
                pts_ns: raw.pts_ns,
                buffer_id: (raw.buffer_id != sys::WE_FRAME_BUFFER_ID_INVALID)
                    .then_some(raw.buffer_id),
                fds_omitted,
                planes,
            }))
        }
        value if value == we_frame_kind_v1::WE_FRAME_KIND_SHM as u32 => Ok(Frame::Shm(ShmFrame {
            width: raw.width,
            height: raw.height,
            stride: raw.shm_stride,
            size: raw.shm_size,
            serial: raw.serial,
            pts_ns: raw.pts_ns,
            fd: duplicate_fd(raw.planes[0].fd)?,
        })),
        other => Err(Error::UnsupportedFrameKind(other)),
    }
}

fn duplicate_fd(fd: i32) -> Result<OwnedFd, Error> {
    let dup_fd = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if dup_fd < 0 {
        return Err(Error::DuplicateFd(std::io::Error::last_os_error()));
    }
    Ok(unsafe { OwnedFd::from_raw_fd(dup_fd) })
}

#[cfg(test)]
mod tests {
    use std::os::fd::AsRawFd;

    use super::{
        encode_media_state, frame_from_raw, parse_diagnostics_json, to_raw_input_event,
        DiagnosticSeverity, Frame, InputEvent, MediaImage, MediaPlaybackState, MediaState,
    };
    use we_renderer_sys::{we_dmabuf_plane_v1, we_frame_kind_v1, we_frame_v1};

    #[test]
    fn input_event_encoding_matches_abi() {
        let raw = to_raw_input_event(InputEvent::PointerDown { x: 0.25, y: 0.75, button: 1 });
        assert_eq!(raw.version, 2);
        assert_eq!(raw.type_, 1);
        assert_eq!(raw.pointer_x, 0.25);
        assert_eq!(raw.pointer_y, 0.75);
        assert_eq!(raw.button, 1);
    }

    #[test]
    fn dmabuf_frame_conversion_rejects_invalid_plane_count() {
        let raw = we_frame_v1 {
            size: std::mem::size_of::<we_frame_v1>() as u32,
            version: 1,
            kind: we_frame_kind_v1::WE_FRAME_KIND_DMABUF as u32,
            width: 1920,
            height: 1080,
            drm_fourcc: 0,
            drm_modifier: 0,
            n_planes: 5,
            flags: 0,
            serial: 1,
            pts_ns: 2,
            shm_stride: 0,
            shm_size: 0,
            planes: [we_dmabuf_plane_v1 { fd: -1, offset: 0, stride: 0 }; 4],
            buffer_id: u32::MAX,
            reusable_buffer_mask: 0,
        };

        let err = frame_from_raw(&raw).expect_err("too many planes should fail");
        assert!(err.to_string().contains("exceeds the ABI limit"));
    }

    #[test]
    fn dmabuf_frame_conversion_accepts_omitted_reusable_fds() {
        let raw = we_frame_v1 {
            size: std::mem::size_of::<we_frame_v1>() as u32,
            version: 1,
            kind: we_frame_kind_v1::WE_FRAME_KIND_DMABUF as u32,
            width: 1920,
            height: 1080,
            drm_fourcc: u32::from_le_bytes(*b"AB24"),
            drm_modifier: 0,
            n_planes: 1,
            flags: we_renderer_sys::WE_FRAME_FLAG_FDS_OMITTED,
            serial: 2,
            pts_ns: 0,
            shm_stride: 0,
            shm_size: 0,
            planes: [we_dmabuf_plane_v1 { fd: -1, offset: 0, stride: 7680 }; 4],
            buffer_id: 2,
            reusable_buffer_mask: 0,
        };

        let frame = frame_from_raw(&raw).expect("reusable DMA-BUF metadata should be valid");
        let Frame::Dmabuf(frame) = frame else {
            panic!("expected DMA-BUF frame");
        };
        assert_eq!(frame.buffer_id, Some(2));
        assert!(frame.fds_omitted);
        assert!(frame.planes.is_empty());
    }

    #[test]
    fn shm_frame_conversion_duplicates_fd() {
        let file = std::fs::File::open("/dev/null").expect("open /dev/null");
        let raw = we_frame_v1 {
            size: std::mem::size_of::<we_frame_v1>() as u32,
            version: 1,
            kind: we_frame_kind_v1::WE_FRAME_KIND_SHM as u32,
            width: 64,
            height: 64,
            drm_fourcc: 0,
            drm_modifier: 0,
            n_planes: 1,
            flags: 0,
            serial: 7,
            pts_ns: 8,
            shm_stride: 256,
            shm_size: 4096,
            planes: [
                we_dmabuf_plane_v1 { fd: file.as_raw_fd(), offset: 0, stride: 0 },
                we_dmabuf_plane_v1 { fd: -1, offset: 0, stride: 0 },
                we_dmabuf_plane_v1 { fd: -1, offset: 0, stride: 0 },
                we_dmabuf_plane_v1 { fd: -1, offset: 0, stride: 0 },
            ],
            buffer_id: u32::MAX,
            reusable_buffer_mask: 0,
        };

        let frame = frame_from_raw(&raw).expect("valid shm frame");
        match frame {
            Frame::Shm(shm) => {
                assert_eq!(shm.width, 64);
                assert_eq!(shm.height, 64);
                assert_eq!(shm.size, 4096);
            }
            Frame::Dmabuf(_) => panic!("expected shm frame"),
        }
    }

    #[test]
    fn unknown_frame_kind_returns_error() {
        let raw = we_frame_v1 {
            size: std::mem::size_of::<we_frame_v1>() as u32,
            version: 1,
            kind: 99,
            width: 1,
            height: 1,
            drm_fourcc: 0,
            drm_modifier: 0,
            n_planes: 0,
            flags: 0,
            serial: 0,
            pts_ns: 0,
            shm_stride: 0,
            shm_size: 0,
            planes: [we_dmabuf_plane_v1 { fd: -1, offset: 0, stride: 0 }; 4],
            buffer_id: u32::MAX,
            reusable_buffer_mask: 0,
        };

        let err = frame_from_raw(&raw).expect_err("unknown kind should fail");
        assert!(err.to_string().contains("unsupported frame kind"));
    }

    #[test]
    fn diagnostics_json_decodes_versioned_entries() {
        let diagnostics = parse_diagnostics_json(
            br#"{"version":1,"entries":[{"severity":"warning","source":"abi.render-config.msaa","message":"scene only"}]}"#,
        )
        .expect("valid diagnostics JSON");

        assert_eq!(diagnostics.version, 1);
        assert_eq!(diagnostics.entries.len(), 1);
        assert_eq!(diagnostics.entries[0].severity, DiagnosticSeverity::Warning);
        assert_eq!(diagnostics.entries[0].source, "abi.render-config.msaa");
        assert_eq!(diagnostics.entries[0].message, "scene only");
    }

    #[test]
    fn diagnostics_json_rejects_unknown_versions_and_invalid_payloads() {
        assert!(parse_diagnostics_json(br#"{"version":2,"entries":[]}"#).is_err());
        assert!(parse_diagnostics_json(b"not-json").is_err());
        assert!(parse_diagnostics_json(&[0xff, 0xfe]).is_err());
    }

    #[test]
    fn media_state_encoding_preserves_abi_values_strings_and_thumbnail_bytes() {
        let state = MediaState {
            playback: MediaPlaybackState::Playing,
            title: "Track".to_string(),
            artist: "Artist".to_string(),
            thumbnail: Some(MediaImage { width: 1, height: 1, rgba: vec![1, 2, 3, 4] }),
            ..MediaState::default()
        };
        let encoded = encode_media_state(&state).expect("valid media state");
        assert_eq!(encoded.raw.playback_state, 1);
        assert!(encoded.raw.has_thumbnail);
        assert_eq!(
            unsafe { std::ffi::CStr::from_ptr(encoded.raw.title) }.to_str().unwrap(),
            "Track"
        );
        assert_eq!(
            unsafe { std::slice::from_raw_parts(encoded.raw.thumbnail_rgba, 4) },
            &[1, 2, 3, 4]
        );
    }

    #[test]
    fn media_state_rejects_invalid_thumbnail_length_before_entering_ffi() {
        let state = MediaState {
            thumbnail: Some(MediaImage { width: 2, height: 2, rgba: vec![0; 4] }),
            ..MediaState::default()
        };
        assert!(encode_media_state(&state).is_err());
    }
}
