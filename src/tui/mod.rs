//! Interactive TUI for DLNA control using Ratatui
//!
//! Provides a terminal-based user interface for:
//! - Discovering and selecting renderers
//! - Playback control (play, pause, stop, next, prev, seek)
//! - Volume control
//! - Queue management with search

mod actions;
mod app;
pub mod colors;
mod input;
mod render;
pub mod types;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use dlna_combo::DaemonClient;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;
use tokio::sync::mpsc;

use actions::process_action;
use app::App;
use input::handle_key;
use render::ui;
use types::BackgroundResult;

/// Run the TUI
pub async fn run_tui(socket_path: &std::path::Path) -> anyhow::Result<()> {
    let client = DaemonClient::new(socket_path);

    if !client.is_daemon_running() {
        anyhow::bail!("Daemon is not running. Start it with: flingdlna daemon --foreground");
    }

    // Setup terminal FIRST - show UI immediately
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(client);

    // Channel for background task results
    let (bg_tx, mut bg_rx) = mpsc::channel::<BackgroundResult>(10);

    // Load cached devices first (instant)
    app.refresh_renderers_cached().await;
    // Load media library
    app.refresh_media_files().await;
    // Load daemon info
    app.refresh_daemon_info().await;
    // Load watch history
    app.refresh_history().await;
    // Load queue for selected device (if any)
    app.refresh_queue().await;
    terminal.draw(|f| ui(f, &app))?;

    // If no cached devices, start background discovery (non-blocking)
    if app.renderers.is_empty() {
        app.start_background_discovery(bg_tx.clone(), 2);
        terminal.draw(|f| ui(f, &app))?;
    }

    // Start initial status refresh in background (non-blocking)
    if !app.renderers.is_empty() {
        if let Some(device) = app.selected_device().map(|s| s.to_string()) {
            app.start_background_status_refresh(bg_tx.clone(), device);
        }
    }

    // Event poll timeout - short enough for responsive UI, long enough to not spin CPU
    let poll_timeout = Duration::from_millis(50);
    // Track time for periodic daemon polling
    let mut last_daemon_poll = std::time::Instant::now();
    let daemon_poll_interval = Duration::from_secs(6);
    // Track time for UI updates (progress bar interpolation)
    let mut last_ui_update = std::time::Instant::now();
    let ui_update_interval = Duration::from_secs(1);

    loop {
        terminal.draw(|f| ui(f, &app))?;

        // === PHASE 1: Check for input events (non-blocking with short timeout) ===
        // This is the KEY to responsive UI - poll with timeout, never block indefinitely
        if event::poll(poll_timeout)? {
            if let Event::Key(key) = event::read()? {
                let action = handle_key(&mut app, key);

                // Redraw immediately after key press
                terminal.draw(|f| ui(f, &app))?;

                // Process async action (spawns background task, returns immediately)
                if let Some(action) = action {
                    process_action(&mut app, action, bg_tx.clone()).await;
                }

                if app.should_quit {
                    break;
                }
            }
        }

        // === PHASE 2: Process ALL pending background results (non-blocking) ===
        // Drain the channel without blocking
        loop {
            match bg_rx.try_recv() {
                Ok(result) => {
                    let needs_status_refresh = matches!(
                        result,
                        BackgroundResult::Renderers(_) | BackgroundResult::OperationSuccess(_)
                    );
                    app.handle_background_result(result);
                    if needs_status_refresh && !app.renderers.is_empty() {
                        if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                            app.start_background_status_refresh(bg_tx.clone(), device);
                        }
                    }
                }
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => break,
            }
        }

        // === PHASE 3: Periodic UI updates (for progress bar interpolation) ===
        if last_ui_update.elapsed() >= ui_update_interval {
            last_ui_update = std::time::Instant::now();
            app.clear_expired_action();
        }

        // === PHASE 4: Periodic daemon polling ===
        if last_daemon_poll.elapsed() >= daemon_poll_interval {
            last_daemon_poll = std::time::Instant::now();
            app.start_background_daemon_info(bg_tx.clone());
            if !app.renderers.is_empty() {
                if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                    app.start_background_status_refresh(bg_tx.clone(), device.clone());

                    // Poll for UPnP events if subscribed
                    if app.subscribed_device.as_ref() == Some(&device) {
                        app.start_background_event_poll(bg_tx.clone(), Some(device.clone()));
                    }

                    // Auto-subscribe if not already subscribed to this device
                    if app.subscribed_device.as_ref() != Some(&device) {
                        app.start_background_subscribe(bg_tx.clone(), device.clone());
                    }

                    // Auto-save playback position every 30 seconds while playing
                    if app.should_auto_save_position() {
                        app.start_background_save_position(device);
                    }
                }
            }
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(())
}
