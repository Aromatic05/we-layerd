use std::{
    fs::File,
    os::fd::{AsFd, FromRawFd},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::{anyhow, Context, Result};
use wayland_client::{
    protocol::{
        wl_buffer::WlBuffer,
        wl_compositor::WlCompositor,
        wl_shm::{self, WlShm},
        wl_surface::WlSurface,
    },
    QueueHandle,
};
use wayland_protocols::wp::viewporter::client::{
    wp_viewport::WpViewport, wp_viewporter::WpViewporter,
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{Anchor, KeyboardInteractivity, ZwlrLayerSurfaceV1},
};
use we_core::config::InputMode;

use super::state::LayerShellState;

const INPUT_PROXY_NAMESPACE: &str = "we-layerd-input-proxy";

fn proxy_enabled(interactive: bool, mode: InputMode) -> bool {
    interactive && mode == InputMode::BackgroundProxy
}

fn buffer_dimensions(has_viewporter: bool, logical_size: (u32, u32)) -> (u32, u32) {
    if has_viewporter {
        (1, 1)
    } else {
        logical_size
    }
}

pub(super) fn accepts_pointer_surface(mode: InputMode, is_wallpaper: bool, is_proxy: bool) -> bool {
    match mode {
        InputMode::Direct => is_wallpaper,
        InputMode::BackgroundProxy => is_proxy,
    }
}

struct TransparentBuffer {
    buffer: WlBuffer,
    _backing_file: File,
    released: Arc<AtomicBool>,
    dimensions: (u32, u32),
}

impl Drop for TransparentBuffer {
    fn drop(&mut self) {
        self.buffer.destroy();
    }
}

pub(super) struct InputProxy {
    pub(super) surface: WlSurface,
    pub(super) layer_surface: ZwlrLayerSurfaceV1,
    viewport: Option<WpViewport>,
    pub(super) configured: bool,
    pub(super) configured_size: (u32, u32),
    committed_size: Option<(u32, u32)>,
    buffers: Vec<TransparentBuffer>,
}

pub(super) fn create(
    state: &mut LayerShellState,
    qh: &QueueHandle<LayerShellState>,
    layer_shell: &ZwlrLayerShellV1,
    viewporter: Option<&WpViewporter>,
) -> Result<()> {
    if !proxy_enabled(state.interactive, state.input_mode) {
        return Ok(());
    }

    let compositor = state.objects.compositor.as_ref().context("wl_compositor unavailable")?;
    let surface = compositor.create_surface(qh, ());
    surface.set_buffer_scale(1);
    let layer_surface = layer_shell.get_layer_surface(
        &surface,
        state.objects.output.as_ref(),
        Layer::Background,
        INPUT_PROXY_NAMESPACE.to_string(),
        qh,
        (),
    );
    layer_surface.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
    layer_surface.set_size(0, 0);
    layer_surface.set_exclusive_zone(-1);
    layer_surface.set_margin(0, 0, 0, 0);
    layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);

    let viewport = viewporter.map(|manager| manager.get_viewport(&surface, qh, ()));
    state.input_proxy = Some(InputProxy {
        surface: surface.clone(),
        layer_surface,
        viewport,
        configured: false,
        configured_size: (0, 0),
        committed_size: None,
        buffers: Vec::new(),
    });

    // A layer-shell surface first commits without a buffer, then receives a
    // configure event. Only after ack_configure can it attach the transparent
    // buffer and become eligible for pointer hit-testing.
    surface.commit();
    Ok(())
}

fn create_transparent_buffer(
    shm: &WlShm,
    qh: &QueueHandle<LayerShellState>,
    dimensions: (u32, u32),
) -> Result<TransparentBuffer> {
    let (width, height) = dimensions;
    let stride = width
        .checked_mul(4)
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| anyhow!("input proxy buffer stride overflow"))?;
    let byte_len = i64::from(stride)
        .checked_mul(i64::from(height))
        .and_then(|value| i32::try_from(value).ok())
        .ok_or_else(|| anyhow!("input proxy buffer size overflow"))?;

    let file = allocate_zeroed_backing_file(byte_len)?;

    let released = Arc::new(AtomicBool::new(false));
    let pool = shm.create_pool(file.as_fd(), byte_len, qh, ());
    let buffer = pool.create_buffer(
        0,
        width as i32,
        height as i32,
        stride,
        wl_shm::Format::Argb8888,
        qh,
        Arc::clone(&released),
    );
    pool.destroy();
    Ok(TransparentBuffer { buffer, _backing_file: file, released, dimensions })
}

fn allocate_zeroed_backing_file(byte_len: i32) -> Result<File> {
    let fd = unsafe {
        libc::memfd_create(b"we-layerd-input-proxy\0".as_ptr().cast(), libc::MFD_CLOEXEC)
    };
    if fd < 0 {
        return Err(std::io::Error::last_os_error()).context("input proxy memfd_create failed");
    }
    let file = unsafe { File::from_raw_fd(fd) };
    // Sparse, zero-filled SHM produces ARGB8888 pixels with alpha = 0.
    file.set_len(byte_len as u64).context("failed to size input proxy SHM buffer")?;
    Ok(file)
}

pub(super) fn update(state: &mut LayerShellState, qh: &QueueHandle<LayerShellState>) -> Result<()> {
    if !state.configured {
        return Ok(());
    }
    let Some(proxy) = state.input_proxy.as_mut() else { return Ok(()) };
    if !proxy.configured {
        return Ok(());
    }

    let logical_size = (
        if proxy.configured_size.0 > 0 {
            proxy.configured_size.0
        } else {
            state.output.logical_width.max(state.output.fallback_width)
        }
        .max(1),
        if proxy.configured_size.1 > 0 {
            proxy.configured_size.1
        } else {
            state.output.logical_height.max(state.output.fallback_height)
        }
        .max(1),
    );
    if proxy.committed_size == Some(logical_size) {
        // A configure still requires a subsequent commit even if dimensions
        // didn't change.
        proxy.surface.commit();
        return Ok(());
    }

    let compositor: &WlCompositor =
        state.objects.compositor.as_ref().context("wl_compositor unavailable")?;
    let shm = state.objects.shm.as_ref().context("wl_shm unavailable")?;
    let required_dimensions = buffer_dimensions(proxy.viewport.is_some(), logical_size);
    if proxy.buffers.last().map_or(true, |last| last.dimensions != required_dimensions) {
        proxy.buffers.push(create_transparent_buffer(shm, qh, required_dimensions)?);
    }

    let region = compositor.create_region(qh, ());
    region.add(0, 0, logical_size.0 as i32, logical_size.1 as i32);
    proxy.surface.set_input_region(Some(&region));
    region.destroy();

    if let Some(viewport) = proxy.viewport.as_ref() {
        viewport.set_destination(logical_size.0 as i32, logical_size.1 as i32);
    }
    proxy.surface.attach(Some(&proxy.buffers.last().expect("buffer created").buffer), 0, 0);
    proxy.surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
    proxy.surface.commit();
    proxy.committed_size = Some(logical_size);

    // Retired SHM buffers must remain alive until the compositor releases them.
    let keep = proxy.buffers.len() - 1;
    let mut index = 0;
    proxy.buffers.retain(|buffer| {
        let retain = index == keep || !buffer.released.load(Ordering::SeqCst);
        index += 1;
        retain
    });
    Ok(())
}

pub(super) fn detach(state: &mut LayerShellState) {
    if let Some(proxy) = state.input_proxy.as_mut() {
        proxy.surface.attach(None, 0, 0);
        proxy.surface.commit();
    }
}

pub(super) fn close(state: &mut LayerShellState) {
    state.pointer_left();
    state.pointer_surface = None;
    if let Some(proxy) = state.input_proxy.take() {
        if let Some(viewport) = proxy.viewport.as_ref() {
            viewport.destroy();
        }
        proxy.layer_surface.destroy();
        proxy.surface.destroy();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        accepts_pointer_surface, allocate_zeroed_backing_file, buffer_dimensions, proxy_enabled,
    };
    use std::io::Read;
    use we_core::config::InputMode;

    #[test]
    fn proxy_is_opt_in_and_requires_interactivity() {
        assert!(!proxy_enabled(true, InputMode::Direct));
        assert!(!proxy_enabled(false, InputMode::BackgroundProxy));
        assert!(proxy_enabled(true, InputMode::BackgroundProxy));
    }

    #[test]
    fn transparent_proxy_buffer_uses_viewport_or_shm_fallback() {
        assert_eq!(buffer_dimensions(true, (2560, 1440)), (1, 1));
        assert_eq!(buffer_dimensions(false, (2560, 1440)), (2560, 1440));
    }

    #[test]
    fn proxy_backing_file_has_zero_alpha() {
        let mut backing = allocate_zeroed_backing_file(4).unwrap();
        let mut pixel = [255u8; 4];
        backing.read_exact(&mut pixel).unwrap();
        assert_eq!(pixel, [0, 0, 0, 0]);
    }

    #[test]
    fn direct_mode_only_targets_the_wallpaper_surface() {
        assert!(accepts_pointer_surface(InputMode::Direct, true, false));
        assert!(!accepts_pointer_surface(InputMode::Direct, false, true));
        assert!(!accepts_pointer_surface(InputMode::Direct, false, false));
    }

    #[test]
    fn proxy_mode_only_targets_the_input_surface() {
        assert!(accepts_pointer_surface(InputMode::BackgroundProxy, false, true));
        assert!(!accepts_pointer_surface(InputMode::BackgroundProxy, true, false));
        assert!(!accepts_pointer_surface(InputMode::BackgroundProxy, false, false));
    }
}
