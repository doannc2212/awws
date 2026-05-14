mod cache;
mod config;
mod daemon;
mod history;
mod ipc;
mod setter;
mod source;
mod trigger;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing_subscriber::{EnvFilter, fmt};

#[derive(Debug, Parser)]
#[command(name = "awws", version, about = "awws wallpaper daemon and CLI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    #[arg(long, help = "Path to config.toml")]
    config: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Subcommand)]
enum Command {
    /// Start the wallpaper daemon in the foreground
    Daemon,
    /// Switch to the next wallpaper
    Next,
    /// Go back to the previous wallpaper
    Prev,
    /// Pause automatic wallpaper rotation
    Pause,
    /// Resume automatic wallpaper rotation
    Resume,
    /// Show current daemon status and wallpaper info
    Status,
    /// Reload configuration without restarting the daemon
    Reload,
}

#[tokio::main]
async fn main() -> Result<()> {
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let config_path = cli.config.unwrap_or_else(config::default_config_path);

    let Some(command) = cli.command else {
        use clap::CommandFactory;
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };

    match command {
        Command::Daemon => daemon::run(config_path).await,
        command => {
            let request = match command {
                Command::Next => ipc::ClientRequest::simple("next"),
                Command::Prev => ipc::ClientRequest::simple("prev"),
                Command::Pause => ipc::ClientRequest::simple("pause"),
                Command::Resume => ipc::ClientRequest::simple("resume"),
                Command::Status => ipc::ClientRequest::simple("status"),
                Command::Reload => ipc::ClientRequest::simple("reload"),
                Command::Daemon => unreachable!(),
            };

            match ipc::send_request(request).await {
                Ok(response) => {
                    println!("{}", serde_json::to_string_pretty(&response)?);
                    Ok(())
                }
                Err(err) if matches!(command, Command::Status) => {
                    tracing::info!(%err, "daemon not reachable; starting daemon");
                    daemon::run(config_path)
                        .await
                        .context("daemon exited unexpectedly")
                }
                Err(err) => Err(err).context("failed to talk to daemon"),
            }
        }
    }
}
