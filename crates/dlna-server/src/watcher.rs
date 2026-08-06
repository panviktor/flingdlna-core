//! File system watcher for automatic media library updates
//!
//! Uses notify with debouncing to detect file changes and update
//! the media library automatically.

use dlna_core::Result;
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult, Debouncer, RecommendedCache};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Events emitted by the file watcher
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// A new file was created
    Created(PathBuf),
    /// A file was modified
    Modified(PathBuf),
    /// A file was deleted
    Deleted(PathBuf),
    /// A file was renamed (from, to)
    Renamed(PathBuf, PathBuf),
}

/// File watcher with debouncing support
pub struct FileWatcher {
    debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
    watched_paths: Vec<PathBuf>,
}

impl FileWatcher {
    /// Create a new file watcher with the given debounce duration
    ///
    /// The event sender will receive watch events that can be processed
    /// to update the media library.
    pub fn new(debounce_duration: Duration, event_tx: mpsc::Sender<WatchEvent>) -> Result<Self> {
        let tx = event_tx.clone();

        let debouncer = new_debouncer(
            debounce_duration,
            None, // No tick rate limit
            move |result: DebounceEventResult| match result {
                Ok(events) => {
                    for event in events {
                        let watch_events = convert_event(&event);
                        for we in watch_events {
                            if let Err(e) = tx.blocking_send(we) {
                                error!("Failed to send watch event: {}", e);
                            }
                        }
                    }
                }
                Err(errors) => {
                    for e in errors {
                        warn!("File watcher error: {}", e);
                    }
                }
            },
        )
        .map_err(|e| dlna_core::Error::Config(format!("Failed to create file watcher: {e}")))?;

        Ok(Self {
            debouncer,
            watched_paths: Vec::new(),
        })
    }

    /// Start watching a directory
    pub fn watch(&mut self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Err(dlna_core::Error::FileNotFound(path.to_path_buf()));
        }

        self.debouncer
            .watch(path, RecursiveMode::Recursive)
            .map_err(|e| dlna_core::Error::Config(format!("Failed to watch path {path:?}: {e}")))?;

        self.watched_paths.push(path.to_path_buf());
        info!("Started watching: {:?}", path);
        Ok(())
    }

    /// Stop watching a directory
    pub fn unwatch(&mut self, path: &Path) -> Result<()> {
        self.debouncer.unwatch(path).map_err(|e| {
            dlna_core::Error::Config(format!("Failed to unwatch path {path:?}: {e}"))
        })?;

        self.watched_paths.retain(|p| p != path);
        info!("Stopped watching: {:?}", path);
        Ok(())
    }

    /// Get list of currently watched paths
    pub fn watched_paths(&self) -> &[PathBuf] {
        &self.watched_paths
    }
}

/// Convert a notify event to our WatchEvent type
fn convert_event(event: &notify_debouncer_full::DebouncedEvent) -> Vec<WatchEvent> {
    use notify::EventKind;

    let mut events = Vec::new();

    match &event.kind {
        EventKind::Create(_) => {
            for path in &event.paths {
                if is_media_file(path) {
                    debug!("File created: {:?}", path);
                    events.push(WatchEvent::Created(path.clone()));
                }
            }
        }
        EventKind::Modify(notify::event::ModifyKind::Name(_)) => {
            // Rename events have two paths: [from, to]
            if event.paths.len() >= 2 {
                let from = &event.paths[0];
                let to = &event.paths[1];
                debug!("File renamed: {:?} -> {:?}", from, to);
                events.push(WatchEvent::Renamed(from.clone(), to.clone()));
            }
        }
        EventKind::Modify(_) => {
            for path in &event.paths {
                if is_media_file(path) {
                    debug!("File modified: {:?}", path);
                    events.push(WatchEvent::Modified(path.clone()));
                }
            }
        }
        EventKind::Remove(_) => {
            for path in &event.paths {
                // For deletions, we can't check if it was a media file
                // since it no longer exists, so we emit the event anyway
                debug!("File deleted: {:?}", path);
                events.push(WatchEvent::Deleted(path.clone()));
            }
        }
        _ => {}
    }

    events
}

/// Check if a path is a supported media file
fn is_media_file(path: &Path) -> bool {
    const VIDEO_EXTENSIONS: &[&str] = &[
        "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "3gp", "mpg", "mpeg", "ts", "m2ts",
    ];
    const AUDIO_EXTENSIONS: &[&str] = &[
        "mp3", "flac", "wav", "aac", "ogg", "wma", "m4a", "opus", "aiff",
    ];
    const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "bmp", "tiff", "webp"];

    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let e_lower = e.to_lowercase();
            VIDEO_EXTENSIONS.contains(&e_lower.as_str())
                || AUDIO_EXTENSIONS.contains(&e_lower.as_str())
                || IMAGE_EXTENSIONS.contains(&e_lower.as_str())
        })
        .unwrap_or(false)
}

/// Helper to process watch events and update server state
pub struct WatchEventProcessor {
    event_rx: mpsc::Receiver<WatchEvent>,
}

impl WatchEventProcessor {
    /// Create a new processor with the event receiver
    pub fn new(event_rx: mpsc::Receiver<WatchEvent>) -> Self {
        Self { event_rx }
    }

    /// Process events in a loop, calling the handler for each event
    pub async fn run<F>(mut self, mut handler: F)
    where
        F: FnMut(WatchEvent) + Send,
    {
        while let Some(event) = self.event_rx.recv().await {
            handler(event);
        }
    }
}

/// Create a watcher and processor pair with the default debounce duration (250ms)
pub fn create_watcher() -> Result<(FileWatcher, WatchEventProcessor)> {
    create_watcher_with_debounce(Duration::from_millis(250))
}

/// Create a watcher and processor pair with a custom debounce duration
pub fn create_watcher_with_debounce(
    debounce_duration: Duration,
) -> Result<(FileWatcher, WatchEventProcessor)> {
    let (tx, rx) = mpsc::channel(100);
    let watcher = FileWatcher::new(debounce_duration, tx)?;
    let processor = WatchEventProcessor::new(rx);
    Ok((watcher, processor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_media_file() {
        assert!(is_media_file(Path::new("video.mp4")));
        assert!(is_media_file(Path::new("audio.mp3")));
        assert!(is_media_file(Path::new("image.jpg")));
        assert!(!is_media_file(Path::new("document.pdf")));
        assert!(!is_media_file(Path::new("no_extension")));
    }
}
