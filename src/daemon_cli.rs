//! Daemon-related CLI commands

use crate::cli::Commands;
use crate::utils::{format_duration, get_hostname, parse_duration, shutdown_signal};
use anyhow::Result;
use dlna_combo::{
    Command as DaemonCommand, Daemon, DaemonClient, DaemonConfig, Response, ResponseData,
    ServerConfig,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Start the daemon process
pub async fn start_daemon(
    socket_path: PathBuf,
    foreground: bool,
    _port: u16, // TODO: use for dynamic streaming port allocation
    serve_dirs: Option<Vec<PathBuf>>,
    serve_port: u16,
    serve_name: Option<String>,
    watch: bool,
) -> Result<()> {
    // Check if already running
    if dlna_combo::is_daemon_running(&socket_path) {
        anyhow::bail!("Daemon already running at {socket_path:?}");
    }

    let mut config = DaemonConfig::new().with_socket_path(socket_path.clone());

    // If serve directories are specified, add server config
    if let Some(dirs) = serve_dirs {
        let mut server_config = match serve_name {
            Some(name) => ServerConfig::new(name),
            None => ServerConfig::new(format!("FlingDLNA ({})", get_hostname())),
        }
        .with_port(serve_port)
        .with_directories(dirs);

        if watch {
            server_config = server_config.with_watch(true);
        }

        config = config.with_server(server_config);
    }

    if foreground {
        println!("Starting daemon in foreground...");
        let daemon = Daemon::new(config).await?;
        daemon.run().await?;
    } else {
        // For now, just run in foreground
        // TODO: actual daemonization with fork() on Unix
        println!("Starting daemon (foreground mode)...");
        println!("Socket: {socket_path:?}");
        let daemon = Daemon::new(config).await?;

        // Handle shutdown signal
        let shutdown_daemon = daemon.shutdown_tx.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            let _ = shutdown_daemon.send(());
        });

        daemon.run().await?;
    }

    Ok(())
}

/// Stop the running daemon
pub async fn stop_daemon(socket_path: &Path) -> Result<()> {
    let client = DaemonClient::new(socket_path.to_path_buf());

    if !client.is_daemon_running() {
        println!("Daemon is not running");
        return Ok(());
    }

    match client.shutdown().await {
        Ok(()) => println!("Daemon stopped"),
        Err(e) => anyhow::bail!("Failed to stop daemon: {e}"),
    }

    Ok(())
}

/// Check daemon status
pub async fn ping_daemon(socket_path: &Path) -> Result<()> {
    let client = DaemonClient::new(socket_path.to_path_buf());

    if !client.is_daemon_running() {
        println!("Daemon is not running (socket does not exist)");
        return Ok(());
    }

    match client.send(&DaemonCommand::Info).await {
        Ok(Response::Ok {
            data:
                Some(ResponseData::DaemonInfo {
                    version,
                    uptime_secs,
                    active_streams,
                }),
        }) => {
            println!("Daemon running:");
            println!("  Version: {version}");
            println!("  Uptime: {uptime_secs}s");
            println!("  Active streams: {active_streams}");
        }
        Ok(Response::Error { message, .. }) => {
            anyhow::bail!("Daemon error: {message}");
        }
        Err(e) => {
            anyhow::bail!("Cannot connect to daemon: {e}");
        }
        _ => {
            println!("Daemon running (unexpected response)");
        }
    }

    Ok(())
}

/// Route a command through the daemon
pub async fn run_via_daemon(socket_path: &Path, command: Commands) -> Result<()> {
    let client = DaemonClient::new(socket_path.to_path_buf());

    if !client.is_daemon_running() {
        anyhow::bail!("Daemon is not running. Start it with: flingdlna daemon --foreground");
    }

    let cmd = match command {
        Commands::List { timeout } => DaemonCommand::List {
            timeout_secs: timeout,
        },
        Commands::Push { file, device, port } => DaemonCommand::Push { file, device, port },
        Commands::Play { url, device } => DaemonCommand::PlayUrl { url, device },
        Commands::Pause { device } => DaemonCommand::Pause { device },
        Commands::Resume { device } => DaemonCommand::Resume { device },
        Commands::Stop { device } => DaemonCommand::Stop { device },
        Commands::Wake { device, mac } => DaemonCommand::Wake { device, mac },
        Commands::Seek { position, device } => {
            let duration = parse_duration(&position)?;
            DaemonCommand::Seek {
                device,
                position_secs: duration.as_secs(),
            }
        }
        Commands::Status { device } => DaemonCommand::Status { device },
        Commands::Volume { level, device } => {
            if let Some(vol) = level {
                DaemonCommand::SetVolume {
                    device,
                    volume: vol,
                }
            } else {
                DaemonCommand::GetVolume { device }
            }
        }
        Commands::Mute { state, device } => {
            let mute = state.map(|s| s.to_lowercase() == "on" || s == "1" || s == "true");
            DaemonCommand::Mute { device, mute }
        }
        // Queue commands
        Commands::QueueAdd {
            source,
            title,
            content_type,
            subtitle_url,
            device,
            position,
        } => DaemonCommand::QueueAdd {
            source,
            title,
            content_type,
            subtitle_url,
            device,
            position,
        },
        Commands::QueueRemove { index, device } => DaemonCommand::QueueRemove { device, index },
        Commands::QueueClear { device } => DaemonCommand::QueueClear { device },
        Commands::QueueList { device } => DaemonCommand::QueueList { device },
        Commands::Next { device } => DaemonCommand::Next { device },
        Commands::Prev { device } => DaemonCommand::Prev { device },
        Commands::Shuffle { state, device } => {
            let enabled = state.to_lowercase() == "on" || state == "1" || state == "true";
            DaemonCommand::Shuffle { device, enabled }
        }
        Commands::Repeat { mode, device } => DaemonCommand::Repeat { device, mode },
        // Position commands
        Commands::SavePosition { device } => DaemonCommand::SavePosition { device },
        Commands::ListPositions => DaemonCommand::ListPositions,
        Commands::ResumeFrom { url, device } => DaemonCommand::ResumeFrom { device, uri: url },
        Commands::ClearPosition { url } => DaemonCommand::ClearPosition { uri: url },
        // Event subscription commands
        Commands::Subscribe { device } => DaemonCommand::Subscribe { device },
        Commands::Unsubscribe { device } => DaemonCommand::Unsubscribe { device },
        Commands::PollEvents { device } => DaemonCommand::PollEvents { device },
        // Media server directory commands
        Commands::ListMedia => DaemonCommand::ListMedia,
        Commands::SetMediaDir { path } => DaemonCommand::SetMediaDir { path },
        Commands::AddMediaDir { path } => DaemonCommand::AddMediaDir { path },
        Commands::RemoveMediaDir { path } => DaemonCommand::RemoveMediaDir { path },
        Commands::ClearMedia => DaemonCommand::ClearMedia,
        Commands::AutoRefresh { state } => {
            if let Some(s) = state {
                let enabled = s.to_lowercase() == "on" || s == "1" || s == "true";
                DaemonCommand::SetAutoRefresh { enabled }
            } else {
                DaemonCommand::GetAutoRefresh
            }
        }
        _ => unreachable!(),
    };

    match client.send(&cmd).await? {
        Response::Ok { data } => {
            print_response_data(data);
        }
        Response::Error { message, .. } => {
            anyhow::bail!("{message}");
        }
    }

    Ok(())
}

/// Print response data in human-readable format
pub fn print_response_data(data: Option<ResponseData>) {
    match data {
        Some(ResponseData::Renderers { renderers }) => {
            if renderers.is_empty() {
                println!("No renderers found.");
            } else {
                println!("Found {} renderer(s):\n", renderers.len());
                for r in renderers {
                    println!("  {}", r.name);
                    println!("    Location: {}", r.location);
                    if let Some(m) = r.manufacturer {
                        println!("    Manufacturer: {m}");
                    }
                    if let Some(m) = r.model {
                        println!("    Model: {m}");
                    }
                    println!();
                }
            }
        }
        Some(ResponseData::Playback {
            state,
            position_secs,
            duration_secs,
            current_uri,
        }) => {
            println!("Playback status:");
            println!("  State: {state}");
            println!(
                "  Position: {} / {}",
                format_duration(Duration::from_secs(position_secs)),
                format_duration(Duration::from_secs(duration_secs))
            );
            if let Some(uri) = current_uri {
                println!("  Playing: {uri}");
            }
        }
        Some(ResponseData::Streaming { device, file, url }) => {
            println!("Streaming started:");
            println!("  Device: {device}");
            println!("  File: {file}");
            println!("  URL: {url}");
        }
        Some(ResponseData::DaemonInfo {
            version,
            uptime_secs,
            active_streams,
        }) => {
            println!("Daemon info:");
            println!("  Version: {version}");
            println!("  Uptime: {uptime_secs}s");
            println!("  Active streams: {active_streams}");
        }
        Some(ResponseData::Volume { volume, muted }) => {
            println!("Volume: {volume}%");
            println!("Muted: {}", if muted { "yes" } else { "no" });
        }
        Some(ResponseData::Pong) => {
            println!("Pong!");
        }
        Some(ResponseData::Queue {
            items,
            current_index,
            shuffle,
            repeat,
        }) => {
            if items.is_empty() {
                println!("Queue is empty");
            } else {
                println!("Queue ({} items):", items.len());
                println!(
                    "  Shuffle: {}, Repeat: {}",
                    if shuffle { "on" } else { "off" },
                    repeat
                );
                println!();
                for item in items {
                    let marker = if Some(item.index) == current_index {
                        ">"
                    } else {
                        " "
                    };
                    let duration = item
                        .duration_secs
                        .map(|d| format_duration(Duration::from_secs(d)))
                        .unwrap_or_else(|| "--:--".to_string());
                    println!("{} {:2}. {} [{}]", marker, item.index, item.title, duration);
                }
            }
        }
        Some(ResponseData::Position {
            uri,
            position_secs,
            duration_secs,
            device,
            ..
        }) => {
            println!("Position saved:");
            println!("  URI: {uri}");
            println!(
                "  Position: {} / {}",
                format_duration(Duration::from_secs(position_secs)),
                format_duration(Duration::from_secs(duration_secs))
            );
            println!("  Device: {device}");
        }
        Some(ResponseData::Positions { positions }) => {
            if positions.is_empty() {
                println!("No saved positions");
            } else {
                println!("Saved positions ({}):\n", positions.len());
                for pos in positions {
                    let uri_short = if pos.uri.len() > 50 {
                        &pos.uri[pos.uri.len() - 50..]
                    } else {
                        &pos.uri
                    };
                    println!(
                        "  {} / {} - {}",
                        format_duration(Duration::from_secs(pos.position_secs)),
                        format_duration(Duration::from_secs(pos.duration_secs)),
                        uri_short
                    );
                    println!("    Device: {}", pos.device);
                    println!();
                }
            }
        }
        Some(ResponseData::MediaFiles { files }) => {
            if files.is_empty() {
                println!("No media files (server not running or no directories configured)");
            } else {
                println!("Media library ({} files):\n", files.len());
                for f in files {
                    let size_mb = f.size as f64 / 1024.0 / 1024.0;
                    println!("  {} ({:.1} MB)", f.filename, size_mb);
                    println!("    Path: {}", f.path.display());
                    println!("    Type: {}", f.mime_type);
                    println!();
                }
            }
        }
        Some(ResponseData::Events { events }) => {
            if events.is_empty() {
                println!("No pending events");
            } else {
                println!("Events ({}):\n", events.len());
                for e in events {
                    match e {
                        dlna_combo::Notification::VolumeChanged { device, volume } => {
                            println!("  [{device}] Volume changed to {volume}%");
                        }
                        dlna_combo::Notification::MuteChanged { device, muted } => {
                            println!("  [{}] Mute: {}", device, if muted { "on" } else { "off" });
                        }
                        dlna_combo::Notification::TransportStateChanged { device, state } => {
                            println!("  [{device}] Transport state: {state}");
                        }
                        dlna_combo::Notification::CurrentUriChanged { device, uri } => {
                            println!("  [{device}] Now playing: {uri}");
                        }
                        dlna_combo::Notification::SubscriptionLost { device, service } => {
                            println!("  [{device}] Subscription lost for {service}");
                        }
                    }
                }
            }
        }
        Some(ResponseData::FileCount { count }) => {
            println!("Media files: {count}");
        }
        Some(ResponseData::AutoRefresh { enabled }) => {
            println!("Auto-refresh: {}", if enabled { "on" } else { "off" });
        }
        None => {
            println!("OK");
        }
    }
}
