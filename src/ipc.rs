use std::{
    fs,
    io::{self, Read, Write},
    net::Shutdown,
    os::fd::AsRawFd,
    os::unix::net::{SocketAddr, UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::mpsc::Sender,
    thread,
};

#[cfg(target_os = "linux")]
use std::os::linux::net::SocketAddrExt;

use anyhow::{anyhow, Context, Result};
use we_core::config::OutputBinding;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlCommand {
    Stop,
    Pause,
    Resume,
    Reload,
    Reconfigure,
}

impl ControlCommand {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stop => "stop",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Reload => "reload",
            Self::Reconfigure => "reconfigure",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "stop" => Some(Self::Stop),
            "pause" => Some(Self::Pause),
            "resume" => Some(Self::Resume),
            "reload" => Some(Self::Reload),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaylistCommand {
    Play(String),
    Next,
    Previous,
    Stop,
    Output { output: String, action: OutputPlaylistAction },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputPlaylistAction {
    Play(String),
    Next,
    Previous,
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileCommand {
    Apply(String),
}

impl ProfileCommand {
    fn request(&self) -> String {
        match self {
            Self::Apply(name) => format!("profile apply {name}"),
        }
    }
}

#[derive(Debug)]
pub struct OutputPlaylistRequest {
    pub output: String,
    pub action: OutputPlaylistAction,
    pub reply: std::sync::mpsc::Sender<std::result::Result<Option<OutputBinding>, String>>,
}

impl PlaylistCommand {
    fn request(&self) -> String {
        match self {
            Self::Play(name) => format!("playlist play {name}"),
            Self::Next => "playlist next".to_string(),
            Self::Previous => "playlist previous".to_string(),
            Self::Stop => "playlist stop".to_string(),
            Self::Output { output, action } => match action {
                OutputPlaylistAction::Play(name) => {
                    format!("playlist output {output} play {name}")
                }
                OutputPlaylistAction::Next => format!("playlist output {output} next"),
                OutputPlaylistAction::Previous => {
                    format!("playlist output {output} previous")
                }
                OutputPlaylistAction::Stop => format!("playlist output {output} stop"),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeLoopExit {
    Stop,
    RestartCurrent,
    Reconfigure,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ControlRequest {
    Command(ControlCommand),
    Status,
    SwitchConfig(PathBuf),
    Playlist(PlaylistCommand),
    Profile(ProfileCommand),
}

impl ControlRequest {
    fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim();
        if let Some(rest) = trimmed.strip_prefix("switch-config ") {
            let path = rest.trim();
            if path.is_empty() {
                return None;
            }
            return Some(Self::SwitchConfig(PathBuf::from(path)));
        }
        if let Some(name) = trimmed.strip_prefix("playlist play ") {
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            return Some(Self::Playlist(PlaylistCommand::Play(name.to_string())));
        }
        if let Some(name) = trimmed.strip_prefix("profile apply ") {
            let name = name.trim();
            if name.is_empty() {
                return None;
            }
            return Some(Self::Profile(ProfileCommand::Apply(name.to_string())));
        }
        if let Some(rest) = trimmed.strip_prefix("playlist output ") {
            let (output, action) = rest.split_once(' ')?;
            let output = output.trim();
            let action = action.trim();
            if output.is_empty() || action.is_empty() {
                return None;
            }
            let action = if let Some(name) = action.strip_prefix("play ") {
                let name = name.trim();
                if name.is_empty() {
                    return None;
                }
                OutputPlaylistAction::Play(name.to_string())
            } else {
                match action.to_ascii_lowercase().as_str() {
                    "next" => OutputPlaylistAction::Next,
                    "previous" | "prev" => OutputPlaylistAction::Previous,
                    "stop" => OutputPlaylistAction::Stop,
                    _ => return None,
                }
            };
            return Some(Self::Playlist(PlaylistCommand::Output {
                output: output.to_string(),
                action,
            }));
        }
        match trimmed.to_ascii_lowercase().as_str() {
            "playlist next" => return Some(Self::Playlist(PlaylistCommand::Next)),
            "playlist previous" | "playlist prev" => {
                return Some(Self::Playlist(PlaylistCommand::Previous))
            }
            "playlist stop" => return Some(Self::Playlist(PlaylistCommand::Stop)),
            _ => {}
        }
        let normalized = trimmed.to_ascii_lowercase();
        if normalized == "status" {
            return Some(Self::Status);
        }
        ControlCommand::parse(&normalized).map(Self::Command)
    }
}

pub struct ControlServer {
    socket_path: Option<PathBuf>,
    _instance_lock: fs::File,
}

impl ControlServer {
    pub fn start<F, H, S, P, R>(
        tx: Sender<ControlCommand>,
        status_provider: F,
        command_handler: H,
        switch_config_handler: S,
        playlist_handler: P,
        profile_handler: R,
    ) -> Result<Self>
    where
        F: Fn() -> String + Send + Sync + 'static,
        H: Fn(ControlCommand) -> Result<bool> + Send + Sync + 'static,
        S: Fn(&Path) -> Result<()> + Send + Sync + 'static,
        P: Fn(PlaylistCommand) -> Result<()> + Send + Sync + 'static,
        R: Fn(ProfileCommand) -> Result<()> + Send + Sync + 'static,
    {
        let instance_lock = acquire_instance_lock()?;
        let endpoint = default_endpoint()?;
        let listener = bind_listener(&endpoint)?;
        let socket_path = endpoint.socket_path();
        let status_provider = std::sync::Arc::new(status_provider);
        let command_handler = std::sync::Arc::new(command_handler);
        let switch_config_handler = std::sync::Arc::new(switch_config_handler);
        let playlist_handler = std::sync::Arc::new(playlist_handler);
        let profile_handler = std::sync::Arc::new(profile_handler);
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else {
                    continue;
                };
                let mut buf = String::new();
                if stream.read_to_string(&mut buf).is_err() {
                    continue;
                }
                let Some(request) = ControlRequest::parse(&buf) else {
                    let _ = stream.write_all(b"ERR unknown command\n");
                    continue;
                };
                match request {
                    ControlRequest::Status => {
                        let status = status_provider();
                        let _ = stream.write_all(status.as_bytes());
                    }
                    ControlRequest::SwitchConfig(path) => match switch_config_handler(&path) {
                        Ok(()) => {
                            let _ = stream.write_all(b"OK\n");
                        }
                        Err(err) => {
                            let _ = stream.write_all(format!("ERR {err}\n").as_bytes());
                        }
                    },
                    ControlRequest::Playlist(command) => match playlist_handler(command) {
                        Ok(()) => {
                            let _ = stream.write_all(b"OK\n");
                        }
                        Err(err) => {
                            let _ = stream.write_all(format!("ERR {err}\n").as_bytes());
                        }
                    },
                    ControlRequest::Profile(command) => match profile_handler(command) {
                        Ok(()) => {
                            let _ = stream.write_all(b"OK\n");
                        }
                        Err(err) => {
                            let _ = stream.write_all(format!("ERR {err}\n").as_bytes());
                        }
                    },
                    ControlRequest::Command(cmd) => {
                        match command_handler(cmd) {
                            Ok(true) => {
                                let _ = stream.write_all(b"OK\n");
                                continue;
                            }
                            Ok(false) => {}
                            Err(err) => {
                                let _ = stream.write_all(format!("ERR {err}\n").as_bytes());
                                continue;
                            }
                        }
                        if tx.send(cmd).is_ok() {
                            let _ = stream.write_all(b"OK\n");
                        } else {
                            let _ = stream.write_all(b"ERR daemon not running\n");
                        }
                    }
                }
            }
        });

        Ok(Self { socket_path, _instance_lock: instance_lock })
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        if let Some(socket_path) = &self.socket_path {
            let _ = fs::remove_file(socket_path);
        }
    }
}

pub fn send_command(command: ControlCommand) -> Result<()> {
    let response = send_request(command.as_str())?;
    if response.trim_start().starts_with("ERR") {
        return Err(anyhow!(response.trim().to_string()));
    }
    Ok(())
}

pub fn request_running_config() -> Result<String> {
    let response = send_request("status")?;
    if response.trim_start().starts_with("ERR") {
        return Err(anyhow!(response.trim().to_string()));
    }
    Ok(response)
}

pub fn send_switch_config(config_path: &Path) -> Result<()> {
    let response = send_request(&format!("switch-config {}", config_path.display()))?;
    if response.trim_start().starts_with("ERR") {
        return Err(anyhow!(response.trim().to_string()));
    }
    Ok(())
}

pub fn send_playlist_command(command: &PlaylistCommand) -> Result<()> {
    let response = send_request(&command.request())?;
    if response.trim_start().starts_with("ERR") {
        return Err(anyhow!(response.trim().to_string()));
    }
    Ok(())
}

pub fn send_profile_command(command: &ProfileCommand) -> Result<()> {
    let response = send_request(&command.request())?;
    if response.trim_start().starts_with("ERR") {
        return Err(anyhow!(response.trim().to_string()));
    }
    Ok(())
}

fn send_request(request: &str) -> Result<String> {
    let mut last_error: Option<anyhow::Error> = None;
    for endpoint in control_endpoints() {
        match connect_stream(&endpoint) {
            Ok(mut stream) => {
                stream
                    .write_all(request.as_bytes())
                    .with_context(|| format!("failed to send IPC request '{request}'"))?;
                let _ = stream.shutdown(Shutdown::Write);
                let mut response = String::new();
                stream.read_to_string(&mut response).context("failed to read IPC response")?;
                return Ok(response);
            }
            Err(err) => {
                last_error = Some(err);
            }
        }
    }

    Err(anyhow!("failed to reach we-layerd control endpoint (daemon may not be running)"))
        .context(last_error.unwrap_or_else(|| anyhow!("no endpoint available")))
}

#[derive(Debug, Clone)]
enum Endpoint {
    Path(PathBuf),
    #[cfg(target_os = "linux")]
    Abstract(Vec<u8>),
}

impl Endpoint {
    fn socket_path(&self) -> Option<PathBuf> {
        match self {
            Self::Path(path) => Some(path.clone()),
            #[cfg(target_os = "linux")]
            Self::Abstract(_) => None,
        }
    }
}

fn default_endpoint() -> Result<Endpoint> {
    #[cfg(target_os = "linux")]
    {
        Ok(Endpoint::Abstract(abstract_socket_name()))
    }

    #[cfg(not(target_os = "linux"))]
    {
        Ok(Endpoint::Path(default_socket_path()?))
    }
}

fn control_endpoints() -> Vec<Endpoint> {
    #[cfg(target_os = "linux")]
    {
        vec![
            Endpoint::Abstract(abstract_socket_name()),
            Endpoint::Path(
                default_socket_path()
                    .unwrap_or_else(|_| PathBuf::from("/tmp/we-layerd-control.sock")),
            ),
        ]
    }

    #[cfg(not(target_os = "linux"))]
    {
        vec![Endpoint::Path(
            default_socket_path().unwrap_or_else(|_| PathBuf::from("/tmp/we-layerd-control.sock")),
        )]
    }
}

fn bind_listener(endpoint: &Endpoint) -> Result<UnixListener> {
    match endpoint {
        Endpoint::Path(socket_path) => bind_file_listener(socket_path),
        #[cfg(target_os = "linux")]
        Endpoint::Abstract(name) => {
            let addr = SocketAddr::from_abstract_name(name)
                .context("failed to build abstract IPC socket")?;
            UnixListener::bind_addr(&addr)
                .context("failed to bind abstract IPC socket for we-layerd")
        }
    }
}

fn bind_file_listener(socket_path: &Path) -> Result<UnixListener> {
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    if socket_path.exists() {
        if UnixStream::connect(socket_path).is_ok() {
            return Err(anyhow!("we-layerd is already running"));
        }
        let _ = fs::remove_file(socket_path);
    }

    UnixListener::bind(socket_path)
        .with_context(|| format!("failed to bind IPC socket {}", socket_path.display()))
}

fn connect_stream(endpoint: &Endpoint) -> Result<UnixStream> {
    match endpoint {
        Endpoint::Path(path) => UnixStream::connect(path)
            .with_context(|| format!("failed to connect IPC socket {}", path.display())),
        #[cfg(target_os = "linux")]
        Endpoint::Abstract(name) => {
            let addr = SocketAddr::from_abstract_name(name)
                .context("failed to build abstract IPC socket")?;
            UnixStream::connect_addr(&addr).context("failed to connect abstract IPC socket")
        }
    }
}

fn default_socket_path() -> Result<PathBuf> {
    Ok(ipc_runtime_dir()?.join("control.sock"))
}

fn instance_lock_path() -> Result<PathBuf> {
    Ok(ipc_runtime_dir()?.join("instance.lock"))
}

fn ipc_runtime_dir() -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    if let Some(runtime_dir) = std::env::var_os("XDG_RUNTIME_DIR") {
        return Ok(PathBuf::from(runtime_dir).join("we-layerd"));
    }

    let home = std::env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".config/we-layerd"))
}

fn acquire_instance_lock() -> Result<fs::File> {
    let lock_path = instance_lock_path()?;
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("failed to open lock file {}", lock_path.display()))?;

    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
            return Err(anyhow!("we-layerd is already running"));
        }
        return Err(err).with_context(|| format!("failed to lock {}", lock_path.display()));
    }

    Ok(file)
}

#[cfg(target_os = "linux")]
fn abstract_socket_name() -> Vec<u8> {
    let uid = unsafe { libc::geteuid() };
    format!("we-layerd.control.{uid}").into_bytes()
}

#[cfg(test)]
mod tests {
    use super::{ControlRequest, OutputPlaylistAction, PlaylistCommand, ProfileCommand};

    #[test]
    fn playlist_ipc_preserves_named_playlists_and_rejects_empty_names() {
        assert_eq!(
            ControlRequest::parse("playlist play Focus Session"),
            Some(ControlRequest::Playlist(PlaylistCommand::Play("Focus Session".to_string())))
        );
        assert_eq!(ControlRequest::parse("playlist play    "), None);
        assert_eq!(
            ControlRequest::parse("playlist next"),
            Some(ControlRequest::Playlist(PlaylistCommand::Next))
        );
        assert_eq!(
            ControlRequest::parse("playlist previous"),
            Some(ControlRequest::Playlist(PlaylistCommand::Previous))
        );
        assert_eq!(
            ControlRequest::parse("playlist stop"),
            Some(ControlRequest::Playlist(PlaylistCommand::Stop))
        );
    }

    #[test]
    fn playlist_ipc_routes_output_scoped_actions_without_changing_global_commands() {
        assert_eq!(
            ControlRequest::parse("playlist output DP-1 play Focus Session"),
            Some(ControlRequest::Playlist(PlaylistCommand::Output {
                output: "DP-1".to_string(),
                action: OutputPlaylistAction::Play("Focus Session".to_string()),
            }))
        );
        assert_eq!(
            ControlRequest::parse("playlist output HDMI-A-1 next"),
            Some(ControlRequest::Playlist(PlaylistCommand::Output {
                output: "HDMI-A-1".to_string(),
                action: OutputPlaylistAction::Next,
            }))
        );
        assert_eq!(
            ControlRequest::parse("playlist output HDMI-A-1 stop"),
            Some(ControlRequest::Playlist(PlaylistCommand::Output {
                output: "HDMI-A-1".to_string(),
                action: OutputPlaylistAction::Stop,
            }))
        );
        assert_eq!(ControlRequest::parse("playlist output DP-1 play    "), None);
        assert_eq!(ControlRequest::parse("playlist output    next"), None);
        assert_eq!(
            ControlRequest::parse("playlist next"),
            Some(ControlRequest::Playlist(PlaylistCommand::Next))
        );
    }

    #[test]
    fn profile_ipc_preserves_named_profile_and_rejects_empty_names() {
        assert_eq!(
            ControlRequest::parse("profile apply Desk Setup"),
            Some(ControlRequest::Profile(ProfileCommand::Apply("Desk Setup".to_string())))
        );
        assert_eq!(ControlRequest::parse("profile apply    "), None);
    }
}
