//! Home screen rendering — session list with status icons

use crate::app::{App, Overlay};
use ratatui::prelude::*;
use ratatui::widgets::*;

/// Main render function for the home screen
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // When the terminal is wide enough, split horizontally: list on left, detail on right
    let (list_area, detail_area) =
        if area.width >= crate::ui::detail::DETAIL_PANEL_MIN_WIDTH {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(0), Constraint::Length(36)])
                .split(area);
            (cols[0], Some(cols[1]))
        } else {
            (area, None)
        };

    // Layout: header (1), body (fill), activity feed (0 or 4), footer (1)
    let show_feed = app.show_activity_feed && !app.activity_feed.is_empty();
    let feed_height = if show_feed { 4 } else { 0 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(feed_height),
            Constraint::Length(1),
        ])
        .split(list_area);

    render_header(frame, chunks[0], &app.theme);
    render_session_list(frame, chunks[1], app);
    if show_feed {
        render_activity_feed(frame, chunks[2], app);
    }
    if let Some(ref query) = app.search_query {
        let matches = app.search_matches();
        let match_count = matches.len();
        let search_line = Line::from(vec![
            Span::styled(" / ", Style::default().fg(app.theme.primary).bold()),
            Span::styled(query.as_str(), Style::default().fg(app.theme.text)),
            Span::styled("\u{2588}", Style::default().fg(app.theme.primary)),
            Span::styled(
                format!(
                    "  {} match{}",
                    match_count,
                    if match_count == 1 { "" } else { "es" }
                ),
                Style::default().fg(app.theme.text_muted),
            ),
        ]);
        frame.render_widget(Paragraph::new(search_line), chunks[3]);
    } else {
        crate::ui::footer::render(frame, chunks[3], app);
    }

    // Render detail panel for selected session when wide enough
    if let Some(detail_rect) = detail_area {
        if let Some(session) = app.selected_session() {
            crate::ui::detail::render(frame, detail_rect, session, &app.theme);
        }
    }

    // Render overlay on top if active
    match &app.overlay {
        Overlay::NewSession(form) => {
            crate::ui::overlay::render_new_session(frame, area, form, &app.theme);
        }
        Overlay::Confirm(dialog) => {
            crate::ui::overlay::render_confirm(frame, area, dialog, &app.theme);
        }
        Overlay::Rename(form) => {
            crate::ui::overlay::render_rename(frame, area, form, &app.theme);
        }
        Overlay::Move(form) => {
            crate::ui::overlay::render_move(frame, area, form, &app.theme);
        }
        Overlay::GroupManage(form) => {
            crate::ui::overlay::render_group_manage(frame, area, form, &app.theme);
        }
        Overlay::CommandPalette(palette) => {
            crate::ui::overlay::render_command_palette(frame, area, palette, &app.theme);
        }
        Overlay::Help => {
            crate::ui::overlay::render_help(frame, area, &app.theme);
        }
        Overlay::None => {}
    }
}

fn render_activity_feed(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let block = Block::default()
        .title(" Activity ")
        .title_style(Style::default().fg(theme.text_muted))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = app
        .activity_feed
        .iter()
        .take(inner.height as usize)
        .map(|event| {
            let status_color = crate::ui::theme::status_color(theme, event.new_status);
            Line::from(vec![
                Span::styled(
                    format_activity_age(event.timestamp),
                    Style::default().fg(theme.text_muted),
                ),
                Span::styled(
                    format!(" {} ", event.session_title),
                    Style::default().fg(theme.text),
                ),
                Span::styled("-> ", Style::default().fg(theme.text_muted)),
                Span::styled(
                    event.new_status.as_str(),
                    Style::default().fg(status_color),
                ),
            ])
        })
        .collect();

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn format_activity_age(timestamp: i64) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let ago_ms = now - timestamp;
    if ago_ms < 60_000 {
        " <1m ".to_string()
    } else if ago_ms < 3_600_000 {
        format!(" {}m  ", ago_ms / 60_000)
    } else {
        format!(" {}h  ", ago_ms / 3_600_000)
    }
}

fn render_header(frame: &mut Frame, area: Rect, theme: &crate::ui::theme::Theme) {
    let version = env!("CARGO_PKG_VERSION");
    let header = Line::from(vec![
        Span::styled("agent-view ", Style::default().fg(theme.primary).bold()),
        Span::styled(format!("v{}", version), Style::default().fg(theme.text_muted)),
    ]);
    frame.render_widget(Paragraph::new(header), area);
}

fn render_session_list(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;

    if app.list_rows.is_empty() {
        let msg = Paragraph::new("No sessions. Press 'n' to create one.")
            .style(Style::default().fg(theme.text_muted))
            .alignment(Alignment::Center);
        frame.render_widget(msg, area);
        return;
    }

    let search_matches = app.search_matches();

    let items: Vec<ListItem> = app
        .list_rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let is_selected = i == app.selected_index;
            let is_search_match = !search_matches.is_empty() && search_matches.contains(&i);
            match row {
                crate::core::groups::ListRow::Group {
                    group,
                    session_count,
                    running_count,
                    waiting_count,
                } => {
                    let arrow = if group.expanded { "\u{25BC}" } else { "\u{25B6}" };
                    let mut spans = vec![
                        Span::styled(
                            format!(" {} ", arrow),
                            Style::default().fg(if is_selected {
                                theme.selected_item_text
                            } else {
                                theme.accent
                            }),
                        ),
                        Span::styled(
                            group.name.clone(),
                            Style::default()
                                .fg(if is_selected { theme.selected_item_text } else { theme.text })
                                .bold(),
                        ),
                        Span::styled(
                            format!("  ({})", session_count),
                            Style::default().fg(if is_selected {
                                theme.selected_item_text
                            } else {
                                theme.text_muted
                            }),
                        ),
                    ];

                    if *running_count > 0 {
                        spans.push(Span::styled(
                            format!("  \u{25CF}{}", running_count),
                            Style::default().fg(if is_selected {
                                theme.selected_item_text
                            } else {
                                theme.success
                            }),
                        ));
                    }
                    if *waiting_count > 0 {
                        spans.push(Span::styled(
                            format!("  \u{25D0}{}", waiting_count),
                            Style::default().fg(if is_selected {
                                theme.selected_item_text
                            } else {
                                theme.warning
                            }),
                        ));
                    }

                    let bg = if is_selected {
                        theme.primary
                    } else {
                        theme.background_element
                    };
                    ListItem::new(Line::from(spans)).style(Style::default().bg(bg))
                }
                crate::core::groups::ListRow::Session(session) => {
                    let is_bulk_selected = app.bulk_selected.contains(&session.id);
                    let status_color = crate::ui::theme::status_color(theme, session.status);
                    let notify_indicator = if session.notify { " !" } else { "  " };
                    let follow_up_indicator = if session.follow_up { "F " } else { "  " };
                    let pin_indicator = if session.pinned { "^ " } else { "  " };
                    let age = format_age(session.created_at);
                    let sparkline = render_sparkline_str(&session.status_history, 8);

                    // When this session matches the search, highlight the title in the info color
                    let title_color = if is_search_match {
                        theme.info
                    } else {
                        theme.text
                    };

                    let line = Line::from(vec![
                        Span::styled(pin_indicator, Style::default().fg(theme.accent)),
                        Span::raw(follow_up_indicator),
                        Span::styled(
                            format!("   {} ", session.status.icon()),
                            Style::default().fg(status_color),
                        ),
                        Span::styled(notify_indicator, Style::default().fg(theme.warning)),
                        Span::styled(session.title.clone(), Style::default().fg(title_color).bold()),
                        Span::raw("  "),
                        Span::styled(
                            truncate_path(&session.project_path, 30),
                            Style::default().fg(theme.text_muted),
                        ),
                        Span::raw("  "),
                        Span::styled(
                            format!(" {} ", sparkline),
                            Style::default().fg(theme.text_muted),
                        ),
                        Span::styled(age, Style::default().fg(theme.text_muted)),
                    ]);

                    let bg = if is_selected {
                        theme.background_element
                    } else if is_bulk_selected {
                        theme.secondary
                    } else {
                        theme.background
                    };
                    ListItem::new(line).style(Style::default().bg(bg))
                }
            }
        })
        .collect();

    let list = List::new(items);
    frame.render_widget(list, area);
}

fn status_to_sparkline_char(status: &str) -> char {
    match status {
        "idle" | "stopped" => '\u{2581}',     // ▁
        "running" => '\u{2588}',              // █
        "waiting" => '\u{2585}',              // ▅
        "paused" | "compacting" => '\u{2583}', // ▃
        "error" => '\u{2587}',               // ▇
        _ => '\u{2581}',                      // ▁
    }
}

fn render_sparkline_str(history: &[crate::types::StatusHistoryEntry], max_width: usize) -> String {
    if history.is_empty() {
        return String::new();
    }
    let start = if history.len() > max_width {
        history.len() - max_width
    } else {
        0
    };
    history[start..]
        .iter()
        .map(|entry| status_to_sparkline_char(&entry.status))
        .collect()
}

/// Format a millisecond timestamp as a human-readable age
fn format_age(created_at_ms: i64) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let diff_ms = now - created_at_ms;
    if diff_ms < 0 {
        return "just now".to_string();
    }

    let seconds = diff_ms / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;

    if days > 0 {
        format!("{}d", days)
    } else if hours > 0 {
        format!("{}h", hours)
    } else if minutes > 0 {
        format!("{}m", minutes)
    } else {
        "just now".to_string()
    }
}

/// Truncate a path to fit within max_len, keeping the end
fn truncate_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        path.to_string()
    } else {
        let start = path.len() - max_len + 1;
        format!("~{}", &path[start..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SessionStatus;

    #[test]
    fn test_format_age_days() {
        let now = chrono::Utc::now().timestamp_millis();
        let two_days_ago = now - 2 * 24 * 60 * 60 * 1000;
        assert_eq!(format_age(two_days_ago), "2d");
    }

    #[test]
    fn test_format_age_hours() {
        let now = chrono::Utc::now().timestamp_millis();
        let three_hours_ago = now - 3 * 60 * 60 * 1000;
        assert_eq!(format_age(three_hours_ago), "3h");
    }

    #[test]
    fn test_format_age_minutes() {
        let now = chrono::Utc::now().timestamp_millis();
        let five_min_ago = now - 5 * 60 * 1000;
        assert_eq!(format_age(five_min_ago), "5m");
    }

    #[test]
    fn test_format_age_just_now() {
        let now = chrono::Utc::now().timestamp_millis();
        assert_eq!(format_age(now), "just now");
    }

    #[test]
    fn test_truncate_path_short() {
        assert_eq!(truncate_path("/tmp/test", 30), "/tmp/test");
    }

    #[test]
    fn test_truncate_path_long() {
        let long_path = "/Users/mdoyle/projects/very-long-project-name/src";
        let result = truncate_path(long_path, 20);
        assert!(result.starts_with('~'));
        assert!(result.len() <= 20);
    }

    #[test]
    fn test_sparkline_from_history() {
        use crate::types::StatusHistoryEntry;
        let history = vec![
            StatusHistoryEntry { status: "idle".to_string(), timestamp: 1000 },
            StatusHistoryEntry { status: "running".to_string(), timestamp: 2000 },
            StatusHistoryEntry { status: "waiting".to_string(), timestamp: 3000 },
            StatusHistoryEntry { status: "idle".to_string(), timestamp: 4000 },
        ];
        let spark = render_sparkline_str(&history, 4);
        assert_eq!(spark, "\u{2581}\u{2588}\u{2585}\u{2581}");
    }

    #[test]
    fn test_sparkline_empty_history() {
        let spark = render_sparkline_str(&[], 4);
        assert_eq!(spark, "");
    }

    #[test]
    fn test_sparkline_truncates_to_max_width() {
        use crate::types::StatusHistoryEntry;
        let history: Vec<StatusHistoryEntry> = (0..20)
            .map(|i| StatusHistoryEntry {
                status: "running".to_string(),
                timestamp: i * 1000,
            })
            .collect();
        let spark = render_sparkline_str(&history, 8);
        assert_eq!(spark.chars().count(), 8);
    }

    #[test]
    fn test_status_colors_are_distinct() {
        let theme = crate::ui::theme::Theme::dark();
        let statuses = [
            SessionStatus::Running,
            SessionStatus::Waiting,
            SessionStatus::Paused,
            SessionStatus::Error,
        ];
        let colors: Vec<Color> = statuses
            .iter()
            .map(|s| crate::ui::theme::status_color(&theme, *s))
            .collect();
        // Running, Waiting, Paused, Error should all be different colors
        for i in 0..colors.len() {
            for j in i + 1..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }
}
