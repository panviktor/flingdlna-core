//! Input handling for the TUI

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use url::Url;

use super::app::App;
use super::types::{Action, Focus};

/// Handle key press - returns action to perform (if any)
/// This function is INSTANT - no await, no blocking
pub fn handle_key(app: &mut App, key: KeyEvent) -> Option<Action> {
    // URL input mode
    if app.url_input_mode {
        match key.code {
            KeyCode::Esc => {
                app.url_input_mode = false;
                app.url_input.clear();
            }
            KeyCode::Enter => {
                if !app.url_input.is_empty() {
                    let url_str = app.url_input.clone();
                    // Validate URL using url crate (supports http, https, ftp, rtsp, etc.)
                    match Url::parse(&url_str) {
                        Ok(_) => {
                            app.url_input.clear();
                            app.url_input_mode = false;
                            app.set_action("➕ Adding URL to queue...");
                            return Some(Action::AddUrlToQueue(url_str));
                        }
                        Err(e) => {
                            app.status_message = format!("Invalid URL: {e}");
                            app.url_input.clear();
                            app.url_input_mode = false;
                            return None;
                        }
                    }
                }
                app.url_input_mode = false;
            }
            KeyCode::Backspace => {
                app.url_input.pop();
            }
            KeyCode::Char(c) => {
                app.url_input.push(c);
            }
            _ => {}
        }
        return None;
    }

    // Directory input mode
    if app.dir_input_mode {
        match key.code {
            KeyCode::Esc => {
                app.dir_input_mode = false;
                app.dir_input.clear();
            }
            KeyCode::Enter => {
                if !app.dir_input.is_empty() {
                    let dir_str = app.dir_input.clone();
                    app.dir_input.clear();
                    app.dir_input_mode = false;
                    app.set_action("📁 Setting media directory...");
                    return Some(Action::SetMediaDir(dir_str));
                }
                app.dir_input_mode = false;
            }
            KeyCode::Backspace => {
                app.dir_input.pop();
            }
            KeyCode::Char(c) => {
                app.dir_input.push(c);
            }
            _ => {}
        }
        return None;
    }

    // Search mode input
    if app.search_mode {
        match key.code {
            KeyCode::Esc => {
                app.search_mode = false;
                app.search_query.clear();
                app.update_search_filter();
            }
            KeyCode::Enter => {
                app.search_mode = false;
            }
            KeyCode::Backspace => {
                app.search_query.pop();
                app.update_search_filter();
                app.selected_queue.select(Some(0));
            }
            KeyCode::Up => app.select_prev_queue(),
            KeyCode::Down => app.select_next_queue(),
            KeyCode::Char(c) => {
                app.search_query.push(c);
                app.update_search_filter();
                app.selected_queue.select(Some(0));
            }
            _ => {}
        }
        return None;
    }

    // Normal mode
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            app.should_quit = true;
            None
        }
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.should_quit = true;
            None
        }
        KeyCode::Tab => {
            app.focus = match app.focus {
                Focus::Renderers => Focus::Library,
                Focus::Library => Focus::History,
                Focus::History => Focus::Queue,
                Focus::Queue => Focus::Renderers,
            };
            // Refresh history when switching to History tab
            if app.focus == Focus::History {
                return Some(Action::RefreshHistory);
            }
            None
        }
        KeyCode::Up | KeyCode::Char('k') => {
            match app.focus {
                Focus::Renderers => {
                    if app.select_prev_renderer() {
                        return Some(Action::DeviceChanged);
                    }
                }
                Focus::Library => {
                    app.select_prev_library();
                }
                Focus::History => {
                    app.select_prev_history();
                }
                Focus::Queue => {
                    app.select_prev_queue();
                }
            }
            None
        }
        KeyCode::Down | KeyCode::Char('j') => {
            match app.focus {
                Focus::Renderers => {
                    if app.select_next_renderer() {
                        return Some(Action::DeviceChanged);
                    }
                }
                Focus::Library => {
                    app.select_next_library();
                }
                Focus::History => {
                    app.select_next_history();
                }
                Focus::Queue => {
                    app.select_next_queue();
                }
            }
            None
        }
        KeyCode::Left => {
            app.set_action("⏪ -10s");
            Some(Action::SeekBackward(10))
        }
        KeyCode::Right => {
            app.set_action("⏩ +10s");
            Some(Action::SeekForward(10))
        }
        KeyCode::Char('H') => {
            app.set_action("⏪ -30s");
            Some(Action::SeekBackward(30))
        }
        KeyCode::Char('L') => {
            app.set_action("⏩ +30s");
            Some(Action::SeekForward(30))
        }
        KeyCode::Char(' ') => {
            // Space ALWAYS toggles play/pause, regardless of focus
            app.set_action("⏯ Play/Pause");
            Some(Action::PlayPause)
        }
        KeyCode::Char('s') => {
            app.set_action("⏹ Stop");
            Some(Action::Stop)
        }
        KeyCode::Char('n') => {
            app.set_action("⏭ Next");
            Some(Action::NextTrack)
        }
        KeyCode::Char('p') => {
            app.set_action("⏮ Prev");
            Some(Action::PrevTrack)
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            app.set_action("🔊 Vol+");
            Some(Action::VolumeUp)
        }
        KeyCode::Char('-') => {
            app.set_action("🔉 Vol-");
            Some(Action::VolumeDown)
        }
        KeyCode::Char('m') => {
            app.set_action("🔇 Mute");
            Some(Action::ToggleMute)
        }
        KeyCode::Char('S') => {
            app.set_action("🔀 Shuffle");
            Some(Action::ToggleShuffle)
        }
        KeyCode::Char('R') => {
            app.set_action("🔁 Repeat");
            Some(Action::CycleRepeat)
        }
        KeyCode::Char('/') => {
            app.set_action("Search");
            app.search_mode = true;
            app.search_query.clear();
            app.focus = Focus::Queue;
            None
        }
        KeyCode::Char('u') => {
            app.set_action("Enter URL");
            app.url_input_mode = true;
            app.url_input.clear();
            app.focus = Focus::Library; // URL input is in Library panel
            None
        }
        KeyCode::Char('d') => {
            app.set_action("Set Media Dir");
            app.dir_input_mode = true;
            app.dir_input.clear();
            app.focus = Focus::Library;
            None
        }
        KeyCode::Char('M') => {
            // Refresh media library
            app.set_action("🔄 Refresh Media");
            Some(Action::RefreshMediaLibrary)
        }
        KeyCode::Char('W') => {
            // Toggle auto-refresh (file watching)
            app.set_action("🔄 Toggle Auto-Refresh");
            Some(Action::ToggleAutoRefresh)
        }
        KeyCode::Char('r') => {
            app.set_action("🔄 Refresh");
            Some(Action::RefreshRenderers)
        }
        KeyCode::Enter => {
            // Enter: folder navigation in Library, play in Queue, resume in History
            match app.focus {
                Focus::Renderers => Some(Action::RefreshStatus),
                Focus::Library if !app.library_entries.is_empty() => {
                    // Only enter folders - files are added via 'a' key
                    if app.is_selected_folder() {
                        app.set_action("📁 Enter folder");
                        Some(Action::EnterFolder)
                    } else {
                        None
                    }
                }
                Focus::History if !app.history_items.is_empty() => {
                    // Clone uri first to avoid borrow conflict
                    let uri = app.selected_history_item().map(|item| item.uri.clone());
                    if let Some(uri) = uri {
                        app.set_action("▶ Resume from history");
                        Some(Action::ResumeFromHistory(uri))
                    } else {
                        None
                    }
                }
                Focus::Queue if !app.queue_items.is_empty() => {
                    app.set_action("▶ Playing queue item");
                    Some(Action::PlayQueueItem)
                }
                _ => None,
            }
        }
        KeyCode::Char('a') => {
            // 'a' adds to queue without playing (only for files, not folders)
            if app.focus == Focus::Library && app.selected_file().is_some() {
                app.set_action("➕ Add to Queue");
                Some(Action::AddToQueue)
            } else {
                None
            }
        }
        KeyCode::Backspace | KeyCode::Delete => {
            match app.focus {
                Focus::Queue if !app.queue_items.is_empty() => {
                    // Remove selected item from queue
                    app.set_action("➖ Removing from Queue");
                    Some(Action::RemoveFromQueue)
                }
                Focus::Library if app.current_folder.is_some() => {
                    // Go back to parent folder
                    app.set_action("📁 Go back");
                    Some(Action::GoParentFolder)
                }
                _ => None,
            }
        }
        KeyCode::Char('?') => {
            app.status_message = "a:Add Enter:📁/▶ ⌫:Back/Del ←→:±10s Space:⏯ s:Stop n/p:Track +/-:Vol /:Search u:URL d:Dir M:Refresh W:AutoWatch".to_string();
            None
        }
        _ => None,
    }
}
