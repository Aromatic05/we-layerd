#![allow(non_camel_case_types)]

use std::{
    ffi::{c_char, OsStr},
    fmt,
};

use libloading::Library;
use thiserror::Error;

pub const WE_SOURCE_V1_VERSION: u32 = 1;
pub const WE_RENDER_CONFIG_V1_VERSION: u32 = 1;
pub const WE_RUNTIME_SETTINGS_V1_VERSION: u32 = 1;
pub const WE_FRAME_V1_VERSION: u32 = 1;
pub const WE_INPUT_EVENT_V2_VERSION: u32 = 2;

#[repr(C)]
pub struct we_session_t {
    _private: [u8; 0],
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum we_frame_kind_v1 {
    WE_FRAME_KIND_DMABUF = 1,
    WE_FRAME_KIND_SHM = 2,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum we_fill_mode_v1 {
    WE_FILL_MODE_ASPECT_CROP = 0,
    WE_FILL_MODE_STRETCH = 1,
    WE_FILL_MODE_ASPECT_FIT = 2,
    WE_FILL_MODE_CENTER = 3,
}

pub const WE_RUNTIME_SETTINGS_FPS: u32 = 1 << 0;
pub const WE_RUNTIME_SETTINGS_SPEED: u32 = 1 << 1;
pub const WE_RUNTIME_SETTINGS_VOLUME: u32 = 1 << 2;
pub const WE_RUNTIME_SETTINGS_MUTED: u32 = 1 << 3;
pub const WE_RUNTIME_SETTINGS_FILL_MODE: u32 = 1 << 4;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum we_input_event_type_v2 {
    WE_INPUT_POINTER_MOVE = 0,
    WE_INPUT_POINTER_DOWN = 1,
    WE_INPUT_POINTER_UP = 2,
    WE_INPUT_POINTER_WHEEL = 3,
    WE_INPUT_KEY_DOWN = 4,
    WE_INPUT_KEY_UP = 5,
    WE_INPUT_FOCUS = 6,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct we_input_event_v2 {
    pub size: u32,
    pub version: u32,
    pub type_: u32,
    pub pointer_x: f32,
    pub pointer_y: f32,
    pub button: i32,
    pub wheel_delta_x: i32,
    pub wheel_delta_y: i32,
    pub key_code: i32,
    pub native_key_code: i32,
    pub modifiers: i32,
    pub unicode_char: u32,
    pub focused: bool,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct we_dmabuf_plane_v1 {
    pub fd: i32,
    pub offset: u32,
    pub stride: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct we_frame_v1 {
    pub size: u32,
    pub version: u32,
    pub kind: u32,
    pub width: u32,
    pub height: u32,
    pub drm_fourcc: u32,
    pub drm_modifier: u64,
    pub n_planes: u32,
    pub flags: u32,
    pub serial: u64,
    pub pts_ns: u64,
    pub shm_stride: u32,
    pub shm_size: u32,
    pub planes: [we_dmabuf_plane_v1; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct we_source_v1 {
    pub size: u32,
    pub version: u32,
    pub uri: *const c_char,
    pub assets_uri: *const c_char,
    pub fps: i32,
    pub speed: f32,
    pub volume: f32,
    pub muted: bool,
    pub options_json: *const c_char,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct we_render_config_v1 {
    pub size: u32,
    pub version: u32,
    pub width: u32,
    pub height: u32,
    pub enable_valid_layer: bool,
    pub prefer_dmabuf: bool,
    pub allow_shm_fallback: bool,
    pub msaa_samples: u32,
    pub fill_mode: we_fill_mode_v1,
    pub rotation_degrees: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct we_runtime_settings_v1 {
    pub size: u32,
    pub version: u32,
    pub fields: u32,
    pub fps: i32,
    pub speed: f32,
    pub volume: f32,
    pub muted: bool,
    pub fill_mode: we_fill_mode_v1,
}

type WeSessionCreate = unsafe extern "C" fn() -> *mut we_session_t;
type WeSessionCreateWithCachePath = unsafe extern "C" fn(*const c_char) -> *mut we_session_t;
type WeSessionDestroy = unsafe extern "C" fn(*mut we_session_t);
type WeSessionSetSource = unsafe extern "C" fn(*mut we_session_t, *const we_source_v1) -> i32;
type WeSessionSetRenderConfig =
    unsafe extern "C" fn(*mut we_session_t, *const we_render_config_v1) -> i32;
type WeSessionSetDmabufFormats =
    unsafe extern "C" fn(*mut we_session_t, *const u32, *const u64, u32) -> i32;
type WeSessionResizeOutput = unsafe extern "C" fn(*mut we_session_t, u32, u32) -> i32;
type WeSessionSetUserPropertiesJson = unsafe extern "C" fn(*mut we_session_t, *const c_char) -> i32;
type WeSessionApplyRuntimeSettings =
    unsafe extern "C" fn(*mut we_session_t, *const we_runtime_settings_v1) -> i32;
type WeSessionPlayback = unsafe extern "C" fn(*mut we_session_t) -> i32;
type WeSessionGetFrameReadyFd = unsafe extern "C" fn(*mut we_session_t) -> i32;
type WeSessionAcquireFrame = unsafe extern "C" fn(*mut we_session_t, *mut we_frame_v1) -> i32;
type WeFrameRelease = unsafe extern "C" fn(*mut we_frame_v1);
type WeSessionSendInputEvent =
    unsafe extern "C" fn(*mut we_session_t, *const we_input_event_v2) -> i32;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("failed to load renderer library: {0}")]
    Library(#[from] libloading::Error),
}

pub struct RendererLibrary {
    _library: Library,
    we_session_create: WeSessionCreate,
    we_session_create_with_cache_path: WeSessionCreateWithCachePath,
    we_session_destroy: WeSessionDestroy,
    we_session_set_source: WeSessionSetSource,
    we_session_set_render_config: WeSessionSetRenderConfig,
    we_session_set_dmabuf_formats: WeSessionSetDmabufFormats,
    we_session_resize_output: WeSessionResizeOutput,
    we_session_set_user_properties_json: WeSessionSetUserPropertiesJson,
    we_session_apply_runtime_settings: WeSessionApplyRuntimeSettings,
    we_session_play: WeSessionPlayback,
    we_session_pause: WeSessionPlayback,
    we_session_stop: WeSessionPlayback,
    we_session_tick: WeSessionPlayback,
    we_session_get_frame_ready_fd: WeSessionGetFrameReadyFd,
    we_session_acquire_frame: WeSessionAcquireFrame,
    we_frame_release: WeFrameRelease,
    we_session_send_input_event: WeSessionSendInputEvent,
}

impl fmt::Debug for RendererLibrary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RendererLibrary").finish_non_exhaustive()
    }
}

impl RendererLibrary {
    pub fn load(path: impl AsRef<OsStr>) -> Result<Self, LoadError> {
        let library = unsafe { Library::new(path)? };
        let we_session_create = unsafe { *library.get::<WeSessionCreate>(b"we_session_create\0")? };
        let we_session_create_with_cache_path = unsafe {
            *library.get::<WeSessionCreateWithCachePath>(b"we_session_create_with_cache_path\0")?
        };
        let we_session_destroy =
            unsafe { *library.get::<WeSessionDestroy>(b"we_session_destroy\0")? };
        let we_session_set_source =
            unsafe { *library.get::<WeSessionSetSource>(b"we_session_set_source\0")? };
        let we_session_set_render_config =
            unsafe { *library.get::<WeSessionSetRenderConfig>(b"we_session_set_render_config\0")? };
        let we_session_set_dmabuf_formats = unsafe {
            *library.get::<WeSessionSetDmabufFormats>(b"we_session_set_dmabuf_formats\0")?
        };
        let we_session_resize_output =
            unsafe { *library.get::<WeSessionResizeOutput>(b"we_session_resize_output\0")? };
        let we_session_set_user_properties_json = unsafe {
            *library.get::<WeSessionSetUserPropertiesJson>(b"we_session_set_user_properties_json\0")?
        };
        let we_session_apply_runtime_settings = unsafe {
            *library.get::<WeSessionApplyRuntimeSettings>(b"we_session_apply_runtime_settings\0")?
        };
        let we_session_play = unsafe { *library.get::<WeSessionPlayback>(b"we_session_play\0")? };
        let we_session_pause = unsafe { *library.get::<WeSessionPlayback>(b"we_session_pause\0")? };
        let we_session_stop = unsafe { *library.get::<WeSessionPlayback>(b"we_session_stop\0")? };
        let we_session_tick = unsafe { *library.get::<WeSessionPlayback>(b"we_session_tick\0")? };
        let we_session_get_frame_ready_fd = unsafe {
            *library.get::<WeSessionGetFrameReadyFd>(b"we_session_get_frame_ready_fd\0")?
        };
        let we_session_acquire_frame =
            unsafe { *library.get::<WeSessionAcquireFrame>(b"we_session_acquire_frame\0")? };
        let we_frame_release = unsafe { *library.get::<WeFrameRelease>(b"we_frame_release\0")? };
        let we_session_send_input_event =
            unsafe { *library.get::<WeSessionSendInputEvent>(b"we_session_send_input_event\0")? };

        Ok(Self {
            _library: library,
            we_session_create,
            we_session_create_with_cache_path,
            we_session_destroy,
            we_session_set_source,
            we_session_set_render_config,
            we_session_set_dmabuf_formats,
            we_session_resize_output,
            we_session_set_user_properties_json,
            we_session_apply_runtime_settings,
            we_session_play,
            we_session_pause,
            we_session_stop,
            we_session_tick,
            we_session_get_frame_ready_fd,
            we_session_acquire_frame,
            we_frame_release,
            we_session_send_input_event,
        })
    }

    /// # Safety
    ///
    /// The caller must pass a valid cache path pointer or null and handle the returned pointer.
    pub unsafe fn session_create(&self) -> *mut we_session_t {
        (self.we_session_create)()
    }

    /// # Safety
    pub unsafe fn session_create_with_cache_path(
        &self,
        cache_path: *const c_char,
    ) -> *mut we_session_t {
        (self.we_session_create_with_cache_path)(cache_path)
    }

    /// # Safety
    pub unsafe fn session_destroy(&self, session: *mut we_session_t) {
        (self.we_session_destroy)(session)
    }

    /// # Safety
    pub unsafe fn session_set_source(
        &self,
        session: *mut we_session_t,
        source: *const we_source_v1,
    ) -> i32 {
        (self.we_session_set_source)(session, source)
    }

    /// # Safety
    pub unsafe fn session_set_render_config(
        &self,
        session: *mut we_session_t,
        config: *const we_render_config_v1,
    ) -> i32 {
        (self.we_session_set_render_config)(session, config)
    }

    /// # Safety
    pub unsafe fn session_set_dmabuf_formats(
        &self,
        session: *mut we_session_t,
        fourccs: *const u32,
        modifiers: *const u64,
        count: u32,
    ) -> i32 {
        (self.we_session_set_dmabuf_formats)(session, fourccs, modifiers, count)
    }

    /// # Safety
    pub unsafe fn session_resize_output(&self, session: *mut we_session_t, width: u32, height: u32) -> i32 {
        (self.we_session_resize_output)(session, width, height)
    }

    /// # Safety
    pub unsafe fn session_set_user_properties_json(
        &self,
        session: *mut we_session_t,
        properties_json: *const c_char,
    ) -> i32 {
        (self.we_session_set_user_properties_json)(session, properties_json)
    }

    /// # Safety
    pub unsafe fn session_apply_runtime_settings(
        &self,
        session: *mut we_session_t,
        settings: *const we_runtime_settings_v1,
    ) -> i32 {
        (self.we_session_apply_runtime_settings)(session, settings)
    }

    /// # Safety
    pub unsafe fn session_play(&self, session: *mut we_session_t) -> i32 {
        (self.we_session_play)(session)
    }

    /// # Safety
    pub unsafe fn session_pause(&self, session: *mut we_session_t) -> i32 {
        (self.we_session_pause)(session)
    }

    /// # Safety
    pub unsafe fn session_stop(&self, session: *mut we_session_t) -> i32 {
        (self.we_session_stop)(session)
    }

    /// # Safety
    pub unsafe fn session_tick(&self, session: *mut we_session_t) -> i32 {
        (self.we_session_tick)(session)
    }

    /// # Safety
    ///
    /// The returned descriptor is borrowed from the session and remains valid until it is destroyed.
    pub unsafe fn session_get_frame_ready_fd(&self, session: *mut we_session_t) -> i32 {
        (self.we_session_get_frame_ready_fd)(session)
    }

    /// # Safety
    pub unsafe fn session_acquire_frame(
        &self,
        session: *mut we_session_t,
        out_frame: *mut we_frame_v1,
    ) -> i32 {
        (self.we_session_acquire_frame)(session, out_frame)
    }

    /// # Safety
    pub unsafe fn frame_release(&self, frame: *mut we_frame_v1) {
        (self.we_frame_release)(frame)
    }

    /// # Safety
    pub unsafe fn session_send_input_event(
        &self,
        session: *mut we_session_t,
        event: *const we_input_event_v2,
    ) -> i32 {
        (self.we_session_send_input_event)(session, event)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        we_frame_v1, we_input_event_v2, we_render_config_v1, we_runtime_settings_v1, we_source_v1,
        RendererLibrary,
    };
    use std::mem::{align_of, offset_of, size_of};

    #[test]
    fn c_layout_sizes_stay_stable() {
        assert_eq!(size_of::<we_source_v1>(), 48);
        assert_eq!(align_of::<we_source_v1>(), 8);
        assert_eq!(offset_of!(we_source_v1, uri), 8);
        assert_eq!(offset_of!(we_source_v1, assets_uri), 16);
        assert_eq!(offset_of!(we_source_v1, options_json), 40);

        assert_eq!(size_of::<we_render_config_v1>(), 32);
        assert_eq!(align_of::<we_render_config_v1>(), 4);
        assert_eq!(offset_of!(we_render_config_v1, width), 8);
        assert_eq!(offset_of!(we_render_config_v1, allow_shm_fallback), 18);
        assert_eq!(offset_of!(we_render_config_v1, msaa_samples), 20);
        assert_eq!(offset_of!(we_render_config_v1, rotation_degrees), 28);

        assert_eq!(size_of::<we_runtime_settings_v1>(), 32);
        assert_eq!(align_of::<we_runtime_settings_v1>(), 4);
        assert_eq!(offset_of!(we_runtime_settings_v1, fill_mode), 28);

        assert_eq!(size_of::<we_frame_v1>(), 112);
        assert_eq!(align_of::<we_frame_v1>(), 8);
        assert_eq!(offset_of!(we_frame_v1, kind), 8);
        assert_eq!(offset_of!(we_frame_v1, planes), 64);

        assert_eq!(size_of::<we_input_event_v2>(), 52);
        assert_eq!(align_of::<we_input_event_v2>(), 4);
        assert_eq!(offset_of!(we_input_event_v2, focused), 48);
    }

    #[test]
    fn loading_missing_library_fails() {
        let err = RendererLibrary::load("definitely-missing-libwallpaper-engine-renderer.so")
            .expect_err("missing library should fail");
        assert!(err.to_string().contains("failed to load renderer library"));
    }
}
