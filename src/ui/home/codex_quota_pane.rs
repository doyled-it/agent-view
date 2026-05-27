//! Codex subscription quota pane.
//!
//! Reads `payload.rate_limits` from the most recent token_count event in the
//! selected session's rollout file. Renders either:
//!   - "Unlimited (preview)" + plan_type when limits aren't yet populated
//!     (e.g. business preview), OR
//!   - primary/secondary used-percent bars + reset countdowns.

use crate::app::App;
use crate::core::runner::codex::cost_handler::RateLimitInfo;
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

    // Pulled via the watcher's cache — see `EventState::rollout_snapshot`.
    // No filesystem walk on the render path; the snapshot is refreshed only
    // when the file's mtime advances.
    let info = rate_limits_for_session(app, &session.id);

    let block = Block::default()
        .title(" Codex Quota ")
        .title_style(super::pane_title_style(theme))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.border));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = build_quota_lines(info.as_ref(), theme);
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Pure helper for the pane body. Extracted so unit tests can exercise the
/// rendering logic without a `Frame`. Branches:
/// - `None` → "No quota data yet" placeholder
/// - `Some(rl) if unlimited_preview` → bold "Unlimited (preview)" + plan
/// - `Some(rl)` otherwise → primary/secondary windows via `render_windows`
pub(super) fn build_quota_lines(
    info: Option<&RateLimitInfo>,
    theme: &crate::ui::theme::Theme,
) -> Vec<Line<'static>> {
    match info {
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
        Some(rl) => render_windows(rl, theme),
    }
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

fn rate_limits_for_session(app: &App, session_id: &str) -> Option<RateLimitInfo> {
    let handle = app.event_state.as_ref()?;
    let mut state = handle.lock().ok()?;
    let entry = state.hook_status.get(session_id)?;
    let thread_id = entry.tool_session_id.clone()?;
    let path = state.cached_rollout_path(&thread_id)?.to_path_buf();
    state.rollout_snapshot(&path).rate_limits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::runner::codex::cost_handler::RateLimitWindow;
    use crate::ui::theme::Theme;

    fn lines_to_string(lines: &[Line]) -> String {
        let mut s = String::new();
        for line in lines {
            for span in &line.spans {
                s.push_str(&span.content);
            }
            s.push('\n');
        }
        s
    }

    #[test]
    fn build_quota_lines_none_renders_placeholder() {
        let theme = Theme::dark();
        let out = build_quota_lines(None, &theme);
        let joined = lines_to_string(&out);
        assert!(joined.contains("No quota data yet"));
    }

    #[test]
    fn build_quota_lines_unlimited_preview_bolds_label_and_shows_plan() {
        let theme = Theme::dark();
        let info = RateLimitInfo {
            primary: None,
            secondary: None,
            unlimited_preview: true,
            plan_type: Some("business".to_string()),
        };
        let out = build_quota_lines(Some(&info), &theme);
        let joined = lines_to_string(&out);
        assert!(joined.contains("Unlimited"));
        assert!(joined.contains("(preview)"));
        assert!(joined.contains("Plan: business"));
    }

    #[test]
    fn build_quota_lines_primary_window_shows_percent_and_resets() {
        let theme = Theme::dark();
        let info = RateLimitInfo {
            primary: Some(RateLimitWindow {
                used_percent: Some(42.7),
                window_minutes: Some(300),
                resets_in_seconds: Some(1500),
            }),
            secondary: None,
            unlimited_preview: false,
            plan_type: Some("business".to_string()),
        };
        let out = build_quota_lines(Some(&info), &theme);
        let joined = lines_to_string(&out);
        assert!(joined.contains("Primary: 43% used"));
        assert!(joined.contains("Resets in 1500s"));
    }

    #[test]
    fn build_quota_lines_secondary_only_still_renders() {
        let theme = Theme::dark();
        let info = RateLimitInfo {
            primary: None,
            secondary: Some(RateLimitWindow {
                used_percent: Some(10.0),
                window_minutes: Some(60),
                resets_in_seconds: None,
            }),
            unlimited_preview: false,
            plan_type: None,
        };
        let out = build_quota_lines(Some(&info), &theme);
        let joined = lines_to_string(&out);
        assert!(joined.contains("Secondary: 10% used"));
    }

    #[test]
    fn build_quota_lines_empty_windows_renders_fallback() {
        let theme = Theme::dark();
        let info = RateLimitInfo {
            primary: Some(RateLimitWindow::default()),
            secondary: None,
            unlimited_preview: false,
            plan_type: None,
        };
        let out = build_quota_lines(Some(&info), &theme);
        let joined = lines_to_string(&out);
        assert!(joined.contains("no progress fields populated"));
    }
}
