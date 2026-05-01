//! Session detail panel: preview pane and metadata view

use ratatui::layout::Rect;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use ansi_to_tui::IntoText;

use crate::types::{Session, SessionStatus};
use crate::ui::theme::Theme;

use super::compat::convert_core_line;
use super::format::{format_note_age, format_session_duration, format_timestamp};

/// Render the terminal preview pane
pub(super) fn render_preview(
    frame: &mut Frame,
    area: Rect,
    session: &Session,
    theme: &Theme,
    preview_content: &str,
) {
    let block = Block::default()
        .title(" Preview ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    // No active tmux session — show pulsating alert
    let no_tmux = session.tmux_session.is_empty()
        || matches!(
            session.status,
            SessionStatus::Stopped | SessionStatus::Crashed
        );

    if no_tmux {
        render_alert_icon(frame, inner, theme);
        return;
    }

    if preview_content.is_empty() {
        let loading = Paragraph::new("Loading...").style(Style::default().fg(theme.text_muted));
        frame.render_widget(loading, inner);
        return;
    }

    // Convert ANSI content to ratatui Text, keeping only lines that fit
    let height = inner.height as usize;

    match preview_content.into_text() {
        Ok(core_text) => {
            // Convert ratatui_core types to ratatui types for rendering
            let line_count = core_text.lines.len();
            let skip = line_count.saturating_sub(height);
            let visible_lines: Vec<Line> = core_text
                .lines
                .into_iter()
                .skip(skip)
                .map(convert_core_line)
                .collect();
            frame.render_widget(Paragraph::new(visible_lines), inner);
        }
        Err(_) => {
            // Fall back to plain text rendering
            let lines: Vec<&str> = preview_content.lines().collect();
            let skip = if lines.len() > height {
                lines.len() - height
            } else {
                0
            };
            let visible: Vec<Line> = lines.into_iter().skip(skip).map(Line::raw).collect();
            frame.render_widget(Paragraph::new(visible), inner);
        }
    }
}

/// Render a pulsating red alert icon for sessions without an active terminal
fn render_alert_icon(frame: &mut Frame, area: Rect, theme: &Theme) {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as f64;

    // Pulse over a 2-second cycle using a sine wave
    let t = (now_ms / 2000.0) * std::f64::consts::TAU;
    let brightness = ((t.sin() + 1.0) / 2.0 * 200.0 + 55.0) as u8; // 55–255

    let color = Color::Rgb(brightness, 0, 0);

    let icon = Paragraph::new(vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  ⚠  No terminal",
            Style::default().fg(color).bold(),
        )]),
        Line::from(vec![Span::styled(
            "  Session not running",
            Style::default().fg(theme.text_muted),
        )]),
    ]);

    frame.render_widget(icon, area);
}

/// Render the detail panel for the selected session
pub(super) fn render_metadata(frame: &mut Frame, area: Rect, session: &Session, theme: &Theme) {
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
            let note_lines: Vec<&str> = note.text.lines().collect();
            // First line gets the timestamp prefix
            let first_line = note_lines.first().copied().unwrap_or("");
            let first_display = if first_line.len() > 60 {
                format!("{}...", &first_line[..57])
            } else {
                first_line.to_string()
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {}: ", age),
                    Style::default().fg(theme.text_muted),
                ),
                Span::styled(first_display, Style::default().fg(theme.text)),
            ]));
            // Continuation lines indented to align with first line text
            for cont_line in note_lines.iter().skip(1).take(3) {
                let padding = format!("  {}: ", age);
                let indent = " ".repeat(padding.len());
                let display = if cont_line.len() > 60 {
                    format!("{}...", &cont_line[..57])
                } else {
                    cont_line.to_string()
                };
                lines.push(Line::from(vec![
                    Span::styled(indent, Style::default().fg(theme.text_muted)),
                    Span::styled(display, Style::default().fg(theme.text)),
                ]));
            }
        }
    }

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}
