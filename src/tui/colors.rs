//! Color scheme for the TUI

use ratatui::style::Color;

#[allow(dead_code)]
pub const BG: Color = Color::Reset;
pub const FG: Color = Color::White;
pub const ACCENT: Color = Color::Rgb(0, 191, 255); // Deep sky blue
pub const PLAYING: Color = Color::Rgb(50, 205, 50); // Lime green
pub const PAUSED: Color = Color::Rgb(255, 165, 0); // Orange
pub const STOPPED: Color = Color::Rgb(220, 20, 60); // Crimson
pub const MUTED: Color = Color::Rgb(255, 69, 0); // Red-orange
pub const PROGRESS_FG: Color = Color::Rgb(0, 191, 255);
pub const PROGRESS_BG: Color = Color::Rgb(40, 40, 40);
pub const SELECTED: Color = Color::Rgb(255, 215, 0); // Gold
pub const DIM: Color = Color::Rgb(128, 128, 128);
pub const QUEUE_CURRENT: Color = Color::Rgb(50, 205, 50);
pub const BORDER_FOCUS: Color = Color::Rgb(0, 191, 255);
pub const BORDER_NORMAL: Color = Color::Rgb(80, 80, 80);
pub const SEARCH_BG: Color = Color::Rgb(30, 30, 50);
