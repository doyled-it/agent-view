//! Detail panel — shows session metadata and/or terminal preview on the right side

use crate::app::DetailPanelMode;
use crate::types::Session;
use crate::ui::theme::Theme;
use ansi_to_tui::IntoText;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::*;
use ratatui::widgets::*;

/// Minimum terminal width to show the detail panel
pub const DETAIL_PANEL_MIN_WIDTH: u16 = 80;

/// Width of the panel when showing preview (wider modes)
pub const WIDE_PANEL_PERCENT: u16 = 45;

/// Width of the panel when showing metadata only
pub const NARROW_PANEL_WIDTH: u16 = 36;

/// Dispatch rendering to the appropriate sub-renderer based on mode
pub fn render_detail_panel(
    frame: &mut Frame,
    area: Rect,
    session: &Session,
    theme: &Theme,
    mode: DetailPanelMode,
    preview_content: &str,
) {
    match mode {
        DetailPanelMode::None => {}
        DetailPanelMode::Preview => {
            render_preview(frame, area, session, theme, preview_content);
        }
        DetailPanelMode::Metadata => {
            render_metadata(frame, area, session, theme);
        }
        DetailPanelMode::Both => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(area);
            render_preview(frame, chunks[0], session, theme, preview_content);
            render_metadata(frame, chunks[1], session, theme);
        }
    }
}

/// Compute the panel width based on mode and terminal width
pub fn panel_width(mode: DetailPanelMode, terminal_width: u16) -> u16 {
    match mode {
        DetailPanelMode::None => 0,
        DetailPanelMode::Metadata => NARROW_PANEL_WIDTH,
        DetailPanelMode::Preview | DetailPanelMode::Both => {
            (terminal_width * WIDE_PANEL_PERCENT / 100).max(NARROW_PANEL_WIDTH)
        }
    }
}

/// Render the terminal preview pane
fn render_preview(
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
            crate::types::SessionStatus::Stopped | crate::types::SessionStatus::Crashed
        );

    if no_tmux {
        render_alert_icon(frame, inner, theme);
        return;
    }

    if preview_content.is_empty() {
        let loading = Paragraph::new("Loading...")
            .style(Style::default().fg(theme.text_muted));
        frame.render_widget(loading, inner);
        return;
    }

    // Convert ANSI content to ratatui Text, keeping only lines that fit
    let height = inner.height as usize;

    match preview_content.into_text() {
        Ok(text) => {
            let line_count = text.lines.len();
            let skip = if line_count > height {
                line_count - height
            } else {
                0
            };
            let visible_lines: Vec<Line> = text.lines.into_iter().skip(skip).collect();
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
            let visible: Vec<Line> = lines
                .into_iter()
                .skip(skip)
                .map(Line::raw)
                .collect();
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
fn render_metadata(frame: &mut Frame, area: Rect, session: &Session, theme: &Theme) {
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
