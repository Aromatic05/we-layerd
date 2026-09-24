use anyhow::{Context, Result};
use we_renderer::ShmFrame;
use x11rb::{
    connection::Connection as _,
    protocol::{
        randr,
        xproto::{
            self, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, ImageFormat, PropMode,
            WindowClass,
        },
        Event,
    },
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Monitor {
    pub name: String,
    pub x: i16,
    pub y: i16,
    pub width: u32,
    pub height: u32,
}

pub(super) struct Window {
    pub monitor: Monitor,
    pub xid: u32,
    gc: u32,
    pub image_width: u32,
    pub image_height: u32,
}

pub(super) struct X11Windows {
    pub connection: RustConnection,
    root: u32,
    depth: u8,
    pub windows: Vec<Window>,
}

impl X11Windows {
    pub fn create(render_size: Option<(u32, u32)>) -> Result<Self> {
        let (connection, screen_num) = RustConnection::connect(None)
            .context("failed to connect to XWayland; GNOME wallpaper windows require XWayland")?;
        let screen = connection
            .setup()
            .roots
            .get(screen_num)
            .context("X11 server did not expose the selected screen")?;
        let root = screen.root;
        let depth = screen.root_depth;
        let pixmap_format = connection
            .setup()
            .pixmap_formats
            .iter()
            .find(|format| format.depth == depth)
            .context("X11 server did not provide a pixel format for the root window")?;
        if pixmap_format.bits_per_pixel != 32
            || connection.setup().image_byte_order != xproto::ImageOrder::LSB_FIRST
        {
            anyhow::bail!(
                "GNOME XWayland bridge currently requires little-endian 32-bit XRGB8888 windows"
            );
        }
        let monitors =
            query_monitors(&connection, root, screen.width_in_pixels, screen.height_in_pixels);
        randr::select_input(
            &connection,
            root,
            randr::NotifyMask::SCREEN_CHANGE
                | randr::NotifyMask::CRTC_CHANGE
                | randr::NotifyMask::OUTPUT_CHANGE
                | randr::NotifyMask::RESOURCE_CHANGE,
        )?
        .check()
        .context("failed to subscribe to XRandR monitor changes")?;
        let mut windows = Vec::with_capacity(monitors.len());
        let pid = std::process::id();
        let title = "we-layerd wallpaper renderer";
        let wm_class = "we-layerd";

        let wm_name = intern(&connection, "WM_NAME")?;
        let net_wm_name = intern(&connection, "_NET_WM_NAME")?;
        let utf8_string = intern(&connection, "UTF8_STRING")?;
        let net_wm_pid = intern(&connection, "_NET_WM_PID")?;
        let wm_class_atom = intern(&connection, "WM_CLASS")?;
        let net_wm_window_type = intern(&connection, "_NET_WM_WINDOW_TYPE")?;
        let net_wm_window_type_normal = intern(&connection, "_NET_WM_WINDOW_TYPE_NORMAL")?;
        let net_wm_state = intern(&connection, "_NET_WM_STATE")?;
        let skip_pager = intern(&connection, "_NET_WM_STATE_SKIP_PAGER")?;
        let motif_hints = intern(&connection, "_MOTIF_WM_HINTS")?;

        for monitor in monitors {
            let (image_width, image_height) =
                render_size.unwrap_or((monitor.width, monitor.height));
            let width = image_width.clamp(1, u16::MAX as u32);
            let height = image_height.clamp(1, u16::MAX as u32);
            let x = monitor.x as i32 + (monitor.width as i32 - width as i32) / 2;
            let y = monitor.y as i32 + (monitor.height as i32 - height as i32) / 2;
            let xid = connection.generate_id().context("failed to allocate X11 window ID")?;
            connection
                .create_window(
                    depth,
                    xid,
                    root,
                    x.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                    y.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                    width as u16,
                    height as u16,
                    0,
                    WindowClass::INPUT_OUTPUT,
                    0,
                    &CreateWindowAux::new()
                        .background_pixel(0)
                        .event_mask(EventMask::EXPOSURE | EventMask::STRUCTURE_NOTIFY),
                )
                .with_context(|| {
                    format!("failed to create X11 wallpaper window for {}", monitor.name)
                })?;

            connection.change_property8(
                PropMode::REPLACE,
                xid,
                wm_name,
                AtomEnum::STRING,
                title.as_bytes(),
            )?;
            connection.change_property8(
                PropMode::REPLACE,
                xid,
                net_wm_name,
                utf8_string,
                title.as_bytes(),
            )?;
            connection.change_property32(
                PropMode::REPLACE,
                xid,
                net_wm_pid,
                AtomEnum::CARDINAL,
                &[pid],
            )?;
            let mut class = wm_class.as_bytes().to_vec();
            class.push(0);
            class.extend_from_slice(wm_class.as_bytes());
            class.push(0);
            connection.change_property8(
                PropMode::REPLACE,
                xid,
                wm_class_atom,
                AtomEnum::STRING,
                &class,
            )?;
            connection.change_property32(
                PropMode::REPLACE,
                xid,
                net_wm_window_type,
                AtomEnum::ATOM,
                &[net_wm_window_type_normal],
            )?;
            connection.change_property32(
                PropMode::REPLACE,
                xid,
                net_wm_state,
                AtomEnum::ATOM,
                // Mutter disables minimizing skip-taskbar windows. The GNOME
                // extension filters this renderer from shell window lists.
                &[skip_pager],
            )?;
            connection.change_property32(
                PropMode::REPLACE,
                xid,
                motif_hints,
                motif_hints,
                &[2, 0, 0, 0, 0],
            )?;
            let gc = connection.generate_id().context("failed to allocate X11 graphics context")?;
            connection.create_gc(gc, xid, &xproto::CreateGCAux::new())?;
            connection.map_window(xid)?;
            windows.push(Window { monitor, xid, gc, image_width: width, image_height: height });
        }
        connection
            .get_input_focus()
            .context("failed to wait for X11 wallpaper windows")?
            .reply()
            .context("X11 server rejected wallpaper window setup")?;
        connection.flush().context("failed to map X11 wallpaper windows")?;

        Ok(Self { connection, root, depth, windows })
    }

    pub fn present(&self, window: &Window, frame: ShmFrame) -> Result<()> {
        if frame.width > window.image_width || frame.height > window.image_height {
            anyhow::bail!(
                "renderer frame {}x{} exceeds wallpaper window {}x{}",
                frame.width,
                frame.height,
                window.image_width,
                window.image_height
            );
        }
        if frame.stride < frame.width.saturating_mul(4) {
            anyhow::bail!("renderer returned an invalid XRGB8888 frame stride");
        }
        let source_byte_len = (frame.stride as usize)
            .checked_mul(frame.height as usize)
            .filter(|len| *len <= frame.size as usize)
            .context("renderer SHM frame size is inconsistent with its geometry")?;
        let row_bytes =
            (frame.width as usize).checked_mul(4).context("renderer SHM frame width overflowed")?;
        let mapped = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                source_byte_len,
                libc::PROT_READ,
                libc::MAP_SHARED,
                std::os::fd::AsRawFd::as_raw_fd(&frame.fd),
                0,
            )
        };
        if mapped == libc::MAP_FAILED {
            return Err(std::io::Error::last_os_error())
                .context("failed to map renderer SHM frame");
        }
        let bytes = unsafe { std::slice::from_raw_parts(mapped.cast::<u8>(), source_byte_len) };
        let stride = frame.stride as usize;
        // Stay below the core X11 maximum request size, including request overhead.
        let rows_per_request = (128 * 1024 / row_bytes.max(1)).clamp(1, frame.height as usize);
        let result = (|| -> Result<()> {
            for first_row in (0..frame.height as usize).step_by(rows_per_request) {
                let row_count = rows_per_request.min(frame.height as usize - first_row);
                let mut packed = Vec::with_capacity(row_count * row_bytes);
                for row in first_row..first_row + row_count {
                    let start = row * stride;
                    packed.extend_from_slice(&bytes[start..start + row_bytes]);
                }
                self.connection
                    .put_image(
                        ImageFormat::Z_PIXMAP,
                        window.xid,
                        window.gc,
                        frame.width as u16,
                        row_count as u16,
                        0,
                        first_row as i16,
                        0,
                        self.depth,
                        &packed,
                    )
                    .with_context(|| {
                        format!("failed to submit wallpaper frame for {}", window.monitor.name)
                    })?;
            }
            self.connection.flush().context("failed to flush wallpaper frame to XWayland")?;
            Ok(())
        })();
        unsafe {
            libc::munmap(mapped, source_byte_len);
        }
        result
    }

    pub fn destroy(&mut self) {
        for window in self.windows.drain(..) {
            let _ = self.connection.free_gc(window.gc);
            let _ = self.connection.destroy_window(window.xid);
        }
        let _ = self.connection.flush();
    }

    pub fn topology_changed(&self) -> Result<bool> {
        let mut received_randr_event = false;
        while let Some(event) = self.connection.poll_for_event()? {
            received_randr_event |=
                matches!(event, Event::RandrNotify(_) | Event::RandrScreenChangeNotify(_));
        }
        if !received_randr_event {
            return Ok(false);
        }

        let geometry = self.connection.get_geometry(self.root)?.reply()?;
        let monitors = query_monitors(&self.connection, self.root, geometry.width, geometry.height);
        Ok(monitors.len() != self.windows.len()
            || monitors
                .iter()
                .zip(&self.windows)
                .any(|(monitor, window)| monitor != &window.monitor))
    }
}

impl Drop for X11Windows {
    fn drop(&mut self) {
        self.destroy();
    }
}

fn intern(connection: &RustConnection, name: &str) -> Result<u32> {
    Ok(connection.intern_atom(false, name.as_bytes())?.reply()?.atom)
}

fn query_monitors(connection: &RustConnection, root: u32, width: u16, height: u16) -> Vec<Monitor> {
    if let Some(reply) =
        randr::get_monitors(connection, root, true).ok().and_then(|cookie| cookie.reply().ok())
    {
        let mut monitors = Vec::new();
        for (index, info) in reply.monitors.into_iter().enumerate() {
            if info.width == 0 || info.height == 0 {
                continue;
            }
            let name = connection
                .get_atom_name(info.name)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .map(|reply| String::from_utf8_lossy(&reply.name).into_owned())
                .unwrap_or_else(|| format!("GNOME-{index}"));
            monitors.push(Monitor {
                name,
                x: info.x,
                y: info.y,
                width: info.width as u32,
                height: info.height as u32,
            });
        }
        if !monitors.is_empty() {
            return monitors;
        }
    }
    vec![Monitor { name: "GNOME-0".into(), x: 0, y: 0, width: width as u32, height: height as u32 }]
}
