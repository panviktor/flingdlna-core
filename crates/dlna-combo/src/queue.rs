//! Playback queue management

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Source of media to play
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueueSource {
    /// Local file
    File(PathBuf),
    /// Remote URL
    Url(String),
    /// URL with optional metadata (content type, subtitle URL)
    UrlRequest {
        url: String,
        content_type: Option<String>,
        subtitle_url: Option<String>,
    },
}

impl QueueSource {
    /// Get display name for the source
    pub fn display_name(&self) -> String {
        match self {
            QueueSource::File(path) => path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.to_string_lossy().to_string()),
            QueueSource::Url(url) => url
                .rsplit('/')
                .next()
                .unwrap_or(url)
                .split('?')
                .next()
                .unwrap_or(url)
                .to_string(),
            QueueSource::UrlRequest { url, .. } => url
                .rsplit('/')
                .next()
                .unwrap_or(url)
                .split('?')
                .next()
                .unwrap_or(url)
                .to_string(),
        }
    }
}

/// Item in the playback queue
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueItem {
    /// Media source
    pub source: QueueSource,
    /// Display title
    pub title: String,
    /// Duration if known
    pub duration: Option<Duration>,
}

impl QueueItem {
    /// Create from file path
    pub fn from_file(path: PathBuf) -> Self {
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        Self {
            source: QueueSource::File(path),
            title,
            duration: None,
        }
    }

    /// Create from URL
    pub fn from_url(url: String) -> Self {
        let title = url
            .rsplit('/')
            .next()
            .unwrap_or(&url)
            .split('?')
            .next()
            .unwrap_or(&url)
            .to_string();
        Self {
            source: QueueSource::Url(url),
            title,
            duration: None,
        }
    }

    /// Create from URL request (with optional metadata)
    pub fn from_url_request(
        url: String,
        content_type: Option<String>,
        subtitle_url: Option<String>,
    ) -> Self {
        let title = url
            .rsplit('/')
            .next()
            .unwrap_or(&url)
            .split('?')
            .next()
            .unwrap_or(&url)
            .to_string();
        Self {
            source: QueueSource::UrlRequest {
                url,
                content_type,
                subtitle_url,
            },
            title,
            duration: None,
        }
    }

    /// Set duration
    pub fn with_duration(mut self, duration: Option<Duration>) -> Self {
        self.duration = duration;
        self
    }
}

/// Repeat mode for queue playback
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RepeatMode {
    /// No repeat - stop after last item
    #[default]
    None,
    /// Repeat current item
    One,
    /// Repeat entire queue
    All,
}

/// Playback queue
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Queue {
    /// Items in the queue
    items: Vec<QueueItem>,
    /// Current playback index (None if nothing playing)
    current_index: Option<usize>,
    /// Repeat mode
    repeat_mode: RepeatMode,
    /// Shuffle enabled
    shuffle: bool,
    /// Shuffle order (indices into items)
    shuffle_order: Vec<usize>,
}

impl Queue {
    /// Create empty queue
    pub fn new() -> Self {
        Self::default()
    }

    /// Add item to end of queue
    pub fn add(&mut self, item: QueueItem) {
        self.items.push(item);
        if self.shuffle {
            self.shuffle_order.push(self.items.len() - 1);
        }
    }

    /// Add item at specific position
    pub fn insert(&mut self, index: usize, item: QueueItem) {
        let index = index.min(self.items.len());
        self.items.insert(index, item);

        // Update current index if needed
        if let Some(current) = self.current_index {
            if index <= current {
                self.current_index = Some(current + 1);
            }
        }

        // Rebuild shuffle order
        if self.shuffle {
            self.regenerate_shuffle();
        }
    }

    /// Remove item at index
    pub fn remove(&mut self, index: usize) -> Option<QueueItem> {
        if index >= self.items.len() {
            return None;
        }

        let item = self.items.remove(index);

        // Update current index
        if let Some(current) = self.current_index {
            if index < current {
                self.current_index = Some(current - 1);
            } else if index == current {
                // Current item was removed
                if self.items.is_empty() {
                    self.current_index = None;
                } else if current >= self.items.len() {
                    self.current_index = Some(self.items.len() - 1);
                }
            }
        }

        // Rebuild shuffle order
        if self.shuffle {
            self.regenerate_shuffle();
        }

        Some(item)
    }

    /// Clear all items
    pub fn clear(&mut self) {
        self.items.clear();
        self.current_index = None;
        self.shuffle_order.clear();
    }

    /// Get all items
    pub fn items(&self) -> &[QueueItem] {
        &self.items
    }

    /// Get current item
    pub fn current(&self) -> Option<&QueueItem> {
        self.current_index.and_then(|i| self.items.get(i))
    }

    /// Get current index
    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    /// Set current index
    pub fn set_current(&mut self, index: usize) -> bool {
        if index < self.items.len() {
            self.current_index = Some(index);
            true
        } else {
            false
        }
    }

    /// Move to next item, returns the new current item
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&QueueItem> {
        if self.items.is_empty() {
            return None;
        }

        let next_index = if self.shuffle {
            self.next_shuffle_index()
        } else {
            self.next_sequential_index()
        };

        self.current_index = next_index;
        self.current()
    }

    /// Move to previous item
    pub fn prev(&mut self) -> Option<&QueueItem> {
        if self.items.is_empty() {
            return None;
        }

        let prev_index = if self.shuffle {
            self.prev_shuffle_index()
        } else {
            self.prev_sequential_index()
        };

        self.current_index = prev_index;
        self.current()
    }

    /// Get next index (sequential)
    fn next_sequential_index(&self) -> Option<usize> {
        match self.current_index {
            None => {
                if self.items.is_empty() {
                    None
                } else {
                    Some(0)
                }
            }
            Some(current) => {
                if self.repeat_mode == RepeatMode::One {
                    Some(current)
                } else if current + 1 < self.items.len() {
                    Some(current + 1)
                } else if self.repeat_mode == RepeatMode::All {
                    Some(0)
                } else {
                    None
                }
            }
        }
    }

    /// Get previous index (sequential)
    fn prev_sequential_index(&self) -> Option<usize> {
        match self.current_index {
            None => {
                if self.items.is_empty() {
                    None
                } else {
                    Some(self.items.len() - 1)
                }
            }
            Some(current) => {
                if self.repeat_mode == RepeatMode::One {
                    Some(current)
                } else if current > 0 {
                    Some(current - 1)
                } else if self.repeat_mode == RepeatMode::All {
                    Some(self.items.len() - 1)
                } else {
                    None
                }
            }
        }
    }

    /// Get next index (shuffle)
    fn next_shuffle_index(&self) -> Option<usize> {
        if self.shuffle_order.is_empty() {
            return None;
        }

        let shuffle_pos = self
            .current_index
            .and_then(|ci| self.shuffle_order.iter().position(|&i| i == ci))
            .unwrap_or(0);

        if self.repeat_mode == RepeatMode::One {
            self.current_index
        } else if shuffle_pos + 1 < self.shuffle_order.len() {
            Some(self.shuffle_order[shuffle_pos + 1])
        } else if self.repeat_mode == RepeatMode::All {
            Some(self.shuffle_order[0])
        } else {
            None
        }
    }

    /// Get previous index (shuffle)
    fn prev_shuffle_index(&self) -> Option<usize> {
        if self.shuffle_order.is_empty() {
            return None;
        }

        let shuffle_pos = self
            .current_index
            .and_then(|ci| self.shuffle_order.iter().position(|&i| i == ci))
            .unwrap_or(0);

        if self.repeat_mode == RepeatMode::One {
            self.current_index
        } else if shuffle_pos > 0 {
            Some(self.shuffle_order[shuffle_pos - 1])
        } else if self.repeat_mode == RepeatMode::All {
            Some(self.shuffle_order[self.shuffle_order.len() - 1])
        } else {
            None
        }
    }

    /// Set shuffle mode
    pub fn set_shuffle(&mut self, enabled: bool) {
        if enabled && !self.shuffle {
            self.regenerate_shuffle();
        }
        self.shuffle = enabled;
    }

    /// Get shuffle mode
    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Set repeat mode
    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat_mode = mode;
    }

    /// Get repeat mode
    pub fn repeat(&self) -> RepeatMode {
        self.repeat_mode
    }

    /// Regenerate shuffle order
    fn regenerate_shuffle(&mut self) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        use std::time::SystemTime;

        self.shuffle_order = (0..self.items.len()).collect();

        // Simple Fisher-Yates shuffle with time-based seed
        let mut hasher = DefaultHasher::new();
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .hash(&mut hasher);
        let mut seed = hasher.finish();

        for i in (1..self.shuffle_order.len()).rev() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let j = (seed as usize) % (i + 1);
            self.shuffle_order.swap(i, j);
        }
    }

    /// Get queue length
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Has next item to play
    pub fn has_next(&self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        if self.repeat_mode != RepeatMode::None {
            return true;
        }
        match self.current_index {
            None => !self.items.is_empty(),
            Some(i) => i + 1 < self.items.len(),
        }
    }

    /// Has previous item
    pub fn has_prev(&self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        if self.repeat_mode != RepeatMode::None {
            return true;
        }
        match self.current_index {
            None => !self.items.is_empty(),
            Some(i) => i > 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_queue_add_and_next() {
        let mut queue = Queue::new();
        queue.add(QueueItem::from_url("http://a.mp4".into()));
        queue.add(QueueItem::from_url("http://b.mp4".into()));
        queue.add(QueueItem::from_url("http://c.mp4".into()));

        assert_eq!(queue.len(), 3);
        assert!(queue.current().is_none());

        queue.next();
        assert_eq!(queue.current().unwrap().title, "a.mp4");

        queue.next();
        assert_eq!(queue.current().unwrap().title, "b.mp4");

        queue.next();
        assert_eq!(queue.current().unwrap().title, "c.mp4");

        // No repeat - should stay at end
        assert!(queue.next().is_none());
    }

    #[test]
    fn test_repeat_all() {
        let mut queue = Queue::new();
        queue.add(QueueItem::from_url("http://a.mp4".into()));
        queue.add(QueueItem::from_url("http://b.mp4".into()));
        queue.set_repeat(RepeatMode::All);

        queue.set_current(1); // Start at b
        queue.next(); // Should wrap to a
        assert_eq!(queue.current().unwrap().title, "a.mp4");
    }

    #[test]
    fn test_remove() {
        let mut queue = Queue::new();
        queue.add(QueueItem::from_url("http://a.mp4".into()));
        queue.add(QueueItem::from_url("http://b.mp4".into()));
        queue.add(QueueItem::from_url("http://c.mp4".into()));
        queue.set_current(1); // Playing b

        queue.remove(0); // Remove a
        assert_eq!(queue.current_index(), Some(0)); // Index shifted
        assert_eq!(queue.current().unwrap().title, "b.mp4");
    }
}
