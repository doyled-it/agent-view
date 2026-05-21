//! Costs tab UI. Composes per-period summary, per-runner, per-model, and
//! top-session panes. All read-only; aggregation queries live in
//! `core::storage::cost_aggregation`.

pub mod model_pane;
pub mod runner_pane;
pub mod summary_pane;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::app::App;

pub fn render_costs_tab(frame: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),  // Summary
            Constraint::Length(7),  // Per-runner
            Constraint::Min(0),     // bottom row
        ])
        .split(area);
    summary_pane::render(frame, chunks[0], app);
    runner_pane::render(frame, chunks[1], app);

    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[2]);
    model_pane::render(frame, bottom[0], app);
    // bottom[1] populated in Task 13
}
