//! Session list panel rendering

use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::core::groups::ListRow;
use crate::ui::theme::status_color;

pub(super) fn render_session_list(frame: &mut Frame, area: Rect, app: &App) {
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
                ListRow::Group {
                    group,
                    session_count,
                    running_count,
                    waiting_count,
                } => {
                    let arrow = if group.expanded {
                        "\u{25BC}"
                    } else {
                        "\u{25B6}"
                    };
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
                                .fg(if is_selected {
                                    theme.selected_item_text
                                } else {
                                    theme.text
                                })
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
                ListRow::Session(session) => {
                    let is_bulk_selected = app.bulk.selected.contains(&session.id);
                    let status_color = status_color(theme, session.status);
                    let notify_indicator = if session.notify { "\u{266A}" } else { " " };
                    let follow_up_indicator = if session.follow_up { "\u{2691}" } else { " " };
                    let pin_indicator = if session.pinned { "\u{25B4}" } else { " " };
                    let age = format_age(session.last_started_at);
                    let sparkline =
                        super::sparkline::render_sparkline_str(&session.status_history, 16);

                    // When this session matches the search, highlight the title in the info color
                    let title_color = if is_search_match {
                        theme.info
                    } else {
                        theme.text
                    };

                    // Build left side: indicators + status + title + path
                    let left_prefix = format!(" {}", pin_indicator);
                    let status_str = format!(" {} ", session.status.icon());
                    let path_str = truncate_path(&session.project_path, 30);

                    // Build right side: sparkline + age (right-justified)
                    let right_str = if sparkline.is_empty() {
                        format!("{} ", age)
                    } else {
                        format!("{} {} ", sparkline, age)
                    };
                    let right_width = right_str.chars().count();

                    // Calculate left content width to determine padding
                    let left_width = left_prefix.chars().count()
                        + 1 // follow_up_indicator
                        + 1 // notify_indicator
                        + status_str.chars().count()
                        + session.title.chars().count()
                        + 2 // "  " gap
                        + path_str.chars().count();

                    let row_width = area.width as usize;
                    let pad = if left_width + right_width < row_width {
                        row_width - left_width - right_width
                    } else {
                        2
                    };

                    let line = Line::from(vec![
                        Span::styled(left_prefix, Style::default().fg(theme.accent)),
                        Span::styled(follow_up_indicator, Style::default().fg(theme.warning)),
                        Span::styled(notify_indicator, Style::default().fg(theme.info)),
                        Span::styled(status_str, Style::default().fg(status_color)),
                        Span::styled(
                            session.title.clone(),
                            Style::default().fg(title_color).bold(),
                        ),
                        Span::raw("  "),
                        Span::styled(path_str, Style::default().fg(theme.text_muted)),
                        Span::raw(" ".repeat(pad)),
                        Span::styled(right_str, Style::default().fg(theme.text_muted)),
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
        use crate::types::SessionStatus;
        use ratatui::style::Color;
        let theme = crate::ui::theme::Theme::dark();
        let statuses = [
            SessionStatus::Running,
            SessionStatus::Waiting,
            SessionStatus::Paused,
            SessionStatus::Error,
        ];
        let colors: Vec<Color> = statuses.iter().map(|s| status_color(&theme, *s)).collect();
        // Running, Waiting, Paused, Error should all be different colors
        for i in 0..colors.len() {
            for j in i + 1..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
    }
}
