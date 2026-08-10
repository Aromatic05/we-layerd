use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "we-layerd", version, about = "Wallpaper Engine layer daemon")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run daemon with a configuration file
    Run {
        /// Path to TOML config file
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Reconfigure a running daemon with a new renderer-native config
    Switch {
        /// Path to TOML config file
        #[arg(long)]
        config: PathBuf,
    },
    /// Print environment diagnostics
    Doctor {
        /// Path to TOML config file
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Print the effective config as TOML
    PrintConfig {
        /// Path to TOML config file
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Print compositor output names as JSON
    Outputs,
    /// Send control command to a running daemon
    Ctl {
        #[arg(value_enum)]
        action: ControlAction,
    },
    /// Control daemon-managed playlists
    Playlist {
        #[command(subcommand)]
        action: PlaylistAction,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ControlAction {
    Stop,
    Pause,
    Resume,
    Reload,
    Status,
}

#[derive(Debug, Clone, Subcommand)]
pub enum PlaylistAction {
    /// Start a named playlist
    Play { name: String },
    /// Advance to the next playable entry
    Next,
    /// Return to the previous playable entry
    Previous,
    /// Stop playlist progression while leaving the current wallpaper running
    Stop,
}
