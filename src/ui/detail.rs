//! Detail panel — shows session metadata on the right side

use crate::types::Session;
use crate::ui::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::*;

/// Minimum terminal width to show the detail panel
pub const DETAIL_PANEL_MIN_WIDTH: u16 = 100;

/// Render the detail panel for the selected session
pub fn render(frame: &mut Frame, area: Rect, session: &Session, theme: &Theme) {
    let block = Block::default()
        .title(" Details ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let status_color = crate::ui::theme::status_color(theme, session.status);

    let created = format_timestamp(session.created_at);
    let started = format_timestamp(session.last_started_at);
    let duration = format_session_duration(session.last_started_at, session.status);

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Status: ", Style::default().fg(theme.text_muted)),
            Span::styled(
                format!("{} {}", session.status.icon(), session.status.as_str()),
                Style::default().fg(status_color),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Tool: ", Style::default().fg(theme.text_muted)),
            Span::styled(session.tool.as_str(), Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled("Path: ", Style::default().fg(theme.text_muted)),
            Span::styled(&session.project_path, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled("Group: ", Style::default().fg(theme.text_muted)),
            Span::styled(&session.group_path, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled("Created: ", Style::default().fg(theme.text_muted)),
            Span::styled(created, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled("Started: ", Style::default().fg(theme.text_muted)),
            Span::styled(started, Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled("Uptime: ", Style::default().fg(theme.text_muted)),
            Span::styled(duration, Style::default().fg(theme.text)),
        ]),
    ];

    if !session.worktree_path.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Worktree: ", Style::default().fg(theme.text_muted)),
            Span::styled(&session.worktree_path, Style::default().fg(theme.text)),
        ]));
        if !session.worktree_branch.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Branch: ", Style::default().fg(theme.text_muted)),
                Span::styled(
                    &session.worktree_branch,
                    Style::default().fg(theme.secondary),
                ),
            ]));
        }
    }

    if session.notify {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Notifications: ", Style::default().fg(theme.text_muted)),
            Span::styled("on", Style::default().fg(theme.success)),
        ]));
    }

    if session.follow_up {
        lines.push(Line::from(vec![
            Span::styled("Follow-up: ", Style::default().fg(theme.text_muted)),
            Span::styled("marked", Style::default().fg(theme.warning)),
        ]));
    }

    if session.restart_count > 0 {
        lines.push(Line::from(vec![
            Span::styled("Restarts: ", Style::default().fg(theme.text_muted)),
            Span::styled(
                session.restart_count.to_string(),
                Style::default().fg(theme.text),
            ),
        ]));
    }

    if session.tokens_used > 0 {
        lines.push(Line::from(vec![
            Span::styled("Tokens: ", Style::default().fg(theme.text_muted)),
            Span::styled(
                crate::core::tokens::format_tokens(session.tokens_used),
                Style::default().fg(theme.text),
            ),
        ]));
    }

    if !session.notes.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            "Notes:",
            Style::default().fg(theme.text_muted),
        )]));
        for note in session.notes.iter().rev().take(5) {
            let age = format_note_age(note.timestamp);
            // Truncate long notes for the detail panel
            let display_text = if note.text.len() > 60 {
                format!("{}...", &note.text[..57])
            } else {
                note.text.clone()
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {}: ", age),
                    Style::default().fg(theme.text_muted),
                ),
                Span::styled(display_text, Style::default().fg(theme.text)),
            ]));
        }
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

fn format_timestamp(ms: i64) -> String {
    use chrono::{Local, TimeZone, Utc};
    let dt = Utc.timestamp_millis_opt(ms).single();
    match dt {
        Some(utc) => {
            let local = utc.with_timezone(&Local);
            local.format("%Y-%m-%d %H:%M").to_string()
        }
        None => "unknown".to_string(),
    }
}

fn format_session_duration(created_at_ms: i64, _status: crate::types::SessionStatus) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let diff_ms = now - created_at_ms;
    if diff_ms < 0 {
        return "just started".to_string();
    }

    let seconds = diff_ms / 1000;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;

    if days > 0 {
        format!("{}d {}h", days, hours % 24)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes % 60)
    } else if minutes > 0 {
        format!("{}m", minutes)
    } else {
        "< 1m".to_string()
    }
}

fn format_note_age(timestamp_ms: i64) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let diff_ms = now - timestamp_ms;
    if diff_ms < 0 {
        return "now".to_string();
    }

    let minutes = diff_ms / 60_000;
    let hours = minutes / 60;
    let days = hours / 24;

    if days > 0 {
        format!("{}d ago", days)
    } else if hours > 0 {
        format!("{}h ago", hours)
    } else if minutes > 0 {
        format!("{}m ago", minutes)
    } else {
        "now".to_string()
    }
}
