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
        time::{Duration, Instant},
    };

    use super::Startup;

    const SHOW: &[u8] = b"show\n";
    /// How long a losing launch waits for the winner to finish binding its socket.
    const LOCK_WAIT: Duration = Duration::from_millis(500);

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
                // The winner holds the lock before it binds its socket, so retry briefly instead
                // of dropping the show-window request on the floor.
                let deadline = Instant::now() + LOCK_WAIT;
                while !notify_running(path) && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(25));
                }
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
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    let xdg_session = std::env::var("XDG_SESSION_ID").ok();
    let display = std::env::var("DISPLAY").ok();
    choose_session_id(wayland.as_deref(), xdg_session.as_deref(), display.as_deref())
}

/// `$XDG_RUNTIME_DIR` is shared between two logins of the same user, and the systemd user
/// manager starts we-gui without `XDG_SESSION_ID`. The compositor's socket name is the one
/// identifier both launches agree on, so prefer it over the logind id.
#[cfg(unix)]
fn choose_session_id(wayland: Option<&str>, xdg_session: Option<&str>, display: Option<&str>) -> String {
    let raw = [wayland, xdg_session, display]
        .into_iter()
        .flatten()
        .find(|value| !value.is_empty());
    let Some(raw) = raw else {
        return "0".to_string();
    };
    // Session ids and display names are display-server strings; keep only path-safe characters.
    let sanitized: String =
        raw.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')).collect();
    if sanitized.is_empty() {
        "0".to_string()
    } else {
        sanitized
    }
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

    #[test]
    fn session_id_prefers_the_compositor_socket_over_logind() {
        // The systemd user manager has no `XDG_SESSION_ID`; a desktop launch does. Both must
        // still land on the same socket, which only happens if `WAYLAND_DISPLAY` wins.
        assert_eq!(
            super::choose_session_id(Some("wayland-0"), Some("3"), Some(":0")),
            "wayland-0"
        );
        assert_eq!(super::choose_session_id(Some("wayland-0"), None, None), "wayland-0");
        // Without a compositor socket the logind id still separates two sessions.
        assert_eq!(super::choose_session_id(None, Some("3"), Some(":0")), "3");
        assert_eq!(super::choose_session_id(None, None, Some(":1")), "1");
        assert_eq!(super::choose_session_id(None, None, None), "0");
        // A name made only of separators must not produce an empty socket suffix.
        assert_eq!(super::choose_session_id(Some("/"), None, None), "0");
    }
}
