//! Home screen rendering — session list with status icons

use crate::app::{App, Overlay};
use ratatui::prelude::*;
use ratatui::widgets::*;

/// Main render function for the home screen
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Layout: header (1), body (fill), footer (1)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, chunks[0], &app.theme);
    render_session_list(frame, chunks[1], app);
    crate::ui::footer::render(frame, chunks[2], app);

    // Render overlay on top if active
    match &app.overlay {
        Overlay::NewSession(form) => {
            crate::ui::overlay::render_new_session(frame, area, form, &app.theme);
        }
        Overlay::Confirm(dialog) => {
            crate::ui::overlay::render_confirm(frame, area, dialog, &app.theme);
        }
        Overlay::None => {}
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

    let items: Vec<ListItem> = app
        .list_rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let is_selected = i == app.selected_index;
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
                    let status_color = crate::ui::theme::status_color(theme, session.status);
                    let notify_indicator = if session.notify { " !" } else { "  " };
                    let follow_up_indicator = if session.follow_up { "F " } else { "  " };
                    let age = format_age(session.created_at);

                    let line = Line::from(vec![
                        Span::raw(follow_up_indicator),
                        Span::styled(
                            format!("   {} ", session.status.icon()),
                            Style::default().fg(status_color),
                        ),
                        Span::styled(notify_indicator, Style::default().fg(theme.warning)),
                        Span::styled(session.title.clone(), Style::default().fg(theme.text).bold()),
                        Span::raw("  "),
                        Span::styled(
                            truncate_path(&session.project_path, 30),
                            Style::default().fg(theme.text_muted),
                        ),
                        Span::raw("  "),
                        Span::styled(age, Style::default().fg(theme.text_muted)),
                    ]);

                    let bg = if is_selected {
                        theme.background_element
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
