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
    Doctor,
    /// Print the effective config as TOML
    PrintConfig {
        /// Path to TOML config file
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Send control command to a running daemon
    Ctl {
        #[arg(value_enum)]
        action: ControlAction,
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
