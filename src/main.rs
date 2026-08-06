//! flingdlna CLI - Unified DLNA server and controller
//!
//! Supports two modes:
//! 1. Direct mode (default) - each command is standalone
//! 2. Daemon mode - persistent background process with Unix socket IPC

mod cli;
mod daemon_cli;
mod direct;
mod tui;
mod utils;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    let level = if cli.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .with_target(false)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    // Daemon-specific commands (always direct, don't use --daemon flag)
    match &cli.command {
        Commands::StartDaemon {
            foreground,
            port,
            serve_dir,
            serve_port,
            serve_name,
            watch,
        } => {
            return daemon_cli::start_daemon(
                cli.socket.clone(),
                *foreground,
                *port,
                serve_dir.clone(),
                *serve_port,
                serve_name.clone(),
                *watch,
            )
            .await;
        }
        Commands::StopDaemon => {
            return daemon_cli::stop_daemon(&cli.socket).await;
        }
        Commands::PingDaemon => {
            return daemon_cli::ping_daemon(&cli.socket).await;
        }
        Commands::Tui => {
            return tui::run_tui(&cli.socket).await;
        }
        _ => {}
    }

    // If --daemon flag is set, route commands through daemon
    if cli.daemon {
        return daemon_cli::run_via_daemon(&cli.socket, cli.command).await;
    }

    // Direct mode (standalone commands)
    match cli.command {
        Commands::Serve {
            dir,
            port,
            name,
            watch,
        } => {
            direct::run_server(dir, port, name, watch).await?;
        }
        Commands::List { timeout } => {
            direct::list_renderers(timeout).await?;
        }
        Commands::Push { file, device, port } => {
            direct::push_file(file, device, port).await?;
        }
        Commands::Play { url, device } => {
            direct::play_url(url, device).await?;
        }
        Commands::Pause { device } => {
            direct::pause_device(device).await?;
        }
        Commands::Resume { device } => {
            direct::resume_device(device).await?;
        }
        Commands::Stop { device } => {
            direct::stop_device(device).await?;
        }
        Commands::Wake { device, mac } => {
            direct::wake_device(&device, mac.as_deref()).await?;
        }
        Commands::Seek { position, device } => {
            direct::seek_device(position, device).await?;
        }
        Commands::Status { device } => {
            direct::show_status(device).await?;
        }
        Commands::Volume { level, device } => {
            direct::volume_control(device, level).await?;
        }
        Commands::Mute { state, device } => {
            direct::mute_control(device, state).await?;
        }
        // Queue, position, event, and media directory commands require daemon mode
        Commands::QueueAdd { .. }
        | Commands::QueueRemove { .. }
        | Commands::QueueClear { .. }
        | Commands::QueueList { .. }
        | Commands::Next { .. }
        | Commands::Prev { .. }
        | Commands::Shuffle { .. }
        | Commands::Repeat { .. }
        | Commands::SavePosition { .. }
        | Commands::ListPositions
        | Commands::ResumeFrom { .. }
        | Commands::ClearPosition { .. }
        | Commands::Subscribe { .. }
        | Commands::Unsubscribe { .. }
        | Commands::PollEvents { .. }
        | Commands::ListMedia
        | Commands::SetMediaDir { .. }
        | Commands::AddMediaDir { .. }
        | Commands::RemoveMediaDir { .. }
        | Commands::ClearMedia
        | Commands::AutoRefresh { .. } => {
            anyhow::bail!(
                "This command requires daemon mode. Start the daemon first:\n\
                 flingdlna daemon --foreground\n\
                 Then use --daemon flag: flingdlna --daemon <command>"
            );
        }
        // Already handled above
        Commands::StartDaemon { .. }
        | Commands::StopDaemon
        | Commands::PingDaemon
        | Commands::Tui => {
            unreachable!()
        }
    }

    Ok(())
}
