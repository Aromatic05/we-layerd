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
        fs::{File, OpenOptions},
        io::{Read, Write},
        os::{
            fd::AsRawFd,
            unix::net::{UnixListener, UnixStream},
        },
        path::{Path, PathBuf},
        sync::Mutex,
        time::Duration,
    };

    use super::Startup;

    const SHOW: &[u8] = b"show\n";

    struct Instance {
        listener: UnixListener,
        path: PathBuf,
        /// Held for the process lifetime; flock is released by the kernel on exit, so a crashed
        /// instance never leaves a stale lock behind.
        _lock: Option<File>,
    }

    static LISTENER: Mutex<Option<Instance>> = Mutex::new(None);

    enum Lock {
        Held(File),
        /// Another live instance holds the per-session lock.
        Busy,
        /// The lock file cannot be created; fall back to the socket probe alone.
        Unavailable,
    }

    pub fn acquire(path: &Path) -> Startup {
        // Serialise the probe/remove/bind sequence. Without the lock, two launches can both find
        // the socket dead, and the slower one deletes the socket the faster one just bound, so
        // both run as primary.
        let lock = match lock_instance(path) {
            Lock::Busy => {
                let _ = notify_running(path);
                return Startup::AlreadyRunning;
            }
            Lock::Unavailable => None,
            Lock::Held(file) => Some(file),
        };
        if notify_running(path) {
            return Startup::AlreadyRunning;
        }
        // Nobody answered: any socket file left behind is stale.
        let _ = std::fs::remove_file(path);
        match UnixListener::bind(path) {
            Ok(listener) => {
                if listener.set_nonblocking(true).is_ok() {
                    *LISTENER.lock().unwrap_or_else(|e| e.into_inner()) =
                        Some(Instance { listener, path: path.to_path_buf(), _lock: lock });
                }
                Startup::Primary
            }
            // Lost a race with another instance starting at the same moment.
            Err(_) if notify_running(path) => Startup::AlreadyRunning,
            // Never refuse to start just because the socket is unavailable.
            Err(_) => Startup::Primary,
        }
    }

    /// Take the per-session instance lock.
    fn lock_instance(path: &Path) -> Lock {
        let mut lock_path = path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let Ok(file) = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(PathBuf::from(lock_path))
        else {
            return Lock::Unavailable;
        };
        // SAFETY: the fd is open for the life of `file`, and flock only inspects that fd.
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
        if locked {
            Lock::Held(file)
        } else {
            Lock::Busy
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
        let Some(instance) = guard.as_ref() else {
            return false;
        };
        let mut requested = false;
        while let Ok((mut stream, _)) = instance.listener.accept() {
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
        if let Some(instance) = LISTENER.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = std::fs::remove_file(&instance.path);
        }
    }
}

fn socket_path() -> PathBuf {
    #[cfg(unix)]
    {
        socket_path_for_session(&session_id())
    }
    #[cfg(not(unix))]
    {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|dir| dir.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        dir.join("we-gui.sock")
    }
}

#[cfg(unix)]
fn socket_path_for_session(session: &str) -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_dir())
        .unwrap_or_else(std::env::temp_dir);
    dir.join(format!("we-gui-{}-{}.sock", user_id(), session))
}

/// Distinguishes two logins of the same user: `$XDG_RUNTIME_DIR` is shared between them, so one
/// session's GUI must not answer another session's launch or block its socket.
#[cfg(unix)]
fn session_id() -> String {
    let raw = std::env::var_os("XDG_SESSION_ID")
        .or_else(|| std::env::var_os("WAYLAND_DISPLAY"))
        .or_else(|| std::env::var_os("DISPLAY"));
    let Some(raw) = raw.and_then(|value| value.into_string().ok()) else {
        return "0".to_string();
    };
    // Session ids and display names are display-server strings; keep only path-safe characters.
    raw.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).collect()
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

    #[test]
    fn a_concurrent_launch_cannot_become_a_second_primary() {
        let dir = std::env::temp_dir().join(format!("we-gui-race-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gui.sock");

        // Hold the instance lock the way a starting process would, without listening yet: this is
        // the window where both launches used to see a dead socket and both became primary.
        let mut lock_path = path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let held = std::fs::File::create(&lock_path).unwrap();
        assert_eq!(
            // SAFETY: the fd stays open for the whole test.
            unsafe { libc::flock(std::os::fd::AsRawFd::as_raw_fd(&held), libc::LOCK_EX) },
            0
        );

        assert!(matches!(imp::acquire(&path), Startup::AlreadyRunning));
        assert!(!path.exists(), "the loser must not touch the winner's socket");

        drop(held);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn separate_desktop_sessions_get_their_own_instance() {
        // `socket_path` includes the session, so two logins of one user do not fight over a socket
        // in the shared `$XDG_RUNTIME_DIR`.
        assert_ne!(
            super::socket_path_for_session("1"),
            super::socket_path_for_session("2"),
            "two sessions must not share a socket"
        );
    }
}
