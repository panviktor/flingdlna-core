//! Utility functions for CLI

use anyhow::Result;
use std::time::Duration;
use tracing::info;

/// Get system hostname with fallback
pub fn get_hostname() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "FlingDLNA".to_string())
}

/// Parse duration from string (seconds or HH:MM:SS or MM:SS)
pub fn parse_duration(s: &str) -> Result<Duration> {
    // Try parsing as seconds first
    if let Ok(secs) = s.parse::<u64>() {
        return Ok(Duration::from_secs(secs));
    }

    // Try parsing as HH:MM:SS or MM:SS
    let parts: Vec<&str> = s.split(':').collect();
    let secs = match parts.len() {
        2 => {
            let mins: u64 = parts[0].parse()?;
            let secs: u64 = parts[1].parse()?;
            mins * 60 + secs
        }
        3 => {
            let hours: u64 = parts[0].parse()?;
            let mins: u64 = parts[1].parse()?;
            let secs: u64 = parts[2].parse()?;
            hours * 3600 + mins * 60 + secs
        }
        _ => anyhow::bail!("Invalid duration format: {s}"),
    };

    Ok(Duration::from_secs(secs))
}

/// Format duration as HH:MM:SS or MM:SS
pub fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let secs = secs % 60;

    if hours > 0 {
        format!("{hours:02}:{mins:02}:{secs:02}")
    } else {
        format!("{mins:02}:{secs:02}")
    }
}

/// Wait for shutdown signal (Ctrl+C or SIGTERM on Unix)
pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C");
        }
        _ = terminate => {
            info!("Received SIGTERM");
        }
    }
}
