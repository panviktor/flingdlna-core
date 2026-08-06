//! Direct mode CLI commands (standalone operations without daemon)

use crate::utils::{format_duration, get_hostname, parse_duration, shutdown_signal};
use anyhow::Result;
use dlna_combo::{config::ComboConfig, ControllerConfig, DlnaCombo, ServerConfig};
use std::path::PathBuf;
use std::time::Duration;
use tracing::info;

/// Run media server in standalone mode
pub async fn run_server(
    dirs: Vec<PathBuf>,
    port: u16,
    name: Option<String>,
    watch: bool,
) -> Result<()> {
    let server_config = match name {
        Some(n) => ServerConfig::new(n),
        None => ServerConfig::new(format!("FlingDLNA ({})", get_hostname())),
    }
    .with_port(port)
    .with_directories(dirs);

    let config = ComboConfig::new().with_server(server_config);
    let mut combo = DlnaCombo::new(config).await?;

    // Enable file watching if requested
    if watch {
        if let Some(server) = combo.server_mut() {
            server.set_watch_enabled(true)?;
            info!("File watching enabled");
        }
    }

    combo.start_server().await?;

    info!("Server running. Press Ctrl+C to stop.");

    // Wait for shutdown signal (Ctrl+C or SIGTERM)
    shutdown_signal().await;

    info!("Shutting down gracefully...");

    // Graceful shutdown: send SSDP byebye, stop server
    combo.stop_server().await;

    info!("Server stopped.");
    Ok(())
}

/// List available renderers
pub async fn list_renderers(timeout: u64) -> Result<()> {
    let config = ControllerConfig::new().with_discovery_timeout(Duration::from_secs(timeout));

    let combo = DlnaCombo::controller_only(config).await?;

    println!("Discovering devices...");
    let renderers = combo.discover_renderers().await?;

    if renderers.is_empty() {
        println!("No renderers found.");
    } else {
        println!("\nFound {} renderer(s):\n", renderers.len());
        for renderer in renderers {
            println!("  {} ", renderer.friendly_name);
            println!("    Location: {}", renderer.location);
            if let Some(manufacturer) = &renderer.manufacturer {
                println!("    Manufacturer: {manufacturer}");
            }
            if let Some(model) = &renderer.model_name {
                println!("    Model: {model}");
            }
            println!();
        }
    }

    Ok(())
}

/// Push a file to a renderer
pub async fn push_file(file: PathBuf, device: String, port: u16) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("File not found: {file:?}");
    }

    let config = ControllerConfig::new().with_streaming_port(port);
    let combo = DlnaCombo::controller_only(config).await?;

    println!("Finding device '{device}'...");
    let renderer = combo
        .find_renderer(&device)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Device not found: {device}"))?;

    println!(
        "Pushing {:?} to {}...",
        file.file_name().unwrap_or_default(),
        renderer.friendly_name
    );
    combo.push(&file, &renderer).await?;

    println!("Playback started on {}", renderer.friendly_name);

    // Keep running to serve the file
    println!("Streaming... Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;

    Ok(())
}

/// Pause playback on a device
pub async fn pause_device(device: String) -> Result<()> {
    let combo = DlnaCombo::controller_only(ControllerConfig::new()).await?;

    let renderer = combo
        .find_renderer(&device)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Device not found: {device}"))?;

    combo.pause(&renderer).await?;
    println!("Paused playback on {}", renderer.friendly_name);

    Ok(())
}

/// Resume playback on a device
pub async fn resume_device(device: String) -> Result<()> {
    let combo = DlnaCombo::controller_only(ControllerConfig::new()).await?;

    let renderer = combo
        .find_renderer(&device)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Device not found: {device}"))?;

    combo.controller().resume(&renderer).await?;
    println!("Resumed playback on {}", renderer.friendly_name);

    Ok(())
}

/// Stop playback on a device
pub async fn stop_device(device: String) -> Result<()> {
    let combo = DlnaCombo::controller_only(ControllerConfig::new()).await?;

    let renderer = combo
        .find_renderer(&device)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Device not found: {device}"))?;

    combo.stop(&renderer).await?;
    println!("Stopped playback on {}", renderer.friendly_name);

    Ok(())
}

/// Wake a device using Wake-on-LAN
pub async fn wake_device(device: &str, mac: Option<&str>) -> Result<()> {
    // If MAC is provided directly, use it
    let mac_address = if let Some(m) = mac {
        m.to_string()
    } else {
        // Try to find the device and get its MAC
        let combo = DlnaCombo::controller_only(ControllerConfig::new()).await?;

        if let Ok(Some(renderer)) = combo.find_renderer(device).await {
            if let Some(m) = renderer.mac_address {
                m
            } else if let Some(host) = renderer.location.host_str() {
                // Try to get from ARP cache
                dlna_combo::get_mac_from_arp(host).ok_or_else(|| {
                    anyhow::anyhow!("No MAC address found for {device}. Provide one with --mac")
                })?
            } else {
                return Err(anyhow::anyhow!(
                    "No MAC address found for {device}. Provide one with --mac"
                ));
            }
        } else {
            return Err(anyhow::anyhow!(
                "Device not found: {device}. Provide MAC address with --mac"
            ));
        }
    };

    dlna_combo::wake(&mac_address, None)?;
    println!("Sent Wake-on-LAN packet to {mac_address}");

    Ok(())
}

/// Seek to a position on a device
pub async fn seek_device(position: String, device: String) -> Result<()> {
    let combo = DlnaCombo::controller_only(ControllerConfig::new()).await?;

    let renderer = combo
        .find_renderer(&device)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Device not found: {device}"))?;

    let duration = parse_duration(&position)?;
    combo.seek(&renderer, duration).await?;
    println!("Seeked to {} on {}", position, renderer.friendly_name);

    Ok(())
}

/// Show playback status
pub async fn show_status(device: String) -> Result<()> {
    let combo = DlnaCombo::controller_only(ControllerConfig::new()).await?;

    let renderer = combo
        .find_renderer(&device)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Device not found: {device}"))?;

    let info = combo.get_playback_info(&renderer).await?;

    println!("Status for {}:", renderer.friendly_name);
    println!("  State: {}", info.state);
    println!(
        "  Position: {} / {}",
        format_duration(info.position),
        format_duration(info.duration)
    );
    if let Some(uri) = info.current_uri {
        println!("  Playing: {uri}");
    }

    Ok(())
}

/// Play a URL on a renderer
pub async fn play_url(url: String, device: String) -> Result<()> {
    let combo = DlnaCombo::controller_only(ControllerConfig::new()).await?;

    let renderer = combo
        .find_renderer(&device)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Device not found: {device}"))?;

    combo.play_url(&url, &renderer).await?;
    println!("Playing {} on {}", url, renderer.friendly_name);

    Ok(())
}

/// Control volume on a device
pub async fn volume_control(device: String, level: Option<u8>) -> Result<()> {
    let combo = DlnaCombo::controller_only(ControllerConfig::new()).await?;

    let renderer = combo
        .find_renderer(&device)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Device not found: {device}"))?;

    if let Some(vol) = level {
        // Set volume
        combo.controller().set_volume(&renderer, vol).await?;
        println!("Volume set to {}% on {}", vol, renderer.friendly_name);
    } else {
        // Get volume
        let vol = combo.controller().get_volume(&renderer).await?;
        let muted = combo
            .controller()
            .get_mute(&renderer)
            .await
            .unwrap_or(false);
        println!("Volume on {}: {}%", renderer.friendly_name, vol);
        if muted {
            println!("  (muted)");
        }
    }

    Ok(())
}

/// Control mute on a device
pub async fn mute_control(device: String, state: Option<String>) -> Result<()> {
    let combo = DlnaCombo::controller_only(ControllerConfig::new()).await?;

    let renderer = combo
        .find_renderer(&device)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Device not found: {device}"))?;

    let new_mute = match state {
        Some(s) => s.to_lowercase() == "on" || s == "1" || s == "true",
        None => {
            // Toggle
            let current = combo
                .controller()
                .get_mute(&renderer)
                .await
                .unwrap_or(false);
            !current
        }
    };

    combo.controller().set_mute(&renderer, new_mute).await?;
    println!(
        "Mute {} on {}",
        if new_mute { "enabled" } else { "disabled" },
        renderer.friendly_name
    );

    Ok(())
}
