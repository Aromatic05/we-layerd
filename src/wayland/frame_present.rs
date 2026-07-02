use std::{
    os::fd::{AsFd, OwnedFd},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use anyhow::{anyhow, Result};
use wayland_client::{
    protocol::{
        wl_buffer::WlBuffer, wl_shm, wl_shm::WlShm, wl_shm_pool::WlShmPool, wl_surface::WlSurface,
    },
    Dispatch, QueueHandle,
};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_buffer_params_v1::{Flags as DmabufFlags, ZwpLinuxBufferParamsV1},
    zwp_linux_dmabuf_v1::ZwpLinuxDmabufV1,
};

use we_renderer::{DmabufFrame, Frame, ShmFrame};

#[derive(Debug, Clone)]
pub struct BufferState {
    pub(crate) released: Arc<AtomicBool>,
}

impl BufferState {
    fn new() -> (Self, Arc<AtomicBool>) {
        let released = Arc::new(AtomicBool::new(false));
        (Self { released: Arc::clone(&released) }, released)
    }
}

#[derive(Debug)]
struct PresentedBuffer {
    buffer: WlBuffer,
    released: Arc<AtomicBool>,
    pending_fds: Vec<OwnedFd>,
}

#[derive(Debug)]
pub struct FramePresenter {
    compositor_version: u32,
    dmabuf: Option<ZwpLinuxDmabufV1>,
    shm: Option<WlShm>,
    in_flight: Vec<PresentedBuffer>,
}

impl FramePresenter {
    pub fn new(
        compositor_version: u32,
        dmabuf: Option<ZwpLinuxDmabufV1>,
        shm: Option<WlShm>,
    ) -> Self {
        Self { compositor_version, dmabuf, shm, in_flight: Vec::new() }
    }

    pub fn present<State>(
        &mut self,
        surface: &WlSurface,
        qh: &QueueHandle<State>,
        frame: Frame,
    ) -> Result<()>
    where
        State: Dispatch<WlBuffer, BufferState>
            + Dispatch<WlShmPool, ()>
            + Dispatch<ZwpLinuxBufferParamsV1, ()>
            + 'static,
    {
        let entry = self.create_buffer_for_frame(qh, frame)?;
        surface.attach(Some(&entry.buffer), 0, 0);
        if self.compositor_version >= 4 {
            surface.damage_buffer(0, 0, i32::MAX, i32::MAX);
        } else {
            surface.damage(0, 0, i32::MAX, i32::MAX);
        }
        surface.commit();
        self.in_flight.push(entry);
        Ok(())
    }

    pub fn release_pending_send_fds(&mut self) {
        for entry in &mut self.in_flight {
            entry.pending_fds.clear();
        }
    }

    pub fn collect_released_buffers(&mut self) {
        self.in_flight.retain(|entry| {
            !(entry.pending_fds.is_empty() && entry.released.load(Ordering::SeqCst))
        });
    }

    fn create_buffer_for_frame<State>(
        &self,
        qh: &QueueHandle<State>,
        frame: Frame,
    ) -> Result<PresentedBuffer>
    where
        State: Dispatch<WlBuffer, BufferState>
            + Dispatch<WlShmPool, ()>
            + Dispatch<ZwpLinuxBufferParamsV1, ()>
            + 'static,
    {
        match frame {
            Frame::Shm(frame) => self.create_shm_buffer(qh, frame),
            Frame::Dmabuf(frame) => self.create_dmabuf_buffer(qh, frame),
        }
    }

    fn create_shm_buffer<State>(
        &self,
        qh: &QueueHandle<State>,
        frame: ShmFrame,
    ) -> Result<PresentedBuffer>
    where
        State: Dispatch<WlBuffer, BufferState> + Dispatch<WlShmPool, ()> + 'static,
    {
        let shm = self.shm.as_ref().ok_or_else(|| anyhow!("wl_shm is unavailable"))?;
        let (buffer_state, released) = BufferState::new();
        let pool = shm.create_pool(frame.fd.as_fd(), frame.size as i32, qh, ());
        let buffer = pool.create_buffer(
            0,
            frame.width as i32,
            frame.height as i32,
            frame.stride as i32,
            wl_shm::Format::Xrgb8888,
            qh,
            buffer_state,
        );
        pool.destroy();

        Ok(PresentedBuffer { buffer, released, pending_fds: vec![frame.fd] })
    }

    fn create_dmabuf_buffer<State>(
        &self,
        qh: &QueueHandle<State>,
        frame: DmabufFrame,
    ) -> Result<PresentedBuffer>
    where
        State: Dispatch<WlBuffer, BufferState> + Dispatch<ZwpLinuxBufferParamsV1, ()> + 'static,
    {
        let dmabuf =
            self.dmabuf.as_ref().ok_or_else(|| anyhow!("zwp_linux_dmabuf_v1 is unavailable"))?;
        let (buffer_state, released) = BufferState::new();
        let params = dmabuf.create_params(qh, ());
        for (index, plane) in frame.planes.iter().enumerate() {
            let modifier_hi = (frame.drm_modifier >> 32) as u32;
            let modifier_lo = (frame.drm_modifier & 0xffff_ffff) as u32;
            params.add(
                plane.fd.as_fd(),
                index as u32,
                plane.offset,
                plane.stride,
                modifier_hi,
                modifier_lo,
            );
        }
        let buffer = params.create_immed(
            frame.width as i32,
            frame.height as i32,
            to_opaque_drm_fourcc(frame.drm_fourcc),
            DmabufFlags::empty(),
            qh,
            buffer_state,
        );
        params.destroy();

        let pending_fds = frame.planes.into_iter().map(|plane| plane.fd).collect();
        Ok(PresentedBuffer { buffer, released, pending_fds })
    }
}

fn to_opaque_drm_fourcc(fourcc: u32) -> u32 {
    const DRM_FORMAT_ABGR8888: u32 = u32::from_le_bytes(*b"AB24");
    const DRM_FORMAT_XBGR8888: u32 = u32::from_le_bytes(*b"XB24");
    const DRM_FORMAT_ARGB8888: u32 = u32::from_le_bytes(*b"AR24");
    const DRM_FORMAT_XRGB8888: u32 = u32::from_le_bytes(*b"XR24");

    match fourcc {
        DRM_FORMAT_ABGR8888 => DRM_FORMAT_XBGR8888,
        DRM_FORMAT_ARGB8888 => DRM_FORMAT_XRGB8888,
        _ => fourcc,
    }
}
