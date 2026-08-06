//! Sync (blocking) refresh operations for App

use dlna_combo::{Command as DaemonCommand, Response, ResponseData};

use super::App;
use crate::tui::types::{HistoryDisplayItem, QueueDisplayItem};

impl App {
    /// Load cached renderers (instant, no network)
    pub async fn refresh_renderers_cached(&mut self) {
        match self.client.send(&DaemonCommand::ListCached).await {
            Ok(Response::Ok {
                data: Some(ResponseData::Renderers { renderers }),
            }) => {
                self.renderers = renderers;
                if self.renderers.is_empty() {
                    self.status_message = "No cached devices".to_string();
                } else {
                    self.status_message = format!("{} device(s) (cached)", self.renderers.len());
                }
            }
            _ => {
                self.status_message = "No cached devices".to_string();
            }
        }
    }

    /// Load media files from server library
    pub async fn refresh_media_files(&mut self) {
        match self.client.send(&DaemonCommand::ListMedia).await {
            Ok(Response::Ok {
                data: Some(ResponseData::MediaFiles { files }),
            }) => {
                self.media_files = files;
                self.build_library_entries();
            }
            Ok(Response::Error { message, .. }) => {
                self.status_message = format!("Library: {message}");
            }
            _ => {}
        }
    }

    /// Refresh daemon info
    pub async fn refresh_daemon_info(&mut self) {
        if let Ok(Response::Ok {
            data:
                Some(ResponseData::DaemonInfo {
                    uptime_secs,
                    active_streams,
                    ..
                }),
        }) = self.client.send(&DaemonCommand::Info).await
        {
            self.daemon_uptime = uptime_secs;
            self.daemon_streams = active_streams;
        }
    }

    /// Load watch history from daemon (for initial load)
    pub async fn refresh_history(&mut self) {
        match self.client.send(&DaemonCommand::ListPositions).await {
            Ok(Response::Ok {
                data: Some(ResponseData::Positions { positions }),
            }) => {
                self.history_items = positions
                    .into_iter()
                    .map(|p| {
                        // Extract title from URI
                        let title = p.uri.rsplit('/').next().unwrap_or(&p.uri).to_string();
                        HistoryDisplayItem {
                            uri: p.uri,
                            title,
                            position_secs: p.position_secs,
                            duration_secs: p.duration_secs,
                            device: p.device,
                            saved_at: p.saved_at,
                        }
                    })
                    .collect();

                // Clamp selection
                if let Some(selected) = self.selected_history.selected() {
                    if selected >= self.history_items.len() {
                        self.selected_history
                            .select(if self.history_items.is_empty() {
                                None
                            } else {
                                Some(0)
                            });
                    }
                } else if !self.history_items.is_empty() {
                    self.selected_history.select(Some(0));
                }
            }
            Ok(Response::Error { message, .. }) => {
                self.status_message = format!("History: {message}");
            }
            _ => {}
        }
    }

    /// Load queue from daemon for selected device (for initial load)
    pub async fn refresh_queue(&mut self) {
        let device = match self.selected_device() {
            Some(d) => d.to_string(),
            None => return,
        };

        if let Ok(Response::Ok {
            data:
                Some(ResponseData::Queue {
                    items,
                    current_index,
                    shuffle,
                    repeat,
                }),
        }) = self.client.send(&DaemonCommand::QueueList { device }).await
        {
            self.queue_items = items
                .into_iter()
                .map(|i| QueueDisplayItem {
                    index: i.index,
                    title: i.title,
                    source: i.source,
                    is_url: i.is_url,
                    duration: i.duration_secs,
                })
                .collect();
            self.queue_current = current_index;
            self.shuffle = shuffle;
            self.repeat = repeat;
            self.update_search_filter();
            self.clamp_queue_selection();
        }
    }
}
