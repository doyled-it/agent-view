//! Per-session token/context pane. Tool-agnostic.
//!
//! Leads with tokens — context tokens against model window, and per-session
//! totals (input / output / cached). The dollar list-price estimate is
//! hidden by default and only shown when `costs.show_list_price_estimate`
//! is set, because most users are on flat-fee subscription plans where
//! the dollar number is misleading.

use crate::app::App;
use crate::core::storage::CostTotals;
use crate::core::tokens::format_tokens;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

/// Format-only helper, extracted so it can be unit-tested without a Frame.
pub(super) fn build_lines(
    context_tokens: Option<i64>,
    context_window: Option<i64>,
    totals: &CostTotals,
    show_dollars: bool,
    theme: &crate::ui::theme::Theme,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if let (Some(used), Some(window)) = (context_tokens, context_window) {
        let pct = if window > 0 {
            (used as f64 / window as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        lines.push(Line::from(format!(
            "Context: {} / {} ({:.0}%)",
            format_tokens(used),
            format_tokens(window),
            pct * 100.0
        )));
    } else if let Some(used) = context_tokens {
        lines.push(Line::from(format!("Context: {}", format_tokens(used))));
    }
    let in_tot = totals.input + totals.cache_read + totals.cache_creation;
    lines.push(Line::from(format!(
        "Session: {} in \u{00b7} {} out \u{00b7} {} cached",
        format_tokens(in_tot),
        format_tokens(totals.output),
        format_tokens(totals.cache_read + totals.cache_creation),
    )));
    if show_dollars && totals.microdollars > 0 {
        let dollars = totals.microdollars as f64 / 1_000_000.0;
        lines.push(Line::from(Span::styled(
            format!("\u{2248} ${:.2} list-price estimate", dollars),
            Style::default().fg(theme.text_muted),
        )));
    }
    lines
}

pub(super) fn render_session_usage_pane(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let Some(session) = app.selected_session() else {
        return;
    };
    let block = Block::default()
        .title(" Session ")
        .title_style(Style::default().fg(theme.text_muted))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.border));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let totals = app
        .storage
        .as_ref()
        .and_then(|h| h.lock().ok())
        .and_then(|s| s.cost_totals_for_session(&session.id).ok())
        .unwrap_or_default();
    let context_tokens = Some(session.tokens_used).filter(|&n| n > 0);
    let context_window = context_window_for(&session.tool);
    let show_dollars = app.config.costs.show_list_price_estimate;

    let lines = build_lines(context_tokens, context_window, &totals, show_dollars, theme);
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Model-window defaults per tool. TODO: derive per-model from a constants
/// table; today we use a single per-tool fallback that's right for the
/// common case.
fn context_window_for(tool: &crate::types::Tool) -> Option<i64> {
    match tool {
        crate::types::Tool::Claude => Some(200_000),
        crate::types::Tool::Codex => Some(258_400),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;

    fn t() -> Theme {
        Theme::dark()
    }

    fn totals(
        input: i64,
        output: i64,
        cache_read: i64,
        cache_creation: i64,
        micros: i64,
    ) -> CostTotals {
        CostTotals {
            input,
            output,
            cache_read,
            cache_creation,
            microdollars: micros,
        }
    }

    #[test]
    fn build_lines_hides_dollars_by_default() {
        let totals = totals(100, 50, 0, 0, 5_000_000);
        let lines = build_lines(Some(150), Some(200_000), &totals, false, &t());
        let joined = lines_to_string(&lines);
        assert!(joined.contains("Session:"));
        assert!(!joined.contains("$"), "no dollar line when toggle off");
    }

    #[test]
    fn build_lines_shows_dollars_when_enabled() {
        let totals = totals(100, 50, 0, 0, 5_000_000);
        let lines = build_lines(Some(150), Some(200_000), &totals, true, &t());
        let joined = lines_to_string(&lines);
        assert!(joined.contains("\u{2248} $5.00 list-price estimate"));
    }

    #[test]
    fn build_lines_hides_dollars_when_total_is_zero_even_if_enabled() {
        let totals = totals(0, 0, 0, 0, 0);
        let lines = build_lines(None, None, &totals, true, &t());
        let joined = lines_to_string(&lines);
        assert!(!joined.contains("$"));
    }

    #[test]
    fn build_lines_renders_context_progress() {
        let totals = totals(0, 0, 0, 0, 0);
        let lines = build_lines(Some(50_000), Some(200_000), &totals, false, &t());
        let joined = lines_to_string(&lines);
        assert!(joined.contains("Context:"));
        assert!(joined.contains("25%"));
    }

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
}
