//! Per-session token/context pane. Tool-agnostic.
//!
//! Leads with tokens — context against model window, and per-session totals
//! (input / output / cached). The dollar list-price estimate is hidden by
//! default and only shown when `costs.show_list_price_estimate` is set,
//! because most users are on flat-fee subscription plans where the dollar
//! number is misleading.
//!
//! Visual layout mirrors `claude_quota_pane`: a left-padded label, a
//! Unicode block-bar coloured by fill percent, and a right-side annotation.

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::core::storage::CostTotals;
use crate::core::tokens::format_tokens;
use crate::ui::theme::Theme;

/// Row layout knobs. Keep in sync with `claude_quota_pane` so the two panes
/// stack with visually aligned labels and bars.
const LABEL_WIDTH: usize = 8;
const PCT_WIDTH: usize = 5; // " 100%"

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

    let lines = build_lines(
        context_tokens,
        context_window,
        &totals,
        show_dollars,
        theme,
        inner.width as usize,
    );
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Pure render helper, extracted so layout/labelling can be unit-tested
/// without a `Frame`.
pub(super) fn build_lines(
    context_tokens: Option<i64>,
    context_window: Option<i64>,
    totals: &CostTotals,
    show_dollars: bool,
    theme: &Theme,
    inner_width: usize,
) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();

    if let Some(line) = context_line(context_tokens, context_window, theme, inner_width) {
        lines.push(line);
    }

    let totals_text = format_totals(totals);
    lines.push(Line::from(vec![
        Span::styled(
            format!(" {:<width$}", "Tokens", width = LABEL_WIDTH),
            Style::default().fg(theme.text_muted),
        ),
        Span::styled(totals_text, Style::default().fg(theme.text)),
    ]));

    if show_dollars && totals.microdollars > 0 {
        let dollars = totals.microdollars as f64 / 1_000_000.0;
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {:<width$}", "Cost", width = LABEL_WIDTH),
                Style::default().fg(theme.text_muted),
            ),
            Span::styled(
                format!("\u{2248} ${:.2} list-price estimate", dollars),
                Style::default().fg(theme.text_muted),
            ),
        ]));
    }

    lines
}

/// Build the "Context" row. Returns None when no token signal is available
/// — the pane stays silent rather than showing a misleading 0% bar. When
/// the context_window is unknown (Shell, or a tool without a published
/// limit) we render the bare token count without a percentage or bar — a
/// 0% bar would otherwise misrepresent "no limit known" as "no usage".
fn context_line(
    context_tokens: Option<i64>,
    context_window: Option<i64>,
    theme: &Theme,
    inner_width: usize,
) -> Option<Line<'static>> {
    let used = context_tokens?;
    let used_str = format_tokens(used);

    let Some(window) = context_window.filter(|w| *w > 0) else {
        // No window → render `Context  150k` with no bar, no percent.
        return Some(Line::from(vec![
            Span::styled(
                format!(" {:<width$}", "Context", width = LABEL_WIDTH),
                Style::default().fg(theme.text_muted),
            ),
            Span::styled(used_str, Style::default().fg(theme.text)),
        ]));
    };

    let annotation = format!("  {} / {}", used_str, format_tokens(window));
    let raw_pct = (used as f64 / window as f64).clamp(0.0, 1.0) * 100.0;
    let pct = raw_pct.round() as u8;

    // Reserve: leading-space + label + " NN%" + annotation. Bar gets the rest.
    let fixed_width = 1 + LABEL_WIDTH + PCT_WIDTH + annotation.len();
    let bar_width = inner_width.saturating_sub(fixed_width);

    let color = bar_color(theme, pct);
    let filled = ((bar_width as u32) * (pct as u32) / 100) as usize;
    let empty = bar_width.saturating_sub(filled);
    let bar_filled = "\u{2588}".repeat(filled);
    let bar_empty = "\u{2591}".repeat(empty);

    Some(Line::from(vec![
        Span::styled(
            format!(" {:<width$}", "Context", width = LABEL_WIDTH),
            Style::default().fg(theme.text_muted),
        ),
        Span::styled(bar_filled, Style::default().fg(color)),
        Span::styled(
            bar_empty,
            Style::default()
                .fg(theme.text_muted)
                .add_modifier(Modifier::DIM),
        ),
        Span::styled(format!(" {:>3}%", pct), Style::default().fg(color)),
        Span::styled(annotation, Style::default().fg(theme.text_muted)),
    ]))
}

fn format_totals(totals: &CostTotals) -> String {
    let in_tot = totals.input + totals.cache_read + totals.cache_creation;
    let cached = totals.cache_read + totals.cache_creation;
    format!(
        "{} in \u{00b7} {} out \u{00b7} {} cached",
        format_tokens(in_tot),
        format_tokens(totals.output),
        format_tokens(cached),
    )
}

/// Mirrors `claude_quota_pane::usage_percent_color` so a Context bar at 60%
/// reads the same colour as a Usage bar at 60%.
fn bar_color(theme: &Theme, percent: u8) -> Color {
    if percent >= 80 {
        theme.error
    } else if percent >= 50 {
        theme.warning
    } else {
        theme.success
    }
}

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
    fn build_lines_hides_dollars_by_default() {
        let totals = totals(100, 50, 0, 0, 5_000_000);
        let lines = build_lines(Some(150), Some(200_000), &totals, false, &t(), 80);
        let joined = lines_to_string(&lines);
        assert!(joined.contains("Tokens"));
        assert!(!joined.contains("$"), "no dollar line when toggle off");
    }

    #[test]
    fn build_lines_shows_dollars_when_enabled() {
        let totals = totals(100, 50, 0, 0, 5_000_000);
        let lines = build_lines(Some(150), Some(200_000), &totals, true, &t(), 80);
        let joined = lines_to_string(&lines);
        assert!(joined.contains("\u{2248} $5.00 list-price estimate"));
        assert!(joined.contains("Cost"));
    }

    #[test]
    fn build_lines_hides_dollars_when_total_is_zero_even_if_enabled() {
        let totals = totals(0, 0, 0, 0, 0);
        let lines = build_lines(None, None, &totals, true, &t(), 80);
        let joined = lines_to_string(&lines);
        assert!(!joined.contains("$"));
    }

    #[test]
    fn build_lines_renders_context_bar_with_percent() {
        let totals = totals(0, 0, 0, 0, 0);
        let lines = build_lines(Some(50_000), Some(200_000), &totals, false, &t(), 80);
        let joined = lines_to_string(&lines);
        assert!(joined.contains("Context"));
        assert!(joined.contains(" 25%"));
        assert!(joined.contains("50.0k / 200.0k"));
        assert!(joined.contains("\u{2588}") || joined.contains("\u{2591}"));
    }

    #[test]
    fn build_lines_omits_context_row_when_no_signal() {
        let totals = totals(100, 50, 0, 0, 0);
        let lines = build_lines(None, Some(200_000), &totals, false, &t(), 80);
        let joined = lines_to_string(&lines);
        assert!(!joined.contains("Context"));
        assert!(joined.contains("Tokens"));
    }

    #[test]
    fn build_lines_context_without_window_renders_bare_count() {
        // Shell sessions (no published context window) used to render a
        // misleading 0% bar. The pane now drops the bar + percentage when
        // the window is unknown, showing only the raw token count.
        let totals = totals(0, 0, 0, 0, 0);
        let lines = build_lines(Some(150_000), None, &totals, false, &t(), 80);
        let joined = lines_to_string(&lines);
        assert!(joined.contains("Context"));
        assert!(joined.contains("150.0k"));
        assert!(!joined.contains("%"), "no percentage when window unknown");
        assert!(
            !joined.contains("\u{2588}") && !joined.contains("\u{2591}"),
            "no bar when window unknown"
        );
    }

    #[test]
    fn build_lines_context_clamps_at_100_percent() {
        // tokens_used exceeds the window — the bar must clamp, not overflow.
        let totals = totals(0, 0, 0, 0, 0);
        let lines = build_lines(Some(1_000_000), Some(200_000), &totals, false, &t(), 80);
        let joined = lines_to_string(&lines);
        assert!(joined.contains("100%"));
    }

    #[test]
    fn context_line_color_thresholds() {
        let theme = Theme::dark();
        assert_eq!(bar_color(&theme, 0), theme.success);
        assert_eq!(bar_color(&theme, 49), theme.success);
        assert_eq!(bar_color(&theme, 50), theme.warning);
        assert_eq!(bar_color(&theme, 79), theme.warning);
        assert_eq!(bar_color(&theme, 80), theme.error);
        assert_eq!(bar_color(&theme, 100), theme.error);
    }
}
