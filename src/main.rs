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
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

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
    /// List wallpaper history with the current position marked
    History,
}

#[tokio::main]
async fn main() -> Result<()> {
    let log_dir = dirs::state_dir()
        .unwrap_or_else(|| {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/state")
        })
        .join("awws");
    std::fs::create_dir_all(&log_dir).ok();

    let file_appender = tracing_appender::rolling::daily(&log_dir, "awws.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer())
        .with(fmt::layer().with_ansi(false).with_writer(non_blocking))
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
                Command::History => ipc::ClientRequest::simple("history"),
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
