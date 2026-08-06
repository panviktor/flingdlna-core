//! Navigation methods for App

use super::App;
use crate::tui::types::{HistoryDisplayItem, QueueDisplayItem};

impl App {
    // === Selection Helpers ===

    /// Get the currently selected device name
    pub fn selected_device(&self) -> Option<&str> {
        self.selected_renderer
            .selected()
            .and_then(|i| self.renderers.get(i))
            .map(|r| r.name.as_str())
    }

    /// Get the selected queue item (handles filtered indices)
    pub fn selected_queue_item(&self) -> Option<&QueueDisplayItem> {
        self.selected_queue
            .selected()
            .and_then(|selected_idx| self.filtered_queue_indices.get(selected_idx))
            .and_then(|&actual_idx| self.queue_items.get(actual_idx))
    }

    /// Get the actual queue index for the current selection
    #[allow(dead_code)]
    pub fn selected_actual_queue_index(&self) -> Option<usize> {
        self.selected_queue
            .selected()
            .and_then(|selected_idx| self.filtered_queue_indices.get(selected_idx).copied())
    }

    /// Get the currently selected history item
    pub fn selected_history_item(&self) -> Option<&HistoryDisplayItem> {
        self.selected_history
            .selected()
            .and_then(|i| self.history_items.get(i))
    }

    // === Renderer Navigation ===

    /// Select next renderer, returns true if selection changed
    pub fn select_next_renderer(&mut self) -> bool {
        if self.renderers.is_empty() {
            return false;
        }
        let old = self.selected_renderer.selected();
        let i = match old {
            Some(i) => {
                if i >= self.renderers.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.selected_renderer.select(Some(i));
        old != Some(i)
    }

    /// Select previous renderer, returns true if selection changed
    pub fn select_prev_renderer(&mut self) -> bool {
        if self.renderers.is_empty() {
            return false;
        }
        let old = self.selected_renderer.selected();
        let i = match old {
            Some(i) => {
                if i == 0 {
                    self.renderers.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.selected_renderer.select(Some(i));
        old != Some(i)
    }

    // === Queue Navigation ===

    pub fn select_next_queue(&mut self) {
        let len = if self.search_mode && !self.search_query.is_empty() {
            self.filtered_queue_indices.len()
        } else {
            self.queue_items.len()
        };

        if len == 0 {
            return;
        }

        let i = match self.selected_queue.selected() {
            Some(i) => {
                if i >= len - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.selected_queue.select(Some(i));
    }

    pub fn select_prev_queue(&mut self) {
        let len = if self.search_mode && !self.search_query.is_empty() {
            self.filtered_queue_indices.len()
        } else {
            self.queue_items.len()
        };

        if len == 0 {
            return;
        }

        let i = match self.selected_queue.selected() {
            Some(i) => {
                if i == 0 {
                    len - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.selected_queue.select(Some(i));
    }

    // === Library Navigation ===

    pub fn select_next_library(&mut self) {
        if self.library_entries.is_empty() {
            return;
        }
        let i = match self.selected_library.selected() {
            Some(i) => {
                if i >= self.library_entries.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.selected_library.select(Some(i));
    }

    pub fn select_prev_library(&mut self) {
        if self.library_entries.is_empty() {
            return;
        }
        let i = match self.selected_library.selected() {
            Some(i) => {
                if i == 0 {
                    self.library_entries.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.selected_library.select(Some(i));
    }

    // === History Navigation ===

    pub fn select_next_history(&mut self) {
        if self.history_items.is_empty() {
            return;
        }
        let i = match self.selected_history.selected() {
            Some(i) => {
                if i >= self.history_items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.selected_history.select(Some(i));
    }

    pub fn select_prev_history(&mut self) {
        if self.history_items.is_empty() {
            return;
        }
        let i = match self.selected_history.selected() {
            Some(i) => {
                if i == 0 {
                    self.history_items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.selected_history.select(Some(i));
    }
}
