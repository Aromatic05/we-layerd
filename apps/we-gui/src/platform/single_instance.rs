//! Keep a single we-gui per user session.
//!
//! The first instance listens on a Unix socket in `$XDG_RUNTIME_DIR`. Closing
//! the window only hides we-gui to the tray, so launching it again from the
//! app grid used to start a second GUI (and a second owner of the daemon). A
//! later launch now connects to that socket, asks the running instance to show
//! its window, and exits.

use std::path::PathBuf;

/// Outcome of trying to become the primary instance.
pub enum Startup {
    /// This process is the only instance and should run the GUI.
    Primary,
    /// Another instance is running and was asked to show its window.
    AlreadyRunning,
}

#[cfg(unix)]
mod imp {
    use std::{
        io::{Read, Write},
        os::unix::net::{UnixListener, UnixStream},
        path::{Path, PathBuf},
        sync::Mutex,
        time::Duration,
    };

    use super::Startup;

    const SHOW: &[u8] = b"show\n";

    static LISTENER: Mutex<Option<(UnixListener, PathBuf)>> = Mutex::new(None);

    pub fn acquire(path: &Path) -> Startup {
        if notify_running(path) {
            return Startup::AlreadyRunning;
        }
        // Nobody answered: any socket file left behind is stale.
        let _ = std::fs::remove_file(path);
        match UnixListener::bind(path) {
            Ok(listener) => {
                if listener.set_nonblocking(true).is_ok() {
                    *LISTENER.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some((listener, path.to_path_buf()));
                }
                Startup::Primary
            }
            // Lost a race with another instance starting at the same moment.
            Err(_) if notify_running(path) => Startup::AlreadyRunning,
            // Never refuse to start just because the socket is unavailable.
            Err(_) => Startup::Primary,
        }
    }

    fn notify_running(path: &Path) -> bool {
        let Ok(mut stream) = UnixStream::connect(path) else {
            return false;
        };
        let _ = stream.set_write_timeout(Some(Duration::from_millis(500)));
        stream.write_all(SHOW).is_ok()
    }

    pub fn take_show_request() -> bool {
        let guard = LISTENER.lock().unwrap_or_else(|e| e.into_inner());
        let Some((listener, _)) = guard.as_ref() else {
            return false;
        };
        let mut requested = false;
        while let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.set_nonblocking(false);
            let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
            let mut buf = [0u8; 16];
            if let Ok(n) = stream.read(&mut buf) {
                requested |= buf[..n].starts_with(b"show");
            }
        }
        requested
    }

    pub fn release() {
        if let Some((_, path)) = LISTENER.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn socket_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    #[cfg(unix)]
    let name = format!("we-gui-{}.sock", user_id());
    #[cfg(not(unix))]
    let name = "we-gui.sock".to_string();
    dir.join(name)
}

#[cfg(unix)]
fn user_id() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::env::var_os("HOME")
        .and_then(|home| std::fs::metadata(home).ok())
        .map(|meta| meta.uid())
        .unwrap_or(0)
}

pub fn acquire() -> Startup {
    #[cfg(unix)]
    {
        imp::acquire(&socket_path())
    }
    #[cfg(not(unix))]
    {
        let _ = socket_path();
        Startup::Primary
    }
}

/// True when another launch asked this instance to show its window.
pub fn take_show_request() -> bool {
    #[cfg(unix)]
    {
        imp::take_show_request()
    }
    #[cfg(not(unix))]
    {
        false
    }
}

pub fn release() {
    #[cfg(unix)]
    imp::release();
}

#[cfg(all(test, unix))]
mod tests {
    use super::{imp, Startup};

    #[test]
    fn second_launch_asks_the_first_to_show_its_window() {
        let dir = std::env::temp_dir().join(format!("we-gui-single-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gui.sock");
        // A stale socket file from a crashed instance must not block startup.
        std::fs::write(&path, b"").unwrap();

        assert!(matches!(imp::acquire(&path), Startup::Primary));
        assert!(!imp::take_show_request());
        assert!(matches!(imp::acquire(&path), Startup::AlreadyRunning));
        assert!(imp::take_show_request());
        assert!(!imp::take_show_request());

        imp::release();
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(dir);
    }
}
