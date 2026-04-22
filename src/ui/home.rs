//! Home screen rendering — session list with status icons

use crate::app::{App, Overlay};
use ratatui::prelude::*;
use ratatui::widgets::*;

const LOGO: [&str; 4] = [
    r"  __    ___  ____  __ _  ____    _  _  __  ____  _  _ ",
    r" / _\  / __)(  __)(  ( \(_  _)  / )( \(  )(  __)/ )( \",
    r"/    \( (_ \ ) _) /    /  )(    \ \/ / )(  ) _) \ /\ /",
    r"\_/\_/ \___/(____)\_)__) (__)    \__/ (__)(____)(_/\_)",
];

/// Main render function for the home screen
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Fill entire screen with theme background so light theme works properly
    frame.render_widget(
        Block::default().style(Style::default().bg(app.theme.background)),
        area,
    );

    // When the terminal is wide enough, split horizontally: list on left, detail on right
    let detail_width = crate::ui::detail::panel_width(app.detail_mode, area.width);
    let (list_area, detail_area) =
        if area.width >= crate::ui::detail::DETAIL_PANEL_MIN_WIDTH && detail_width > 0 {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(0), Constraint::Length(detail_width)])
                .split(area);
            (cols[0], Some(cols[1]))
        } else {
            (area, None)
        };

    // Layout: header, body, activity feed, usage pane, footer
    let show_feed = app.show_activity_feed && !app.activity_feed.is_empty();
    let feed_height = if show_feed {
        // 1 for border + 1 per event, capped at 8 lines total
        let events = app.activity_feed.len().min(7) as u16;
        events + 1
    } else {
        0
    };
    let has_usage = app.usage_data.is_some();
    let usage_height = if has_usage { 4u16 } else { 0 }; // 1 border + 3 rows

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),            // ASCII header + tab bar
            Constraint::Min(0),               // session/routine list
            Constraint::Length(feed_height),  // activity feed
            Constraint::Length(usage_height), // usage pane
            Constraint::Length(1),            // footer
        ])
        .split(list_area);

    render_header(frame, chunks[0], app);
    match app.active_tab {
        crate::app::ActiveTab::Sessions => render_session_list(frame, chunks[1], app),
        crate::app::ActiveTab::Routines => {
            crate::ui::routines::render_routine_list(frame, chunks[1], app)
        }
    }
    if show_feed {
        render_activity_feed(frame, chunks[2], app);
    }
    if has_usage {
        render_usage_pane(frame, chunks[3], app);
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
        frame.render_widget(Paragraph::new(search_line), chunks[4]);
    } else {
        crate::ui::footer::render(frame, chunks[4], app);
    }

    // Render detail panel when wide enough
    if let Some(detail_rect) = detail_area {
        match app.active_tab {
            crate::app::ActiveTab::Sessions => {
                if let Some(session) = app.selected_session() {
                    crate::ui::detail::render_detail_panel(
                        frame,
                        detail_rect,
                        session,
                        &app.theme,
                        app.detail_mode,
                        &app.preview_content,
                    );
                }
            }
            crate::app::ActiveTab::Routines => {
                match app.routine_list_rows.get(app.routine_selected_index) {
                    Some(crate::app::RoutineListRow::Routine(routine)) => {
                        crate::ui::detail::render_routine_detail(
                            frame,
                            detail_rect,
                            routine,
                            &app.theme,
                            app.detail_mode,
                            &app.preview_content,
                        );
                    }
                    Some(crate::app::RoutineListRow::Run { run, routine_name }) => {
                        crate::ui::detail::render_run_detail(
                            frame,
                            detail_rect,
                            run,
                            routine_name,
                            &app.theme,
                            app.detail_mode,
                            &app.preview_content,
                        );
                    }
                    _ => {}
                }
            }
        }
    }

    // Render overlay on top if active
    match &app.overlay {
        Overlay::NewSession(form) => {
            crate::ui::overlay::render_new_session(frame, area, form, &app.theme);
        }
        Overlay::NewRoutine(form) => {
            crate::ui::overlay::render_new_routine(frame, area, form, &app.theme);
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
            crate::ui::overlay::render_help(frame, area, app);
        }
        Overlay::ThemeSelect(form) => {
            crate::ui::overlay::render_theme_select(frame, area, form, &app.theme);
        }
        Overlay::AddNote(form) => {
            crate::ui::overlay::render_add_note(frame, area, form, &app.theme);
        }
        Overlay::RoutineWarning => {
            crate::ui::overlay::render_routine_warning(frame, area, &app.theme);
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
                Span::styled(event.new_status.as_str(), Style::default().fg(status_color)),
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

fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let version = env!("CARGO_PKG_VERSION");

    let logo_lines: &[&str] = &LOGO;

    let theme = &app.theme;
    let active_tab = app.active_tab;
    let primary_style = Style::default().fg(theme.primary).bold();
    let muted_style = Style::default().fg(theme.text_muted);

    let area_width = area.width as usize;
    let mut lines: Vec<Line> = logo_lines
        .iter()
        .map(|line| {
            let pad = area_width.saturating_sub(line.len()) / 2;
            Line::from(Span::styled(
                format!("{:>width$}{}", "", line, width = pad),
                primary_style,
            ))
        })
        .collect();
    lines.push(Line::from(""));

    // Tab bar line
    let tab_line = Line::from(vec![
        Span::styled("  ", muted_style),
        Span::styled(
            " Sessions ",
            if active_tab == crate::app::ActiveTab::Sessions {
                Style::default()
                    .fg(theme.selected_item_text)
                    .bg(theme.primary)
                    .bold()
            } else {
                muted_style
            },
        ),
        Span::styled(" ", muted_style),
        Span::styled(
            " Routines ",
            if active_tab == crate::app::ActiveTab::Routines {
                Style::default()
                    .fg(theme.selected_item_text)
                    .bg(theme.primary)
                    .bold()
            } else {
                muted_style
            },
        ),
        Span::styled(
            format!("  v{}", version),
            Style::default().fg(theme.text_muted),
        ),
    ]);
    lines.push(tab_line);

    frame.render_widget(Paragraph::new(lines), area);
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
                crate::core::groups::ListRow::Session(session) => {
                    let is_bulk_selected = app.bulk_selected.contains(&session.id);
                    let status_color = crate::ui::theme::status_color(theme, session.status);
                    let notify_indicator = if session.notify { "\u{266A}" } else { " " };
                    let follow_up_indicator = if session.follow_up { "\u{2691}" } else { " " };
                    let pin_indicator = if session.pinned { "\u{25B4}" } else { " " };
                    let age = format_age(session.last_started_at);
                    let sparkline = render_sparkline_str(&session.status_history, 16);

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

/// Activity level for a status — higher = more active
fn status_activity_level(status: &str) -> u8 {
    match status {
        "running" => 5,
        "waiting" => 4,
        "error" => 3,
        "draft" | "paused" | "compacting" => 2,
        "idle" | "stopped" => 1,
        _ => 0,
    }
}

fn activity_level_to_char(level: u8) -> char {
    match level {
        5 => '\u{2586}', // ▆  running
        4 => '\u{2584}', // ▄  waiting
        3 => '\u{2585}', // ▅  error
        2 => '\u{2582}', // ▂  paused
        1 => '\u{2581}', // ▁  idle
        _ => ' ',        //    no data
    }
}

/// Render a time-bucketed sparkline over the last 24 hours.
/// Each character represents ~90 minutes. The dominant (most active)
/// status in each bucket determines the bar height.
fn render_sparkline_str(history: &[crate::types::StatusHistoryEntry], buckets: usize) -> String {
    render_sparkline_at(history, buckets, chrono::Utc::now().timestamp_millis())
}

/// Testable version that accepts a custom "now" timestamp.
fn render_sparkline_at(
    history: &[crate::types::StatusHistoryEntry],
    buckets: usize,
    now_ms: i64,
) -> String {
    if history.is_empty() {
        return String::new();
    }

    let window_ms: i64 = 24 * 60 * 60 * 1000; // 24 hours
    let start_ms = now_ms - window_ms;
    let bucket_ms = window_ms / buckets as i64;

    // Check if any history falls within the window
    let has_recent = history.iter().any(|e| e.timestamp >= start_ms);
    if !has_recent {
        // All activity is older than 24h — show nothing
        return String::new();
    }

    // For each bucket, find the highest activity level.
    // A status entry represents the state AT that timestamp and persists
    // until the next entry. So we need to figure out what status was active
    // during each bucket.
    let mut result = String::with_capacity(buckets);

    for b in 0..buckets {
        let bucket_start = start_ms + b as i64 * bucket_ms;
        let bucket_end = bucket_start + bucket_ms;

        // Find the highest activity status that overlaps this bucket.
        // An entry at timestamp T is active from T until the next entry's timestamp.
        let mut max_level: u8 = 0;

        for (idx, entry) in history.iter().enumerate() {
            let entry_start = entry.timestamp;
            let entry_end = if idx + 1 < history.len() {
                history[idx + 1].timestamp
            } else {
                now_ms // last entry extends to now
            };

            // Does this entry overlap the bucket?
            if entry_start < bucket_end && entry_end > bucket_start {
                let level = status_activity_level(&entry.status);
                if level > max_level {
                    max_level = level;
                }
            }
        }

        result.push(activity_level_to_char(max_level));
    }

    result
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

fn render_usage_pane(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;

    let block = Block::default()
        .title(" Usage ")
        .title_style(Style::default().fg(theme.text_muted))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let usage = match app.usage_data {
        Some(ref u) => u,
        None => return,
    };

    let buckets: Vec<(&str, Option<&crate::types::UsageBucket>)> = vec![
        ("Session", usage.session.as_ref()),
        ("Week", usage.week_all.as_ref()),
        ("Sonnet", usage.week_sonnet.as_ref()),
    ];

    // Pre-compute reset strings to find the longest one for bar width calc
    let resets_strs: Vec<String> = buckets
        .iter()
        .map(|(_, b)| {
            b.map(|b| format!("  resets {}", abbreviate_resets(&b.resets)))
                .unwrap_or_default()
        })
        .collect();
    let max_resets_len = resets_strs.iter().map(|s| s.len()).max().unwrap_or(0);

    // label(9) + bar + pct(5) + resets(max_resets_len)
    let fixed_width = 9 + 5 + max_resets_len;
    let bar_width = (inner.width as usize).saturating_sub(fixed_width);

    let lines: Vec<Line> = buckets
        .into_iter()
        .zip(resets_strs)
        .filter_map(|((label, bucket), resets_str)| {
            let b = bucket?;
            let color = usage_percent_color(theme, b.percent);
            let filled = (bar_width as u32 * b.percent as u32 / 100) as usize;
            let empty = bar_width.saturating_sub(filled);
            let bar_filled = "\u{2588}".repeat(filled);
            let bar_empty = "\u{2591}".repeat(empty);
            // Pad resets to max_resets_len so bars align
            let padded_resets = format!("{:<width$}", resets_str, width = max_resets_len);

            Some(Line::from(vec![
                Span::styled(
                    format!(" {:<8}", label),
                    Style::default().fg(theme.text_muted),
                ),
                Span::styled(bar_filled, Style::default().fg(color)),
                Span::styled(
                    bar_empty,
                    Style::default()
                        .fg(theme.text_muted)
                        .add_modifier(Modifier::DIM),
                ),
                Span::styled(format!(" {:>3}%", b.percent), Style::default().fg(color)),
                Span::styled(padded_resets, Style::default().fg(theme.text_muted)),
            ]))
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn abbreviate_resets(resets: &str) -> String {
    // "12pm (America/Los_Angeles)" -> "12pm PT"
    // "Apr 23 at 6pm (America/New_York)" -> "Apr 23 at 6pm ET"
    if let Some(idx) = resets.find('(') {
        let time_part = resets[..idx].trim_end();
        let tz_part = resets[idx..].trim_matches(|c| c == '(' || c == ')');
        let abbr = match tz_part {
            "America/Los_Angeles" => "PT",
            "America/Denver" => "MT",
            "America/Chicago" => "CT",
            "America/New_York" => "ET",
            "Europe/London" => "GMT",
            "Europe/Paris" | "Europe/Berlin" => "CET",
            "Asia/Tokyo" => "JST",
            "Asia/Shanghai" | "Asia/Hong_Kong" => "CST",
            "UTC" => "UTC",
            other => other.rsplit('/').next().unwrap_or(other),
        };
        format!("{} {}", time_part, abbr)
    } else {
        resets.to_string()
    }
}

fn usage_percent_color(theme: &crate::ui::theme::Theme, percent: u8) -> Color {
    if percent >= 80 {
        theme.error
    } else if percent >= 50 {
        theme.warning
    } else {
        theme.success
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
    fn test_sparkline_empty_history() {
        let spark = render_sparkline_str(&[], 4);
        assert_eq!(spark, "");
    }

    #[test]
    fn test_sparkline_all_old_returns_empty() {
        use crate::types::StatusHistoryEntry;
        // History older than 24h — should return empty
        let now = chrono::Utc::now().timestamp_millis();
        let two_days_ago = now - 2 * 24 * 60 * 60 * 1000;
        let history = vec![StatusHistoryEntry {
            status: "running".to_string(),
            timestamp: two_days_ago,
        }];
        let spark = render_sparkline_at(&history, 16, now);
        assert_eq!(spark, "");
    }

    #[test]
    fn test_sparkline_bucket_count_matches() {
        use crate::types::StatusHistoryEntry;
        let now = chrono::Utc::now().timestamp_millis();
        let history = vec![StatusHistoryEntry {
            status: "running".to_string(),
            timestamp: now - 60 * 60 * 1000, // 1 hour ago
        }];
        let spark = render_sparkline_at(&history, 16, now);
        assert_eq!(spark.chars().count(), 16);
    }

    #[test]
    fn test_sparkline_recent_running_shows_tall_bars() {
        use crate::types::StatusHistoryEntry;
        let now = chrono::Utc::now().timestamp_millis();
        // Running for the entire last 24h
        let history = vec![StatusHistoryEntry {
            status: "running".to_string(),
            timestamp: now - 24 * 60 * 60 * 1000,
        }];
        let spark = render_sparkline_at(&history, 4, now);
        // All 4 buckets should be running = ▆
        assert_eq!(spark, "\u{2586}\u{2586}\u{2586}\u{2586}");
    }

    #[test]
    fn test_sparkline_mixed_statuses() {
        use crate::types::StatusHistoryEntry;
        let now = chrono::Utc::now().timestamp_millis();
        let h24 = 24 * 60 * 60 * 1000;
        // First half idle, second half running (4 buckets)
        let history = vec![
            StatusHistoryEntry {
                status: "idle".to_string(),
                timestamp: now - h24,
            },
            StatusHistoryEntry {
                status: "running".to_string(),
                timestamp: now - h24 / 2,
            },
        ];
        let spark = render_sparkline_at(&history, 4, now);
        assert_eq!(spark.chars().count(), 4);
        // First 2 buckets idle (▁), last 2 running (▆)
        assert_eq!(spark, "\u{2581}\u{2581}\u{2586}\u{2586}");
    }

    #[test]
    fn test_usage_color_thresholds() {
        let theme = crate::ui::theme::Theme::dark();
        // < 50% = success (green)
        assert_eq!(usage_percent_color(&theme, 0), theme.success);
        assert_eq!(usage_percent_color(&theme, 49), theme.success);
        // 50-79% = warning (yellow)
        assert_eq!(usage_percent_color(&theme, 50), theme.warning);
        assert_eq!(usage_percent_color(&theme, 79), theme.warning);
        // >= 80% = error (red)
        assert_eq!(usage_percent_color(&theme, 80), theme.error);
        assert_eq!(usage_percent_color(&theme, 100), theme.error);
    }

    #[test]
    fn test_abbreviate_resets_known_timezones() {
        assert_eq!(abbreviate_resets("12pm (America/Los_Angeles)"), "12pm PT");
        assert_eq!(abbreviate_resets("5pm (America/New_York)"), "5pm ET");
        assert_eq!(abbreviate_resets("3pm (America/Chicago)"), "3pm CT");
        assert_eq!(
            abbreviate_resets("Apr 23 at 6pm (America/Los_Angeles)"),
            "Apr 23 at 6pm PT"
        );
    }

    #[test]
    fn test_abbreviate_resets_unknown_timezone() {
        // Falls back to city name
        assert_eq!(abbreviate_resets("12pm (Asia/Kolkata)"), "12pm Kolkata");
    }

    #[test]
    fn test_abbreviate_resets_no_parens() {
        assert_eq!(abbreviate_resets("12pm PT"), "12pm PT");
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
