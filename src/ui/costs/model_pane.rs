//! Per-model cost breakdown.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::core::cost::{render_usd, ModelCost};
use crate::ui::theme::Theme;

pub fn build_model_lines<'a>(models: &[ModelCost], theme: &'a Theme) -> Vec<Line<'a>> {
    if models.is_empty() {
        return vec![Line::from(Span::styled(
            "no cost events",
            Style::default().fg(theme.text_muted),
        ))];
    }
    models
        .iter()
        .map(|m| {
            Line::from(vec![
                Span::raw(format!("{:<22}", truncate(&m.model, 22))),
                Span::raw(render_usd(m.microdollars)),
            ])
        })
        .collect()
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        s.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let rows = match &app.storage {
        Some(s) => s
            .lock()
            .ok()
            .and_then(|guard| guard.cost_by_model(app.cost_period).ok())
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let lines = build_model_lines(&rows, &app.theme);
    let block = Block::default().borders(Borders::ALL).title(" By model ");
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::Theme;

    #[test]
    fn empty_models_shows_no_events() {
        let t = Theme::dark();
        let lines = build_model_lines(&[], &t);
        let txt: String = lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<Vec<&str>>()
            .join("");
        assert!(txt.contains("no cost events"));
    }

    #[test]
    fn model_lines_render_in_order() {
        let t = Theme::dark();
        let models = vec![
            ModelCost {
                model: "claude-opus-4-7".into(),
                microdollars: 38_410_000,
                credits: Some(100_000),
            },
            ModelCost {
                model: "gpt-5.5".into(),
                microdollars: 5_130_000,
                credits: None,
            },
        ];
        let lines = build_model_lines(&models, &t);
        assert_eq!(lines.len(), 2);
        let first: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(first.contains("claude-opus-4-7"));
        assert!(first.contains("$38.41"));
    }

    #[test]
    fn long_model_name_truncates() {
        assert_eq!(truncate("short", 22), "short");
        assert_eq!(
            truncate("very-very-very-long-model-name-here", 22)
                .chars()
                .count(),
            22
        );
    }
}
