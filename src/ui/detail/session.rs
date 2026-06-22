//! Session detail panel: preview pane and metadata view

use ratatui::layout::Rect;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use ansi_to_tui::IntoText;

use crate::core::tokens::format_tokens;
use crate::types::{Session, SessionStatus};
use crate::ui::theme::{status_color, Theme};

use super::compat::{convert_core_line, wrap_line_to_width};
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

    // Convert ANSI content to ratatui Text, wrapping over-width lines (the
    // captured pane is sized to the agent's terminal, often wider than this
    // pane) and keeping only the tail rows that fit.
    let height = inner.height as usize;
    let width = inner.width as usize;

    match preview_content.into_text() {
        Ok(core_text) => {
            // TUIs that draw with absolute cursor positioning (Codex) pad the
            // capture buffer's tail with blank lines, which would otherwise
            // push real content out of the visible window.
            let mut lines = core_text.lines;
            while lines
                .last()
                .is_some_and(|l| l.spans.iter().all(|s| s.content.trim().is_empty()))
            {
                lines.pop();
            }
            let wrapped: Vec<Line> = lines
                .into_iter()
                .map(convert_core_line)
                .flat_map(|l| wrap_line_to_width(l, width))
                .collect();
            let skip = wrapped.len().saturating_sub(height);
            let visible_lines: Vec<Line> = wrapped.into_iter().skip(skip).collect();
            frame.render_widget(Paragraph::new(visible_lines), inner);
        }
        Err(_) => {
            // Fall back to plain text rendering
            let mut lines: Vec<&str> = preview_content.lines().collect();
            while lines.last().is_some_and(|l| l.trim().is_empty()) {
                lines.pop();
            }
            let wrapped: Vec<Line> = lines
                .into_iter()
                .flat_map(|l| wrap_line_to_width(Line::raw(l), width))
                .collect();
            let skip = wrapped.len().saturating_sub(height);
            let visible: Vec<Line> = wrapped.into_iter().skip(skip).collect();
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

    let paragraph = Paragraph::new(build_metadata_lines(session, theme)).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, inner);
}

fn metadata_line(
    label: &'static str,
    value: impl Into<String>,
    theme: &Theme,
    value_style: Style,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(label, Style::default().fg(theme.text_muted)),
        Span::styled(value.into(), value_style),
    ])
}

fn mcp_server_summary(session: &Session) -> String {
    let mut seen = Vec::new();
    let mut enabled_servers = Vec::new();
    let mut disabled_servers = Vec::new();

    for server in &session.mcp_selection.servers {
        let id = server.id.as_str();
        if seen.contains(&id) {
            continue;
        }
        seen.push(id);

        if server.enabled {
            enabled_servers.push(id);
        } else {
            disabled_servers.push(id);
        }
    }

    if !enabled_servers.is_empty() {
        enabled_servers.join(", ")
    } else if !disabled_servers.is_empty() {
        format!("All except: {}", disabled_servers.join(", "))
    } else {
        "(none)".to_string()
    }
}

fn build_metadata_lines(session: &Session, theme: &Theme) -> Vec<Line<'static>> {
    let status_color = status_color(theme, session.status);

    let created = format_timestamp(session.created_at);
    let started = format_timestamp(session.last_started_at);
    let duration = format_session_duration(session.last_started_at);

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
            Span::styled(
                session.tool.as_str().to_string(),
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(vec![
            Span::styled("Path: ", Style::default().fg(theme.text_muted)),
            Span::styled(
                session.project_path.clone(),
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(vec![
            Span::styled("Group: ", Style::default().fg(theme.text_muted)),
            Span::styled(session.group_path.clone(), Style::default().fg(theme.text)),
        ]),
    ];

    lines.push(metadata_line(
        "Created: ",
        created,
        theme,
        Style::default().fg(theme.text),
    ));
    lines.push(metadata_line(
        "Started: ",
        started,
        theme,
        Style::default().fg(theme.text),
    ));
    lines.push(metadata_line(
        "Uptime: ",
        duration,
        theme,
        Style::default().fg(theme.text),
    ));

    if !session.mcp_selection.is_all_servers() {
        if let Some(profile_id) = session
            .mcp_selection
            .profile_id
            .as_deref()
            .filter(|id| !id.is_empty())
        {
            lines.push(metadata_line(
                "MCP Profile: ",
                profile_id,
                theme,
                Style::default().fg(theme.text),
            ));
        }

        lines.push(metadata_line(
            "MCP Servers: ",
            mcp_server_summary(session),
            theme,
            Style::default().fg(theme.text),
        ));
    }

    if !session.worktree_path.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("Worktree: ", Style::default().fg(theme.text_muted)),
            Span::styled(
                session.worktree_path.clone(),
                Style::default().fg(theme.text),
            ),
        ]));
        if !session.worktree_branch.is_empty() {
            lines.push(Line::from(vec![
                Span::styled("Branch: ", Style::default().fg(theme.text_muted)),
                Span::styled(
                    session.worktree_branch.clone(),
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

    if session.user_waiting {
        lines.push(Line::from(vec![
            Span::styled("Waiting: ", Style::default().fg(theme.text_muted)),
            Span::styled("marked", Style::default().fg(theme.secondary)),
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
                format_tokens(session.tokens_used),
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

    lines
}

#[cfg(test)]
fn build_metadata_lines_for_test(session: &Session) -> Vec<String> {
    build_metadata_lines(session, &Theme::dark())
        .into_iter()
        .map(|line| {
            let mut text = String::new();
            for span in line.spans {
                text.push_str(span.content.as_ref());
            }
            text
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::mcp::{McpSelection, McpServerSelection};

    fn test_session() -> Session {
        crate::core::storage::test_helpers::make_test_session("test-session")
    }

    fn buffer_char_count(buf: &ratatui::buffer::Buffer, ch: &str) -> usize {
        buf.content.iter().filter(|c| c.symbol() == ch).count()
    }

    #[test]
    fn preview_wraps_wide_lines_so_full_text_is_visible() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut session = test_session();
        session.status = SessionStatus::Running;

        // A line far wider than the preview pane's inner width.
        let wide = "A".repeat(120);

        let backend = TestBackend::new(40, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();

        terminal
            .draw(|frame| {
                render_preview(frame, frame.area(), &session, &theme, &wide);
            })
            .unwrap();

        // Inner width is 38 (40 - 2 borders), height 18. 120 chars wrap to
        // 4 rows, all of which fit — so every 'A' must be rendered.
        let count = buffer_char_count(terminal.backend().buffer(), "A");
        assert_eq!(
            count, 120,
            "expected all 120 chars visible via wrapping, got {count}"
        );
    }

    #[test]
    fn preview_keeps_tail_visible_when_wide_lines_wrap() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut session = test_session();
        session.status = SessionStatus::Running;

        // Many wide lines; the newest (tail) line must remain visible after
        // wrapping pushes earlier content off the top.
        let mut content = String::new();
        for _ in 0..30 {
            content.push_str(&"X".repeat(100));
            content.push('\n');
        }
        content.push_str(&"Z".repeat(100));

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        let theme = Theme::dark();

        terminal
            .draw(|frame| {
                render_preview(frame, frame.area(), &session, &theme, &content);
            })
            .unwrap();

        // The tail 'Z' line must be fully visible (100 chars wrapped).
        let count = buffer_char_count(terminal.backend().buffer(), "Z");
        assert_eq!(
            count, 100,
            "expected the tail line fully visible, got {count}"
        );
    }

    #[test]
    fn metadata_lines_include_mcp_selection_summary() {
        let mut session = test_session();
        session.mcp_selection = McpSelection {
            profile_id: Some("rust".to_string()),
            servers: vec![McpServerSelection {
                id: "GitLabMITRE".to_string(),
                enabled: true,
                selected_tools: None,
            }],
        };

        let lines = build_metadata_lines_for_test(&session);

        assert!(
            lines.iter().any(|line| line == "MCP Profile: rust"),
            "metadata lines: {lines:#?}"
        );
        assert!(
            lines.iter().any(|line| line == "MCP Servers: GitLabMITRE"),
            "metadata lines: {lines:#?}"
        );

        let uptime_index = lines
            .iter()
            .position(|line| line.starts_with("Uptime: "))
            .unwrap();
        let profile_index = lines
            .iter()
            .position(|line| line == "MCP Profile: rust")
            .unwrap();
        assert!(uptime_index < profile_index, "metadata lines: {lines:#?}");
    }

    #[test]
    fn metadata_lines_deduplicate_mcp_servers_first_entry_wins() {
        let mut session = test_session();
        session.mcp_selection = McpSelection {
            profile_id: Some("rust".to_string()),
            servers: vec![
                McpServerSelection {
                    id: "GitLabMITRE".to_string(),
                    enabled: true,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "browser".to_string(),
                    enabled: true,
                    selected_tools: None,
                },
                McpServerSelection {
                    id: "GitLabMITRE".to_string(),
                    enabled: true,
                    selected_tools: None,
                },
            ],
        };

        let lines = build_metadata_lines_for_test(&session);

        assert!(
            lines
                .iter()
                .any(|line| line == "MCP Servers: GitLabMITRE, browser"),
            "metadata lines: {lines:#?}"
        );
    }

    #[test]
    fn metadata_lines_show_disabled_only_selection_as_all_except() {
        let mut session = test_session();
        session.mcp_selection = McpSelection {
            profile_id: None,
            servers: vec![McpServerSelection {
                id: "GitLabMITRE".to_string(),
                enabled: false,
                selected_tools: None,
            }],
        };

        let lines = build_metadata_lines_for_test(&session);

        assert!(
            lines
                .iter()
                .any(|line| line == "MCP Servers: All except: GitLabMITRE"),
            "metadata lines: {lines:#?}"
        );
    }
}
