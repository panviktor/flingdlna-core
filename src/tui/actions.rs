//! Action processing for the TUI

use dlna_combo::{Command as DaemonCommand, Response, ResponseData};
use std::sync::Arc;
use tokio::sync::mpsc;

use super::app::App;
use super::types::{Action, BackgroundResult};

/// Process async action
pub async fn process_action(app: &mut App, action: Action, bg_tx: mpsc::Sender<BackgroundResult>) {
    match action {
        Action::SeekBackward(secs) => {
            if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                let client = Arc::clone(&app.client);
                let tx = bg_tx.clone();
                let target = app.playback_position.saturating_sub(secs);
                // Reset interpolation until we get confirmation from device
                app.position_last_update = None;
                app.pending_operation = Some(format!("Seeking -{secs}s..."));
                tokio::spawn(async move {
                    let result = match client
                        .send(&DaemonCommand::Seek {
                            device,
                            position_secs: target,
                        })
                        .await
                    {
                        Ok(Response::Ok { .. }) => {
                            BackgroundResult::OperationSuccess(format!("-{secs}s"))
                        }
                        Ok(Response::Error { message, .. }) => {
                            BackgroundResult::OperationError(message)
                        }
                        Err(e) => BackgroundResult::OperationError(e.to_string()),
                    };
                    let _ = tx.send(result).await;
                });
            }
        }
        Action::SeekForward(secs) => {
            if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                let client = Arc::clone(&app.client);
                let tx = bg_tx.clone();
                let new_pos = app.playback_position.saturating_add(secs);
                let target = if app.playback_duration > 0 {
                    new_pos.min(app.playback_duration.saturating_sub(1))
                } else {
                    new_pos
                };
                // Reset interpolation until we get confirmation from device
                app.position_last_update = None;
                app.pending_operation = Some(format!("Seeking +{secs}s..."));
                tokio::spawn(async move {
                    let result = match client
                        .send(&DaemonCommand::Seek {
                            device,
                            position_secs: target,
                        })
                        .await
                    {
                        Ok(Response::Ok { .. }) => {
                            BackgroundResult::OperationSuccess(format!("+{secs}s"))
                        }
                        Ok(Response::Error { message, .. }) => {
                            BackgroundResult::OperationError(message)
                        }
                        Err(e) => BackgroundResult::OperationError(e.to_string()),
                    };
                    let _ = tx.send(result).await;
                });
            }
        }
        Action::PlayPause => {
            if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                let client = Arc::clone(&app.client);
                let tx = bg_tx.clone();
                let is_playing = app.playback_state == "Playing";

                // Save position before pausing (fire-and-forget)
                if is_playing {
                    app.start_background_save_position(device.clone());
                }

                // Set pending indicator (no optimistic update - wait for device confirmation)
                app.pending_operation = Some(if is_playing {
                    "Pausing...".to_string()
                } else {
                    "Resuming...".to_string()
                });
                tokio::spawn(async move {
                    let cmd = if is_playing {
                        DaemonCommand::Pause { device }
                    } else {
                        DaemonCommand::Resume { device }
                    };
                    let result = match client.send(&cmd).await {
                        Ok(Response::Ok { .. }) => {
                            if is_playing {
                                BackgroundResult::OperationSuccess("Paused".to_string())
                            } else {
                                BackgroundResult::OperationSuccess("Playing".to_string())
                            }
                        }
                        Ok(Response::Error { message, .. }) => {
                            BackgroundResult::OperationError(message)
                        }
                        Err(e) => BackgroundResult::OperationError(e.to_string()),
                    };
                    let _ = tx.send(result).await;
                });
            }
        }
        Action::PlayQueueItem => {
            // Play the selected queue item (can be file or URL) - run in background
            if let Some(item) = app.selected_queue_item().cloned() {
                if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                    let client = Arc::clone(&app.client);
                    let tx = bg_tx.clone();
                    let title = item.title.clone();
                    let source = item.source.clone();
                    let is_url = item.is_url;
                    tokio::spawn(async move {
                        let result = if is_url {
                            // Play URL directly
                            match client
                                .send(&DaemonCommand::PlayUrl {
                                    url: source,
                                    device,
                                })
                                .await
                            {
                                Ok(Response::Ok { .. }) => BackgroundResult::OperationSuccess(
                                    format!("Playing URL: {title}"),
                                ),
                                Ok(Response::Error { message, .. }) => {
                                    BackgroundResult::OperationError(message)
                                }
                                Err(e) => BackgroundResult::OperationError(e.to_string()),
                            }
                        } else {
                            // Play file via Push
                            match client
                                .send(&DaemonCommand::Push {
                                    file: std::path::PathBuf::from(&source),
                                    device,
                                    port: 9000,
                                })
                                .await
                            {
                                Ok(Response::Ok { .. }) => {
                                    BackgroundResult::OperationSuccess(format!("Playing: {title}"))
                                }
                                Ok(Response::Error { message, .. }) => {
                                    BackgroundResult::OperationError(message)
                                }
                                Err(e) => BackgroundResult::OperationError(e.to_string()),
                            }
                        };
                        let _ = tx.send(result).await;
                    });
                } else {
                    app.status_message = "No device selected".to_string();
                }
            }
        }
        Action::Stop => {
            if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                // Save position before stopping (fire-and-forget)
                app.start_background_save_position(device.clone());

                let client = Arc::clone(&app.client);
                let tx = bg_tx.clone();
                tokio::spawn(async move {
                    let result = match client.send(&DaemonCommand::Stop { device }).await {
                        Ok(Response::Ok { .. }) => {
                            BackgroundResult::OperationSuccess("Stopped".to_string())
                        }
                        Ok(Response::Error { message, .. }) => {
                            BackgroundResult::OperationError(message)
                        }
                        Err(e) => BackgroundResult::OperationError(e.to_string()),
                    };
                    let _ = tx.send(result).await;
                });
            }
        }
        Action::NextTrack => {
            if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                let client = Arc::clone(&app.client);
                let tx = bg_tx.clone();
                tokio::spawn(async move {
                    let result = match client.send(&DaemonCommand::Next { device }).await {
                        Ok(Response::Ok { .. }) => {
                            BackgroundResult::OperationSuccess("Next track".to_string())
                        }
                        Ok(Response::Error { message, .. }) => {
                            BackgroundResult::OperationError(message)
                        }
                        Err(e) => BackgroundResult::OperationError(e.to_string()),
                    };
                    let _ = tx.send(result).await;
                });
            }
        }
        Action::PrevTrack => {
            if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                let client = Arc::clone(&app.client);
                let tx = bg_tx.clone();
                tokio::spawn(async move {
                    let result = match client.send(&DaemonCommand::Prev { device }).await {
                        Ok(Response::Ok { .. }) => {
                            BackgroundResult::OperationSuccess("Previous track".to_string())
                        }
                        Ok(Response::Error { message, .. }) => {
                            BackgroundResult::OperationError(message)
                        }
                        Err(e) => BackgroundResult::OperationError(e.to_string()),
                    };
                    let _ = tx.send(result).await;
                });
            }
        }
        Action::VolumeUp => {
            if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                let client = Arc::clone(&app.client);
                let tx = bg_tx.clone();
                // Use AdjustVolume - daemon will read current volume from device
                tokio::spawn(async move {
                    let result = match client
                        .send(&DaemonCommand::AdjustVolume { device, delta: 5 })
                        .await
                    {
                        Ok(Response::Ok {
                            data: Some(ResponseData::Volume { volume, .. }),
                            ..
                        }) => BackgroundResult::OperationSuccess(format!("Vol {volume}%")),
                        Ok(Response::Ok { .. }) => {
                            BackgroundResult::OperationSuccess("Vol +".to_string())
                        }
                        Ok(Response::Error { message, .. }) => {
                            BackgroundResult::OperationError(message)
                        }
                        Err(e) => BackgroundResult::OperationError(e.to_string()),
                    };
                    let _ = tx.send(result).await;
                });
            }
        }
        Action::VolumeDown => {
            if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                let client = Arc::clone(&app.client);
                let tx = bg_tx.clone();
                // Use AdjustVolume - daemon will read current volume from device
                tokio::spawn(async move {
                    let result = match client
                        .send(&DaemonCommand::AdjustVolume { device, delta: -5 })
                        .await
                    {
                        Ok(Response::Ok {
                            data: Some(ResponseData::Volume { volume, .. }),
                            ..
                        }) => BackgroundResult::OperationSuccess(format!("Vol {volume}%")),
                        Ok(Response::Ok { .. }) => {
                            BackgroundResult::OperationSuccess("Vol -".to_string())
                        }
                        Ok(Response::Error { message, .. }) => {
                            BackgroundResult::OperationError(message)
                        }
                        Err(e) => BackgroundResult::OperationError(e.to_string()),
                    };
                    let _ = tx.send(result).await;
                });
            }
        }
        Action::ToggleMute => {
            if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                let client = Arc::clone(&app.client);
                let tx = bg_tx.clone();
                let was_muted = app.muted;
                // No optimistic update - wait for device confirmation via UPnP events
                tokio::spawn(async move {
                    let result = match client
                        .send(&DaemonCommand::Mute { device, mute: None })
                        .await
                    {
                        Ok(Response::Ok { .. }) => {
                            if was_muted {
                                BackgroundResult::OperationSuccess("Unmuted".to_string())
                            } else {
                                BackgroundResult::OperationSuccess("Muted".to_string())
                            }
                        }
                        Ok(Response::Error { message, .. }) => {
                            BackgroundResult::OperationError(message)
                        }
                        Err(e) => BackgroundResult::OperationError(e.to_string()),
                    };
                    let _ = tx.send(result).await;
                });
            }
        }
        Action::ToggleShuffle => {
            if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                let client = Arc::clone(&app.client);
                let tx = bg_tx.clone();
                let enabled = !app.shuffle;
                // No optimistic update - wait for queue refresh confirmation
                tokio::spawn(async move {
                    let result = match client
                        .send(&DaemonCommand::Shuffle { device, enabled })
                        .await
                    {
                        Ok(Response::Ok { .. }) => {
                            if enabled {
                                BackgroundResult::OperationSuccess("Shuffle ON".to_string())
                            } else {
                                BackgroundResult::OperationSuccess("Shuffle off".to_string())
                            }
                        }
                        Ok(Response::Error { message, .. }) => {
                            BackgroundResult::OperationError(message)
                        }
                        Err(e) => BackgroundResult::OperationError(e.to_string()),
                    };
                    let _ = tx.send(result).await;
                });
            }
        }
        Action::CycleRepeat => {
            if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                let client = Arc::clone(&app.client);
                let tx = bg_tx.clone();
                let new_mode = match app.repeat.as_str() {
                    "none" => "all",
                    "all" => "one",
                    _ => "none",
                };
                // No optimistic update - wait for queue refresh confirmation
                let mode_str = new_mode.to_string();
                tokio::spawn(async move {
                    let result = match client
                        .send(&DaemonCommand::Repeat {
                            device,
                            mode: mode_str.clone(),
                        })
                        .await
                    {
                        Ok(Response::Ok { .. }) => {
                            BackgroundResult::OperationSuccess(format!("Repeat: {mode_str}"))
                        }
                        Ok(Response::Error { message, .. }) => {
                            BackgroundResult::OperationError(message)
                        }
                        Err(e) => BackgroundResult::OperationError(e.to_string()),
                    };
                    let _ = tx.send(result).await;
                });
            }
        }
        Action::RefreshRenderers => {
            // Use background discovery to avoid blocking UI
            app.start_background_discovery(bg_tx, 3);
        }
        Action::RefreshStatus => {
            // Don't block - status is refreshed periodically anyway
        }
        Action::AddToQueue => {
            // Prevent race conditions: don't start new queue operation while one is pending
            if app.pending_operation.is_some() {
                return;
            }
            // Run in background to avoid blocking UI
            if let Some(file) = app.selected_file().cloned() {
                if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                    let client = Arc::clone(&app.client);
                    let tx = bg_tx.clone();
                    let filename = file.filename.clone();
                    let path = file.path.to_string_lossy().to_string();

                    // No optimistic update - wait for daemon confirmation
                    app.pending_operation = Some(format!("Adding {filename}..."));

                    tokio::spawn(async move {
                        // Step 1: Add to queue
                        let add_result = client
                            .send(&DaemonCommand::QueueAdd {
                                source: path,
                                title: None,
                                content_type: None,
                                subtitle_url: None,
                                device: device.clone(),
                                position: None,
                            })
                            .await;

                        let result = match add_result {
                            Ok(Response::Ok { .. }) => {
                                // Step 2: Immediately fetch updated queue (fast - local data only)
                                match client.send(&DaemonCommand::QueueList { device }).await {
                                    Ok(Response::Ok {
                                        data:
                                            Some(ResponseData::Queue {
                                                items,
                                                current_index,
                                                shuffle,
                                                repeat,
                                            }),
                                    }) => BackgroundResult::QueueUpdated {
                                        message: format!("Added: {filename}"),
                                        items,
                                        current_index,
                                        shuffle,
                                        repeat,
                                    },
                                    _ => BackgroundResult::OperationSuccess(format!(
                                        "Added: {filename}"
                                    )),
                                }
                            }
                            Ok(Response::Error { message, .. }) => {
                                BackgroundResult::OperationError(message)
                            }
                            Err(e) => BackgroundResult::OperationError(e.to_string()),
                        };
                        let _ = tx.send(result).await;
                    });
                } else {
                    app.status_message = "No device selected".to_string();
                }
            }
        }
        Action::RemoveFromQueue => {
            // Prevent race conditions: don't start new queue operation while one is pending
            if app.pending_operation.is_some() {
                return;
            }
            // Get the selected queue item index from filtered_queue_indices
            if let Some(selected_idx) = app.selected_queue.selected() {
                // Map filtered index to actual queue index - bail if out of bounds
                let Some(&actual_index) = app.filtered_queue_indices.get(selected_idx) else {
                    return; // Selection out of bounds, skip this operation
                };
                // Also verify the actual index is valid for queue_items
                if actual_index >= app.queue_items.len() {
                    return; // Invalid index, skip
                }
                if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                    let client = Arc::clone(&app.client);
                    let tx = bg_tx.clone();
                    let title = app
                        .queue_items
                        .get(actual_index)
                        .map(|q| q.title.clone())
                        .unwrap_or_default();

                    // No optimistic update - wait for daemon confirmation
                    app.pending_operation = Some(format!("Removing {title}..."));

                    tokio::spawn(async move {
                        // Step 1: Remove from queue
                        let remove_result = client
                            .send(&DaemonCommand::QueueRemove {
                                device: device.clone(),
                                index: actual_index,
                            })
                            .await;

                        let result = match remove_result {
                            Ok(Response::Ok { .. }) => {
                                // Step 2: Immediately fetch updated queue (fast - local data only)
                                match client.send(&DaemonCommand::QueueList { device }).await {
                                    Ok(Response::Ok {
                                        data:
                                            Some(ResponseData::Queue {
                                                items,
                                                current_index,
                                                shuffle,
                                                repeat,
                                            }),
                                    }) => BackgroundResult::QueueUpdated {
                                        message: format!("Removed: {title}"),
                                        items,
                                        current_index,
                                        shuffle,
                                        repeat,
                                    },
                                    _ => BackgroundResult::OperationSuccess(format!(
                                        "Removed: {title}"
                                    )),
                                }
                            }
                            Ok(Response::Error { message, .. }) => {
                                BackgroundResult::OperationError(message)
                            }
                            Err(e) => BackgroundResult::OperationError(e.to_string()),
                        };
                        let _ = tx.send(result).await;
                    });
                } else {
                    app.status_message = "No device selected".to_string();
                }
            }
        }
        Action::AddUrlToQueue(url) => {
            // Prevent race conditions: don't start new queue operation while one is pending
            if app.pending_operation.is_some() {
                return;
            }
            // Only add URL to queue without playing
            if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                let client = Arc::clone(&app.client);
                let tx = bg_tx.clone();
                let url_short = if url.len() > 40 {
                    format!("{}...", &url[..40])
                } else {
                    url.clone()
                };

                // No optimistic update - wait for daemon confirmation
                app.pending_operation = Some(format!("Adding {url_short}..."));

                tokio::spawn(async move {
                    // Step 1: Add URL to queue
                    let add_result = client
                        .send(&DaemonCommand::QueueAdd {
                            source: url,
                            title: None,
                            content_type: None,
                            subtitle_url: None,
                            device: device.clone(),
                            position: None,
                        })
                        .await;

                    let result = match add_result {
                        Ok(Response::Ok { .. }) => {
                            // Step 2: Immediately fetch updated queue (fast - local data only)
                            match client.send(&DaemonCommand::QueueList { device }).await {
                                Ok(Response::Ok {
                                    data:
                                        Some(ResponseData::Queue {
                                            items,
                                            current_index,
                                            shuffle,
                                            repeat,
                                        }),
                                }) => BackgroundResult::QueueUpdated {
                                    message: format!("Added: {url_short}"),
                                    items,
                                    current_index,
                                    shuffle,
                                    repeat,
                                },
                                _ => BackgroundResult::OperationSuccess(format!(
                                    "Added: {url_short}"
                                )),
                            }
                        }
                        Ok(Response::Error { message, .. }) => {
                            BackgroundResult::OperationError(message)
                        }
                        Err(e) => BackgroundResult::OperationError(e.to_string()),
                    };
                    let _ = tx.send(result).await;
                });
            } else {
                app.status_message = "No device selected".to_string();
            }
        }
        Action::EnterFolder => {
            // Enter selected folder in library
            app.enter_selected_folder();
        }
        Action::GoParentFolder => {
            // Go back to parent folder in library
            app.go_parent_folder();
        }
        Action::ResumeFromHistory(uri) => {
            // Resume playback from saved history position
            if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                let client = Arc::clone(&app.client);
                let tx = bg_tx.clone();
                let uri_short = uri.rsplit('/').next().unwrap_or(&uri).to_string();
                app.pending_operation = Some(format!("Resuming {uri_short}..."));
                tokio::spawn(async move {
                    let result = match client
                        .send(&DaemonCommand::ResumeFrom {
                            device,
                            uri: uri.clone(),
                        })
                        .await
                    {
                        Ok(Response::Ok { .. }) => {
                            BackgroundResult::OperationSuccess(format!("Resumed: {uri_short}"))
                        }
                        Ok(Response::Error { message, .. }) => {
                            BackgroundResult::OperationError(message)
                        }
                        Err(e) => BackgroundResult::OperationError(e.to_string()),
                    };
                    let _ = tx.send(result).await;
                });
            } else {
                app.status_message = "No device selected".to_string();
            }
        }
        Action::RefreshHistory => {
            // Refresh history from daemon in background
            let client = Arc::clone(&app.client);
            let tx = bg_tx.clone();
            tokio::spawn(async move {
                match client.send(&DaemonCommand::ListPositions).await {
                    Ok(Response::Ok {
                        data: Some(ResponseData::Positions { positions }),
                    }) => {
                        // Convert to HistoryDisplayItem and send via BackgroundResult
                        let items: Vec<super::types::HistoryDisplayItem> = positions
                            .into_iter()
                            .map(|p| {
                                let title = p.uri.rsplit('/').next().unwrap_or(&p.uri).to_string();
                                super::types::HistoryDisplayItem {
                                    uri: p.uri,
                                    title,
                                    position_secs: p.position_secs,
                                    duration_secs: p.duration_secs,
                                    device: p.device,
                                    saved_at: p.saved_at,
                                }
                            })
                            .collect();
                        let _ = tx.send(BackgroundResult::HistoryRefreshed(items)).await;
                    }
                    _ => {
                        // Silently ignore errors - history is not critical
                    }
                }
            });
        }
        Action::DeviceChanged => {
            // Device selection changed - load queue for new device
            if let Some(device) = app.selected_device().map(|s| s.to_string()) {
                // Clear old queue immediately to avoid showing stale data
                app.queue_items.clear();
                app.filtered_queue_indices.clear();
                app.queue_current = None;

                // Reset playback state for new device
                app.playback_state = "Unknown".to_string();
                app.playback_position = 0;
                app.playback_duration = 0;
                app.current_uri = None;
                app.position_last_update = None;
                // Note: volume/muted NOT reset - will be updated when new device status arrives
                // Old device updates are ignored via device check in apply_status_update

                // Fetch queue for new device
                app.start_background_queue_refresh(bg_tx.clone(), device.clone());

                // Fetch status for new device
                app.start_background_status_refresh(bg_tx.clone(), device.clone());

                // Subscribe to events for new device
                app.start_background_subscribe(bg_tx, device);
            }
        }
        Action::SetMediaDir(path) => {
            // Set media directory
            let client = Arc::clone(&app.client);
            let tx = bg_tx.clone();
            let path_clone = path.clone();
            app.pending_operation = Some("Setting media directory...".to_string());
            tokio::spawn(async move {
                let result = match client
                    .send(&DaemonCommand::SetMediaDir {
                        path: std::path::PathBuf::from(&path),
                    })
                    .await
                {
                    Ok(Response::Ok {
                        data: Some(ResponseData::FileCount { count }),
                    }) => BackgroundResult::OperationSuccess(format!(
                        "Set dir: {path_clone} ({count} files)"
                    )),
                    Ok(Response::Ok { .. }) => {
                        BackgroundResult::OperationSuccess(format!("Set dir: {path_clone}"))
                    }
                    Ok(Response::Error { message, .. }) => {
                        BackgroundResult::OperationError(message)
                    }
                    Err(e) => BackgroundResult::OperationError(e.to_string()),
                };
                let _ = tx.send(result).await;
            });
            // Refresh media library after setting directory
            app.start_background_media_refresh(bg_tx);
        }
        Action::RefreshMediaLibrary => {
            // Refresh media library from daemon
            app.start_background_media_refresh(bg_tx);
        }
        Action::ToggleAutoRefresh => {
            // Toggle auto-refresh (file watching)
            app.start_background_toggle_auto_refresh(bg_tx);
        }
    }
}
