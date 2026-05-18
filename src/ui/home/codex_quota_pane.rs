//! Codex subscription quota pane.
//!
//! Reads `payload.rate_limits` from the most recent token_count event in the
//! selected session's rollout file. Renders either:
//!   - "Unlimited (preview)" + plan_type when limits aren't yet populated
//!     (e.g. business preview), OR
//!   - primary/secondary used-percent bars + reset countdowns.

use crate::app::App;
use crate::core::runner::codex::cost_handler::{current_rate_limits, RateLimitInfo};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

pub(super) fn render_codex_quota_pane(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let Some(session) = app.selected_session() else {
        return;
    };
    if session.tool != crate::types::Tool::Codex {
        return;
    }

    let rollout_path = resolve_rollout_path_for_session(app, &session.id);
    let info = rollout_path.as_ref().and_then(|p| current_rate_limits(p));

    let block = Block::default()
        .title(" Codex Quota ")
        .title_style(Style::default().fg(theme.text_muted))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.border));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = match info {
        None => vec![Line::from(Span::styled(
            "No quota data yet — session has not produced a token_count event.",
            Style::default().fg(theme.text_muted),
        ))],
        Some(rl) if rl.unlimited_preview => vec![
            Line::from(vec![
                Span::styled(
                    "Unlimited",
                    Style::default()
                        .fg(theme.success)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" (preview)", Style::default().fg(theme.text_muted)),
            ]),
            Line::from(format!(
                "Plan: {}",
                rl.plan_type.as_deref().unwrap_or("unknown")
            )),
        ],
        Some(rl) => render_windows(&rl, theme),
    };

    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_windows(rl: &RateLimitInfo, theme: &crate::ui::theme::Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let Some(p) = &rl.primary {
        if let Some(pct) = p.used_percent {
            lines.push(Line::from(format!("Primary: {:.0}% used", pct)));
        }
        if let Some(secs) = p.resets_in_seconds {
            lines.push(Line::from(format!("Resets in {}s", secs)));
        }
    }
    if let Some(s) = &rl.secondary {
        if let Some(pct) = s.used_percent {
            lines.push(Line::from(format!("Secondary: {:.0}% used", pct)));
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Quota data present but no progress fields populated.",
            Style::default().fg(theme.text_muted),
        )));
    }
    lines
}

fn resolve_rollout_path_for_session(app: &App, session_id: &str) -> Option<std::path::PathBuf> {
    let handle = app.event_state.as_ref()?;
    let state = handle.lock().ok()?;
    let entry = state.hook_status.get(session_id)?;
    let thread_id = entry.tool_session_id.as_deref()?;
    let sessions_root = dirs::home_dir()?.join(".codex").join("sessions");
    crate::core::runner::codex::cost_handler::find_rollout_for_thread(thread_id, &sessions_root)
}
