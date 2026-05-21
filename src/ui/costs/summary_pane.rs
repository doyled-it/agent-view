//! Summary pane: API-rate cost, plan cost (if any), savings, token counts.

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::core::cost::{render_usd, CostPeriod, CostSummary, Plan};
use crate::core::tokens::format_tokens;
use crate::ui::theme::Theme;

/// Build the lines a Summary pane displays for `summary` in `period`. Pure
/// — no Frame/area — so tests can assert exact text without a renderer.
pub fn build_summary_lines<'a>(
    summary: &CostSummary,
    period: CostPeriod,
    claude_plan: Plan,
    theme: &'a Theme,
) -> Vec<Line<'a>> {
    let api_label_style = Style::default().fg(theme.text_muted);
    let value_style = Style::default().fg(theme.text).add_modifier(Modifier::BOLD);

    let mut lines = vec![Line::from(vec![
        Span::styled("API-rate cost      ", api_label_style),
        Span::styled(render_usd(summary.total_microdollars), value_style),
    ])];

    // Plan + Saved rows: only when a plan is configured AND the period
    // has a non-zero day count (AllTime suppresses).
    if claude_plan != Plan::Api && period.days() > 0.0 {
        if let Some(limits) = claude_plan.limits() {
            let plan_micro = (limits.monthly_cost_usd * 1_000_000.0 * period.days() / 30.4) as i64;
            lines.push(Line::from(vec![
                Span::styled("Plan cost          ", api_label_style),
                Span::styled(render_usd(plan_micro), value_style),
                Span::raw("  "),
                Span::styled(plan_label(claude_plan), api_label_style),
            ]));
            if let Some(saved) = claude_plan.saved_vs_api(summary.total_microdollars, period.days())
            {
                lines.push(Line::from(vec![
                    Span::styled("Saved             ", api_label_style),
                    Span::styled(format!("-{}", render_usd(saved)), value_style),
                ]));
            }
        }
    }

    lines.push(Line::from(vec![
        Span::styled("Tokens          ", api_label_style),
        Span::raw(format_tokens(summary.input_tokens)),
        Span::raw(" in │ "),
        Span::raw(format_tokens(summary.output_tokens)),
        Span::raw(" out │ "),
        Span::raw(format_tokens(summary.cache_read_tokens)),
        Span::raw(" cache_read"),
    ]));

    lines
}

fn plan_label(plan: Plan) -> &'static str {
    match plan {
        Plan::Api => "(API)",
        Plan::Pro => "Claude Pro",
        Plan::Max5x => "Claude Max 5×",
        Plan::Max20x => "Claude Max 20×",
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let summary = match &app.storage {
        Some(s) => s
            .lock()
            .ok()
            .and_then(|guard| guard.cost_summary(app.cost_period).ok())
            .unwrap_or_default(),
        None => CostSummary::default(),
    };
    let plan = app
        .config
        .costs
        .plan
        .get("claude")
        .copied()
        .unwrap_or_default();
    let lines = build_summary_lines(&summary, app.cost_period, plan, &app.theme);
    let block = Block::default().borders(Borders::ALL).title(" Summary ");
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;

    fn theme() -> Theme {
        Theme::dark()
    }

    #[test]
    fn api_runner_shows_no_plan_lines() {
        let summary = CostSummary {
            total_microdollars: 47_230_000,
            ..Default::default()
        };
        let t = theme();
        let lines = build_summary_lines(&summary, CostPeriod::Week, Plan::Api, &t);
        let rendered: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert!(rendered
            .iter()
            .any(|s: &String| s.contains("API-rate cost")));
        assert!(!rendered.iter().any(|s: &String| s.contains("Plan cost")));
        assert!(!rendered.iter().any(|s: &String| s.contains("Saved")));
    }

    #[test]
    fn pro_plan_shows_savings_row_for_week() {
        let summary = CostSummary {
            total_microdollars: 80_000_000,
            ..Default::default()
        };
        let t = theme();
        let lines = build_summary_lines(&summary, CostPeriod::Week, Plan::Pro, &t);
        let rendered: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert!(rendered.iter().any(|s: &String| s.contains("Claude Pro")));
        assert!(rendered.iter().any(|s: &String| s.contains("Saved")));
    }

    #[test]
    fn alltime_suppresses_savings_row() {
        let summary = CostSummary {
            total_microdollars: 80_000_000,
            ..Default::default()
        };
        let t = theme();
        let lines = build_summary_lines(&summary, CostPeriod::AllTime, Plan::Pro, &t);
        let rendered: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert!(!rendered.iter().any(|s: &String| s.contains("Saved")));
    }
}
