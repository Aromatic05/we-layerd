mod app;
mod backend;
mod cli;
mod config;
mod ipc;
mod logging;
mod runtime;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command, ControlAction};
use ipc::ControlCommand;

fn main() -> Result<()> {
    logging::init();

    let cli = Cli::parse();
    match cli.command {
        Command::Run { config } => app::run(config.as_deref()),
        Command::Switch { config } => ipc::send_switch_config(&config),
        Command::Doctor { config } => app::doctor(config.as_deref()),
        Command::PrintConfig { config } => {
            let cfg = config::Config::load(config.as_deref())?;
            println!("{}", cfg.to_toml_pretty()?);
            Ok(())
        }
        Command::Outputs => {
            println!(
                "{}",
                serde_json::to_string(&backend::wayland_common::outputs::list_output_names()?)?
            );
            Ok(())
        }
        Command::Ctl { action } => match action {
            ControlAction::Stop => ipc::send_command(ControlCommand::Stop),
            ControlAction::Pause => ipc::send_command(ControlCommand::Pause),
            ControlAction::Resume => ipc::send_command(ControlCommand::Resume),
            ControlAction::Reload => ipc::send_command(ControlCommand::Reload),
            ControlAction::Status => {
                println!("{}", ipc::request_running_config()?);
                Ok(())
            }
        },
    }
}
