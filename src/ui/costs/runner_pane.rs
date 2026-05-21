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
    detected_labels: &std::collections::HashMap<String, String>,
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
            // Resolution order: explicit config plan > runtime-detected label
            // (e.g. Codex `plan_type: "business"`) > literal "API".
            let key = tool_key(r.tool);
            let plan_str: String = if let Some(plan) = plan_map.get(key).copied() {
                plan_short(plan).to_string()
            } else if let Some(label) = detected_labels.get(key) {
                title_case(label)
            } else {
                "API".to_string()
            };
            let credit_str = r
                .credits
                .map(|c| format!("  {} credits", compact_int(c)))
                .unwrap_or_default();
            Line::from(vec![
                Span::raw(format!("{:<8} ({:<14})  ", tool_label(r.tool), plan_str)),
                Span::raw(render_usd(r.microdollars)),
                Span::raw(credit_str),
            ])
        })
        .collect()
}

fn title_case(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
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

/// Build the display label for the Claude row from a detected account.
/// Returns `None` when neither tier nor org type are known (so the
/// renderer falls back to "API"). Output examples:
///   - Max5x personal → "Max5x"
///   - Max5x team     → "Max5x · Team"
///   - Unknown tier on team account → "Team"
fn claude_label_from_account(acct: &crate::core::cost::ClaudeAccount) -> Option<String> {
    let tier = acct.plan.map(plan_short);
    match (tier, acct.is_team()) {
        (Some(t), true) => Some(format!("{} · Team", t)),
        (Some(t), false) => Some(t.to_string()),
        (None, true) => Some("Team".to_string()),
        (None, false) => None,
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
    // Runtime-detected per-runner plan labels for rows where the user
    // hasn't pinned a value in `costs.plan`. Sourced from:
    //   - Codex: `rate_limits.plan_type` in any cached rollout snapshot.
    //   - Claude: `oauthAccount` in `~/.claude.json` — tier maps to a
    //     Plan, organizationType adds a "Team" suffix when applicable.
    let mut detected: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Some(state) = &app.event_state {
        if let Ok(guard) = state.lock() {
            if let Some(plan) = guard.detected_codex_plan() {
                detected.insert("codex".to_string(), plan);
            }
        }
    }
    let claude_acct = crate::core::cost::detect_claude_account();
    if let Some(label) = claude_label_from_account(&claude_acct) {
        detected.insert("claude".to_string(), label);
    }
    let lines = build_runner_lines(&rows, &app.config.costs.plan, &detected, &app.theme);
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
        let lines = build_runner_lines(&[], &HashMap::new(), &HashMap::new(), &t);
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
        let lines = build_runner_lines(&runners, &plan_map, &HashMap::new(), &t);
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

    #[test]
    fn detected_label_shown_when_no_config_plan_set() {
        // No `costs.plan.codex` configured but the runtime detected
        // `plan_type: "business"` from a rollout snapshot. The row should
        // show "Business" instead of the misleading "API".
        let t = Theme::dark();
        let runners = vec![RunnerCost {
            tool: Tool::Codex,
            microdollars: 5_130_000,
            input_tokens: 0,
            output_tokens: 0,
            credits: None,
        }];
        let mut detected = HashMap::new();
        detected.insert("codex".to_string(), "business".to_string());
        let lines = build_runner_lines(&runners, &HashMap::new(), &detected, &t);
        let s: String = lines[0]
            .spans
            .iter()
            .map(|sp| sp.content.as_ref())
            .collect();
        assert!(s.contains("Codex"));
        assert!(s.contains("Business"));
        assert!(!s.contains("API"));
    }

    #[test]
    fn config_plan_wins_over_detected_label() {
        // When both a config Plan and a detected string are present, the
        // explicit config value takes precedence.
        let t = Theme::dark();
        let runners = vec![RunnerCost {
            tool: Tool::Codex,
            microdollars: 5_130_000,
            input_tokens: 0,
            output_tokens: 0,
            credits: None,
        }];
        let mut plan_map = HashMap::new();
        plan_map.insert("codex".into(), crate::core::cost::Plan::Pro);
        let mut detected = HashMap::new();
        detected.insert("codex".to_string(), "business".to_string());
        let lines = build_runner_lines(&runners, &plan_map, &detected, &t);
        let s: String = lines[0]
            .spans
            .iter()
            .map(|sp| sp.content.as_ref())
            .collect();
        assert!(s.contains("Pro"));
        assert!(!s.contains("Business"));
    }

    #[test]
    fn claude_label_individual_max5x() {
        let acct = crate::core::cost::ClaudeAccount {
            plan: Some(crate::core::cost::Plan::Max5x),
            org_type: Some("individual".to_string()),
        };
        assert_eq!(claude_label_from_account(&acct).as_deref(), Some("Max5x"));
    }

    #[test]
    fn claude_label_team_max5x_includes_team_suffix() {
        let acct = crate::core::cost::ClaudeAccount {
            plan: Some(crate::core::cost::Plan::Max5x),
            org_type: Some("claude_team".to_string()),
        };
        assert_eq!(
            claude_label_from_account(&acct).as_deref(),
            Some("Max5x · Team")
        );
    }

    #[test]
    fn claude_label_team_unknown_tier_falls_back_to_team_only() {
        let acct = crate::core::cost::ClaudeAccount {
            plan: None,
            org_type: Some("claude_team".to_string()),
        };
        assert_eq!(claude_label_from_account(&acct).as_deref(), Some("Team"));
    }

    #[test]
    fn claude_label_empty_account_returns_none() {
        let acct = crate::core::cost::ClaudeAccount::default();
        assert_eq!(claude_label_from_account(&acct), None);
    }
}
