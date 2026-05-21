//! Per-runner cost breakdown: one row per Tool with USD + (Claude only)
//! credit total.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::core::cost::{render_usd, RunnerCost};
use crate::types::Tool;
use crate::ui::theme::Theme;

pub fn build_runner_lines<'a>(
    runners: &[RunnerCost],
    plan_map: &std::collections::HashMap<String, crate::core::cost::Plan>,
    theme: &'a Theme,
) -> Vec<Line<'a>> {
    if runners.is_empty() {
        return vec![Line::from(Span::styled(
            "no cost events in this period",
            Style::default().fg(theme.text_muted),
        ))];
    }
    runners
        .iter()
        .map(|r| {
            let plan_str = plan_map
                .get(tool_key(r.tool))
                .copied()
                .map(plan_short)
                .unwrap_or("API");
            let credit_str = r
                .credits
                .map(|c| format!("  {} credits", compact_int(c)))
                .unwrap_or_default();
            Line::from(vec![
                Span::raw(format!("{:<8} ({:>5})  ", tool_label(r.tool), plan_str)),
                Span::raw(render_usd(r.microdollars)),
                Span::raw(credit_str),
            ])
        })
        .collect()
}

fn tool_label(t: Tool) -> &'static str {
    match t {
        Tool::Claude => "Claude",
        Tool::Codex => "Codex",
        Tool::Gemini => "Gemini",
        Tool::Opencode => "OpenCode",
        Tool::Custom => "Custom",
        Tool::Shell => "Shell",
    }
}

fn tool_key(t: Tool) -> &'static str {
    match t {
        Tool::Claude => "claude",
        Tool::Codex => "codex",
        Tool::Gemini => "gemini",
        Tool::Opencode => "opencode",
        Tool::Custom => "custom",
        Tool::Shell => "shell",
    }
}

fn plan_short(plan: crate::core::cost::Plan) -> &'static str {
    match plan {
        crate::core::cost::Plan::Api => "API",
        crate::core::cost::Plan::Pro => "Pro",
        crate::core::cost::Plan::Max5x => "Max5x",
        crate::core::cost::Plan::Max20x => "Max20x",
    }
}

fn compact_int(n: i64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let rows = match &app.storage {
        Some(s) => s
            .lock()
            .ok()
            .and_then(|guard| guard.cost_by_runner(app.cost_period).ok())
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let lines = build_runner_lines(&rows, &app.config.costs.plan, &app.theme);
    let block = Block::default().borders(Borders::ALL).title(" Per-runner ");
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;
    use std::collections::HashMap;

    #[test]
    fn empty_runners_shows_no_events_message() {
        let t = Theme::dark();
        let lines = build_runner_lines(&[], &HashMap::new(), &t);
        let rendered: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<Vec<&str>>()
            .join("");
        assert!(rendered.contains("no cost events"));
    }

    #[test]
    fn claude_row_shows_credits_codex_does_not() {
        let t = Theme::dark();
        let runners = vec![
            RunnerCost {
                tool: Tool::Claude,
                microdollars: 41_200_000,
                input_tokens: 0,
                output_tokens: 0,
                credits: Some(90_400_000),
            },
            RunnerCost {
                tool: Tool::Codex,
                microdollars: 5_130_000,
                input_tokens: 0,
                output_tokens: 0,
                credits: None,
            },
        ];
        let mut plan_map = HashMap::new();
        plan_map.insert("claude".into(), crate::core::cost::Plan::Pro);
        let lines = build_runner_lines(&runners, &plan_map, &t);
        let strs: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert!(
            strs[0].contains("Claude")
                && strs[0].contains("Pro")
                && strs[0].contains("90.4M credits")
        );
        assert!(
            strs[1].contains("Codex") && strs[1].contains("API") && !strs[1].contains("credits")
        );
    }
}
