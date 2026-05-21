//! Costs tab UI. Composes per-period summary, per-runner, per-model, and
//! top-session panes. All read-only; aggregation queries live in
//! `core::storage::cost_aggregation`.

pub mod summary_pane;

use ratatui::layout::Rect;
use ratatui::Frame;

use crate::app::App;

pub fn render_costs_tab(frame: &mut Frame, area: Rect, app: &App) {
    summary_pane::render(frame, area, app);
}
