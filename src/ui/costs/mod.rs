//! Costs tab UI. Composes per-period summary, per-runner, per-model, and
//! top-session panes. All read-only; aggregation queries live in
//! `core::storage::cost_aggregation`.

pub mod model_pane;
pub mod runner_pane;
pub mod summary_pane;
pub mod top_sessions_pane;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::App;

fn render_period_bar(frame: &mut Frame, area: Rect, app: &App) {
    let active = app.cost_period;
    let theme = &app.theme;
    let mut spans = vec![Span::styled(
        "Period: ◀ ",
        Style::default().fg(theme.text_muted),
    )];
    for (i, p) in crate::core::cost::CostPeriod::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        let label = p.label();
        let style = if *p == active {
            Style::default()
                .fg(theme.text)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(theme.text_muted)
        };
        spans.push(Span::styled(label.to_string(), style));
    }
    spans.push(Span::styled(
        "  ▶  (←/→ to change)",
        Style::default().fg(theme.text_muted),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

pub fn render_costs_tab(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Period bar
            Constraint::Length(7), // Summary
            Constraint::Length(7), // Per-runner
            Constraint::Min(0),    // bottom row
        ])
        .split(area);
    render_period_bar(frame, chunks[0], app);
    summary_pane::render(frame, chunks[1], app);
    runner_pane::render(frame, chunks[2], app);

    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[3]);
    model_pane::render(frame, bottom[0], app);
    top_sessions_pane::render(frame, bottom[1], app);
}
