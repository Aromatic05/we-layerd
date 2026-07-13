use std::{
    ffi::CString,
    os::fd::{FromRawFd, OwnedFd},
    path::Path,
    sync::Arc,
};

use thiserror::Error;
use we_renderer_sys::{
    self as sys, we_fill_mode_v1, we_frame_kind_v1, we_input_event_type_v2,
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
    #[error("failed to duplicate frame fd")]
    DuplicateFd(#[source] std::io::Error),
}

impl RendererLibrary {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Error> {
        let inner = SysRendererLibrary::load(path.as_ref().as_os_str())?;
        Ok(Self { inner: Arc::new(inner) })
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

    pub fn resize_output(&mut self, width: u32, height: u32) -> Result<(), Error> {
        self.check_status(
            unsafe { self.library.session_resize_output(self.raw, width, height) },
            "we_session_resize_output",
        )
    }

    pub fn set_user_properties_json(&mut self, properties_json: &str) -> Result<(), Error> {
        let properties_json = CString::new(properties_json)
            .map_err(|_| Error::InvalidSourceString)?;
        self.check_status(
            unsafe {
                self.library
                    .session_set_user_properties_json(self.raw, properties_json.as_ptr())
            },
            "we_session_set_user_properties_json",
        )
    }

    pub fn apply_runtime_settings(&mut self, settings: RuntimeSettings) -> Result<(), Error> {
        let mut fields = 0;
        if settings.fps.is_some() { fields |= sys::WE_RUNTIME_SETTINGS_FPS; }
        if settings.speed.is_some() { fields |= sys::WE_RUNTIME_SETTINGS_SPEED; }
        if settings.volume.is_some() { fields |= sys::WE_RUNTIME_SETTINGS_VOLUME; }
        if settings.muted.is_some() { fields |= sys::WE_RUNTIME_SETTINGS_MUTED; }
        if settings.fill_mode.is_some() { fields |= sys::WE_RUNTIME_SETTINGS_FILL_MODE; }
        if fields == 0 { return Ok(()); }

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

    fn check_status(&self, status: i32, op: &'static str) -> Result<(), Error> {
        if status == 0 {
            Ok(())
        } else {
            Err(Error::Status(status, op))
        }
    }
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
            let mut planes = Vec::with_capacity(raw.n_planes as usize);
            for plane in raw.planes.iter().take(raw.n_planes as usize) {
                planes.push(DmabufPlane {
                    fd: duplicate_fd(plane.fd)?,
                    offset: plane.offset,
                    stride: plane.stride,
                });
            }
            Ok(Frame::Dmabuf(DmabufFrame {
                width: raw.width,
                height: raw.height,
                drm_fourcc: raw.drm_fourcc,
                drm_modifier: raw.drm_modifier,
                flags: raw.flags,
                serial: raw.serial,
                pts_ns: raw.pts_ns,
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

    use super::{frame_from_raw, to_raw_input_event, Frame, InputEvent};
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
        };

        let err = frame_from_raw(&raw).expect_err("too many planes should fail");
        assert!(err.to_string().contains("exceeds the ABI limit"));
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
        };

        let err = frame_from_raw(&raw).expect_err("unknown kind should fail");
        assert!(err.to_string().contains("unsupported frame kind"));
    }
}
