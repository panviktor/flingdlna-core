//! Rendering functions for the TUI

use crate::utils::format_duration;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Gauge, List, ListItem, Paragraph},
    Frame,
};
use std::time::Duration;

use super::app::App;
use super::colors;
use super::types::Focus;

/// Main UI render function
pub fn ui(f: &mut Frame, app: &App) {
    let area = f.area();

    // Main vertical layout
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header with device + state
            Constraint::Length(3), // Progress bar
            Constraint::Min(8),    // Content area
            Constraint::Length(3), // Status bar
        ])
        .split(area);

    // Header: Device name + playback state
    render_header(f, app, main_chunks[0]);

    // Progress bar
    render_progress(f, app, main_chunks[1]);

    // Content: Renderers | Queue
    render_content(f, app, main_chunks[2]);

    // Status bar
    render_status_bar(f, app, main_chunks[3]);

    // URL input popup (overlay on top of everything)
    if app.url_input_mode {
        render_url_popup(f, app, area);
    }
}

pub fn render_url_popup(f: &mut Frame, app: &App, area: Rect) {
    // Center the popup
    let popup_width = 60.min(area.width.saturating_sub(4));
    let popup_height = 3;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear background
    f.render_widget(Clear, popup_area);

    // Render URL input box
    let url_input = Paragraph::new(Line::from(vec![
        Span::styled(&app.url_input, Style::default().fg(colors::FG)),
        Span::styled("█", Style::default().fg(colors::ACCENT)),
    ]))
    .block(
        Block::default()
            .title(Span::styled(
                " Enter URL (Enter to play, Esc to cancel) ",
                Style::default()
                    .fg(colors::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER_FOCUS)),
    );
    f.render_widget(url_input, popup_area);
}

pub fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let device_name = app.selected_device().unwrap_or("No device");

    let state_color = match app.playback_state.as_str() {
        "Playing" => colors::PLAYING,
        "Paused" | "PausedPlayback" => colors::PAUSED,
        "Stopped" | "NoMediaPresent" => colors::STOPPED,
        _ => colors::DIM,
    };

    // Show pending operation if active, otherwise show playback state
    let (state_display, state_display_color) = if let Some(pending) = &app.pending_operation {
        (pending.clone(), colors::DIM) // Show "Pausing...", "Seeking..." in gray
    } else {
        let icon = match app.playback_state.as_str() {
            "Playing" => "▶",
            "Paused" | "PausedPlayback" => "⏸",
            "Stopped" | "NoMediaPresent" => "⏹",
            _ => "?",
        };
        (format!("{} {}", icon, app.playback_state), state_color)
    };

    // Volume indicator - use simple ASCII icons for better terminal compatibility
    let vol_str = if app.muted {
        "[MUTE]".to_string()
    } else {
        format!("Vol:{}%", app.volume)
    };
    let vol_color = if app.muted { colors::MUTED } else { colors::FG };

    // Current playing file (short)
    let current = app
        .current_uri
        .as_ref()
        .and_then(|u| u.rsplit('/').next())
        .unwrap_or("-");
    let current_short = if current.len() > 25 {
        &current[..25]
    } else {
        current
    };

    // Daemon uptime formatted
    let uptime_str = format_duration(Duration::from_secs(app.daemon_uptime));

    // Separator style
    let sep = Span::styled(" | ", Style::default().fg(colors::DIM));

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            format!(" {device_name} "),
            Style::default()
                .fg(colors::SELECTED)
                .add_modifier(Modifier::BOLD),
        ),
        sep.clone(),
        Span::styled(state_display, Style::default().fg(state_display_color)),
        sep.clone(),
        Span::styled(vol_str, Style::default().fg(vol_color)),
        sep.clone(),
        Span::styled(current_short, Style::default().fg(colors::FG)),
        sep.clone(),
        Span::styled(
            format!("Up:{} Streams:{}", uptime_str, app.daemon_streams),
            Style::default().fg(colors::DIM),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER_NORMAL))
            .title(Span::styled(
                " flingdlna ",
                Style::default()
                    .fg(colors::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )),
    );

    f.render_widget(header, area);
}

pub fn render_progress(f: &mut Frame, app: &App, area: Rect) {
    // Use interpolated position for smooth progress bar updates
    let pos = app.interpolated_position();
    let dur = app.playback_duration;

    let ratio = if dur > 0 {
        (pos as f64 / dur as f64).min(1.0)
    } else {
        0.0
    };

    let pos_str = format_duration(Duration::from_secs(pos));
    let dur_str = format_duration(Duration::from_secs(dur));

    let label = format!(" {pos_str} / {dur_str} ");

    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER_NORMAL)),
        )
        .gauge_style(
            Style::default()
                .fg(colors::PROGRESS_FG)
                .bg(colors::PROGRESS_BG),
        )
        .ratio(ratio)
        .label(Span::styled(
            label,
            Style::default().fg(colors::FG).add_modifier(Modifier::BOLD),
        ));

    f.render_widget(gauge, area);
}

pub fn render_content(f: &mut Frame, app: &App, area: Rect) {
    // Horizontal split: Renderers (17%) | Library (25%) | History (23%) | Queue (35%)
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(17),
            Constraint::Percentage(25),
            Constraint::Percentage(23),
            Constraint::Percentage(35),
        ])
        .split(area);

    // Left: Renderers
    render_renderers(f, app, chunks[0]);

    // Middle-left: Library + File info
    let library_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(4)])
        .split(chunks[1]);

    render_library(f, app, library_chunks[0]);
    render_file_info(f, app, library_chunks[1]);

    // Middle-right: History
    render_history(f, app, chunks[2]);

    // Right: Queue (with optional search)
    render_queue(f, app, chunks[3]);
}

pub fn render_renderers(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .renderers
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let selected = app.selected_renderer.selected() == Some(i);
            let style = if selected {
                Style::default()
                    .fg(colors::SELECTED)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors::FG)
            };
            let marker = if selected { "▸ " } else { "  " };

            // Show model/manufacturer on second line for selected device
            if selected && (r.model.is_some() || r.manufacturer.is_some()) {
                let model_info = match (&r.manufacturer, &r.model) {
                    (Some(mfr), Some(mdl)) => format!("    {mfr} {mdl}"),
                    (Some(mfr), None) => format!("    {mfr}"),
                    (None, Some(mdl)) => format!("    {mdl}"),
                    (None, None) => String::new(),
                };
                ListItem::new(vec![
                    Line::from(vec![Span::styled(format!("{}{}", marker, &r.name), style)]),
                    Line::from(vec![Span::styled(
                        model_info,
                        Style::default().fg(colors::DIM),
                    )]),
                ])
            } else {
                ListItem::new(Line::from(vec![Span::styled(
                    format!("{}{}", marker, &r.name),
                    style,
                )]))
            }
        })
        .collect();

    let border_color = if app.focus == Focus::Renderers {
        colors::BORDER_FOCUS
    } else {
        colors::BORDER_NORMAL
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(Span::styled(
                    " Devices ",
                    Style::default().fg(colors::ACCENT),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_stateful_widget(list, area, &mut app.selected_renderer.clone());
}

pub fn render_library(f: &mut Frame, app: &App, area: Rect) {
    use super::types::LibraryEntry;

    let items: Vec<ListItem> = app
        .library_entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let selected = app.selected_library.selected() == Some(i);
            let style = if selected {
                Style::default()
                    .fg(colors::SELECTED)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors::FG)
            };
            let marker = if selected { "▸ " } else { "  " };

            match entry {
                LibraryEntry::Folder {
                    name, file_count, ..
                } => {
                    // Folder entry with icon and file count
                    let name_display = if name.len() > 18 {
                        format!("{}...", &name[..15])
                    } else {
                        name.clone()
                    };

                    ListItem::new(Line::from(vec![
                        Span::styled(marker, style),
                        Span::styled("📁 ", Style::default().fg(colors::ACCENT)),
                        Span::styled(name_display, style),
                        Span::styled(format!(" ({file_count})"), Style::default().fg(colors::DIM)),
                    ]))
                }
                LibraryEntry::File(file) => {
                    // Format file size
                    let size_str = if file.size >= 1024 * 1024 * 1024 {
                        format!("{:.1}G", file.size as f64 / (1024.0 * 1024.0 * 1024.0))
                    } else if file.size >= 1024 * 1024 {
                        format!("{:.1}M", file.size as f64 / (1024.0 * 1024.0))
                    } else {
                        format!("{:.0}K", file.size as f64 / 1024.0)
                    };

                    // Format duration
                    let dur_str = file
                        .duration_secs
                        .map(|d| format_duration(Duration::from_secs(d)))
                        .unwrap_or_default();

                    // Truncate filename if too long
                    let name = if file.filename.len() > 20 {
                        format!("{}...", &file.filename[..17])
                    } else {
                        file.filename.clone()
                    };

                    let info_str = if dur_str.is_empty() {
                        format!(" [{size_str}]")
                    } else {
                        format!(" [{dur_str}|{size_str}]")
                    };

                    ListItem::new(Line::from(vec![
                        Span::styled(marker, style),
                        Span::styled(name, style),
                        Span::styled(info_str, Style::default().fg(colors::DIM)),
                    ]))
                }
            }
        })
        .collect();

    let border_color = if app.focus == Focus::Library {
        colors::BORDER_FOCUS
    } else {
        colors::BORDER_NORMAL
    };

    // Title with breadcrumb path
    let breadcrumb = app.library_breadcrumb();
    let title = if breadcrumb == "/" {
        format!(" Library ({}) ", app.library_entries.len())
    } else {
        // Truncate breadcrumb if too long
        let bc = if breadcrumb.len() > 15 {
            format!("...{}", &breadcrumb[breadcrumb.len() - 12..])
        } else {
            breadcrumb
        };
        format!(" {} ({}) ", bc, app.library_entries.len())
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(Span::styled(title, Style::default().fg(colors::ACCENT)))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_stateful_widget(list, area, &mut app.selected_library.clone());
}

pub fn render_file_info(f: &mut Frame, app: &App, area: Rect) {
    let file = app.selected_file();

    let lines = if let Some(file) = file {
        // Format path (truncate if too long)
        let path_str = file.path.to_string_lossy();
        let path_display = if path_str.len() > 45 {
            format!("...{}", &path_str[path_str.len() - 42..])
        } else {
            path_str.to_string()
        };

        // Format size
        let size_str = if file.size >= 1024 * 1024 * 1024 {
            format!("{:.2} GB", file.size as f64 / (1024.0 * 1024.0 * 1024.0))
        } else if file.size >= 1024 * 1024 {
            format!("{:.1} MB", file.size as f64 / (1024.0 * 1024.0))
        } else {
            format!("{} KB", file.size / 1024)
        };

        vec![
            Line::from(vec![
                Span::styled(" 📁 ", Style::default().fg(colors::ACCENT)),
                Span::styled(path_display, Style::default().fg(colors::DIM)),
            ]),
            Line::from(vec![
                Span::styled(" 📊 ", Style::default().fg(colors::ACCENT)),
                Span::styled(size_str, Style::default().fg(colors::FG)),
                Span::styled("  ", Style::default()),
                Span::styled(file.mime_type.clone(), Style::default().fg(colors::DIM)),
            ]),
        ]
    } else {
        vec![Line::from(Span::styled(
            " No file selected",
            Style::default().fg(colors::DIM),
        ))]
    };

    let info = Paragraph::new(lines).block(
        Block::default()
            .title(Span::styled(
                " File Info ",
                Style::default().fg(colors::ACCENT),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER_NORMAL)),
    );
    f.render_widget(info, area);
}

pub fn render_history(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .history_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let selected = app.selected_history.selected() == Some(i);
            let style = if selected {
                Style::default()
                    .fg(colors::SELECTED)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(colors::FG)
            };
            let marker = if selected { "▸ " } else { "  " };

            // Format position/duration as progress percentage
            let progress = if item.duration_secs > 0 {
                (item.position_secs as f64 / item.duration_secs as f64 * 100.0) as u8
            } else {
                0
            };

            // Format position time
            let pos_str = format_duration(Duration::from_secs(item.position_secs));
            let dur_str = format_duration(Duration::from_secs(item.duration_secs));

            // Truncate title if too long
            let title = if item.title.len() > 16 {
                format!("{}...", &item.title[..13])
            } else {
                item.title.clone()
            };

            // Time ago
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let ago = now.saturating_sub(item.saved_at);
            let ago_str = if ago < 60 {
                "just now".to_string()
            } else if ago < 3600 {
                format!("{}m ago", ago / 60)
            } else if ago < 86400 {
                format!("{}h ago", ago / 3600)
            } else {
                format!("{}d ago", ago / 86400)
            };

            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(marker, style),
                    Span::styled(title, style),
                ]),
                Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        format!("{pos_str}/{dur_str} ({progress}%)"),
                        Style::default().fg(colors::DIM),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(ago_str, Style::default().fg(colors::DIM)),
                ]),
            ])
        })
        .collect();

    let border_color = if app.focus == Focus::History {
        colors::BORDER_FOCUS
    } else {
        colors::BORDER_NORMAL
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(Span::styled(
                    format!(" History ({}) ", app.history_items.len()),
                    Style::default().fg(colors::ACCENT),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_stateful_widget(list, area, &mut app.selected_history.clone());
}

pub fn render_queue(f: &mut Frame, app: &App, area: Rect) {
    // If search mode, show search input at top
    let (queue_area, search_area) = if app.search_mode {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(area);
        (chunks[1], Some(chunks[0]))
    } else {
        (area, None)
    };

    // Search input
    if let Some(search_rect) = search_area {
        let search = Paragraph::new(Line::from(vec![
            Span::styled(" / ", Style::default().fg(colors::ACCENT)),
            Span::styled(&app.search_query, Style::default().fg(colors::FG)),
            Span::styled("█", Style::default().fg(colors::ACCENT)), // cursor
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(colors::BORDER_FOCUS))
                .style(Style::default().bg(colors::SEARCH_BG)),
        );
        f.render_widget(search, search_rect);
    }

    // Queue items
    let display_items: Vec<&super::types::QueueDisplayItem> =
        if app.search_mode && !app.search_query.is_empty() {
            app.filtered_queue_indices
                .iter()
                .filter_map(|&i| app.queue_items.get(i))
                .collect()
        } else {
            app.queue_items.iter().collect()
        };

    let items: Vec<ListItem> = display_items
        .iter()
        .map(|item| {
            let is_current = app.queue_current == Some(item.index);
            let dur = item
                .duration
                .map(|d| format_duration(Duration::from_secs(d)))
                .unwrap_or_else(|| "--:--".to_string());

            let (marker, style) = if is_current {
                (
                    "▶ ",
                    Style::default()
                        .fg(colors::QUEUE_CURRENT)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ("  ", Style::default().fg(colors::FG))
            };

            // Add URL marker if this is a URL item
            let type_marker = if item.is_url { "[URL] " } else { "" };

            ListItem::new(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(
                    format!("{:2}.", item.index),
                    Style::default().fg(colors::DIM),
                ),
                Span::raw(" "),
                Span::styled(type_marker, Style::default().fg(colors::ACCENT)),
                Span::styled(&item.title, style),
                Span::raw(" "),
                Span::styled(format!("[{dur}]"), Style::default().fg(colors::DIM)),
            ]))
        })
        .collect();

    // Queue title with modes
    let shuffle_str = if app.shuffle { "🔀" } else { "" };
    let repeat_str = match app.repeat.as_str() {
        "one" => "🔂",
        "all" => "🔁",
        _ => "",
    };

    let title = format!(
        " Queue ({}) {} {} ",
        display_items.len(),
        shuffle_str,
        repeat_str
    );

    let border_color = if app.focus == Focus::Queue {
        colors::BORDER_FOCUS
    } else {
        colors::BORDER_NORMAL
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(Span::styled(title, Style::default().fg(colors::ACCENT)))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_stateful_widget(list, queue_area, &mut app.selected_queue.clone());
}

pub fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let help = if app.search_mode {
        "Enter:confirm | Esc:cancel | Type to search"
    } else {
        "Space:⏯ | ←→:Seek | n/p:Track | +/-:Vol | /:Search | ?:Help | q:Quit"
    };

    // Build spans - show action feedback if present
    let mut spans = Vec::new();

    if let Some(action) = &app.last_action {
        // Show action feedback with bright highlight
        spans.push(Span::styled(
            format!(" {action} "),
            Style::default()
                .fg(colors::PLAYING)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(" │ ", Style::default().fg(colors::DIM)));
    }

    spans.push(Span::styled(
        format!(" {} ", &app.status_message),
        Style::default()
            .fg(colors::SELECTED)
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(" │ ", Style::default().fg(colors::DIM)));
    spans.push(Span::styled(help, Style::default().fg(colors::DIM)));

    let status = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(colors::BORDER_NORMAL)),
    );

    f.render_widget(status, area);
}
