//! Routine list rendering for the Routines tab

use crate::app::{App, RoutineListRow};
use ratatui::prelude::*;
use ratatui::widgets::*;

/// Render the routine list in the given area
pub fn render_routine_list(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;

    if app.routine_list_rows.is_empty() {
        let msg = Paragraph::new("No routines. Press 'n' to create one.")
            .style(Style::default().fg(theme.text_muted))
            .alignment(Alignment::Center);
        frame.render_widget(msg, area);
        return;
    }

    let items: Vec<ListItem> = app
        .routine_list_rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let is_selected = i == app.routine_selected_index;
            match row {
                RoutineListRow::Group {
                    group,
                    routine_count,
                } => {
                    let arrow = if group.expanded {
                        "\u{25BC}"
                    } else {
                        "\u{25B6}"
                    };
                    let spans = vec![
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
                            format!("  ({})", routine_count),
                            Style::default().fg(if is_selected {
                                theme.selected_item_text
                            } else {
                                theme.text_muted
                            }),
                        ),
                    ];
                    let bg = if is_selected {
                        theme.primary
                    } else {
                        theme.background_element
                    };
                    ListItem::new(Line::from(spans)).style(Style::default().bg(bg))
                }
                RoutineListRow::Routine(routine) => {
                    let enabled_icon = if routine.enabled {
                        "\u{25CF}"
                    } else {
                        "\u{25CB}"
                    };
                    let enabled_color = if routine.enabled {
                        theme.success
                    } else {
                        theme.text_muted
                    };
                    let pin_indicator = if routine.pinned { "\u{25B4}" } else { " " };
                    let schedule_str = crate::core::schedule::human_readable(&routine.schedule);
                    let expand_arrow = if routine.expanded {
                        "\u{25BC}"
                    } else {
                        "\u{25B6}"
                    };
                    let run_count_str = format!("{}x", routine.run_count);

                    let next_str = routine.next_run_at.map(format_next_run).unwrap_or_default();

                    let row_width = area.width as usize;
                    let left_content = format!(
                        " {} {} {} {}  {}",
                        pin_indicator, enabled_icon, expand_arrow, routine.name, schedule_str
                    );
                    let right_content = format!("{}  {} ", run_count_str, next_str);
                    let pad = if left_content.chars().count() + right_content.chars().count()
                        < row_width
                    {
                        row_width - left_content.chars().count() - right_content.chars().count()
                    } else {
                        1
                    };

                    let line = Line::from(vec![
                        Span::styled(
                            format!(" {} ", pin_indicator),
                            Style::default().fg(theme.accent),
                        ),
                        Span::styled(
                            format!("{} ", enabled_icon),
                            Style::default().fg(enabled_color),
                        ),
                        Span::styled(
                            format!("{} ", expand_arrow),
                            Style::default().fg(theme.text_muted),
                        ),
                        Span::styled(routine.name.clone(), Style::default().fg(theme.text).bold()),
                        Span::styled(
                            format!("  {}", schedule_str),
                            Style::default().fg(theme.text_muted),
                        ),
                        Span::raw(" ".repeat(pad)),
                        Span::styled(run_count_str, Style::default().fg(theme.text_muted)),
                        Span::styled(format!("  {}", next_str), Style::default().fg(theme.info)),
                        Span::raw(" "),
                    ]);

                    let bg = if is_selected {
                        theme.background_element
                    } else {
                        theme.background
                    };
                    ListItem::new(line).style(Style::default().bg(bg))
                }
                RoutineListRow::Run {
                    run,
                    routine_name: _,
                } => {
                    let status_icon = run.status.icon();
                    let status_color = match run.status {
                        crate::types::RunStatus::Completed => theme.success,
                        crate::types::RunStatus::Running => theme.info,
                        crate::types::RunStatus::Failed => theme.error,
                        crate::types::RunStatus::TimedOut => theme.warning,
                        crate::types::RunStatus::Crashed => theme.error,
                    };

                    let time_str = format_timestamp(run.started_at);
                    let duration_str = run
                        .finished_at
                        .map(|f| format_duration_ms(f - run.started_at))
                        .unwrap_or_else(|| "running...".to_string());
                    let steps_str = format!("{}/{}", run.steps_completed, run.steps_total);

                    let line = Line::from(vec![
                        Span::raw("      "),
                        Span::styled(
                            format!("{} ", status_icon),
                            Style::default().fg(status_color),
                        ),
                        Span::styled(time_str, Style::default().fg(theme.text)),
                        Span::styled(
                            format!("  {}", duration_str),
                            Style::default().fg(theme.text_muted),
                        ),
                        Span::styled(
                            format!("  steps: {}", steps_str),
                            Style::default().fg(theme.text_muted),
                        ),
                        if run.promoted_session_id.is_some() {
                            Span::styled("  [promoted]", Style::default().fg(theme.accent))
                        } else {
                            Span::raw("")
                        },
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

fn format_timestamp(millis: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_millis_opt(millis)
        .single()
        .map(|dt| dt.format("%m/%d %H:%M").to_string())
        .unwrap_or_else(|| "???".to_string())
}

fn format_duration_ms(ms: i64) -> String {
    if ms < 0 {
        return "???".to_string();
    }
    let secs = ms / 1000;
    let mins = secs / 60;
    let hours = mins / 60;
    if hours > 0 {
        format!("{}h{}m", hours, mins % 60)
    } else if mins > 0 {
        format!("{}m{}s", mins, secs % 60)
    } else {
        format!("{}s", secs)
    }
}

fn format_next_run(millis: i64) -> String {
    use chrono::{Local, TimeZone};
    Local
        .timestamp_millis_opt(millis)
        .single()
        .map(|dt| {
            let now = Local::now();
            let diff = dt.signed_duration_since(now);
            if diff.num_hours() < 24 {
                dt.format("%H:%M").to_string()
            } else {
                dt.format("%m/%d %H:%M").to_string()
            }
        })
        .unwrap_or_default()
}
