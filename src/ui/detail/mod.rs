//! Detail panel — shows session metadata and/or terminal preview on the right side

mod compat;
mod format;
mod routine;
mod run;
mod session;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::Frame;

use crate::app::DetailPanelMode;
use crate::types::{Routine, RoutineRun, Session};
use crate::ui::theme::Theme;

/// Minimum terminal width to show the detail panel
pub const DETAIL_PANEL_MIN_WIDTH: u16 = 80;

/// Width of the panel when showing preview (wider modes)
pub const WIDE_PANEL_PERCENT: u16 = 45;

/// Width of the panel when showing metadata only
pub const NARROW_PANEL_WIDTH: u16 = 36;

/// Dispatch rendering to the appropriate sub-renderer based on mode
pub fn render_detail_panel(
    frame: &mut Frame,
    area: Rect,
    session: &Session,
    theme: &Theme,
    mode: DetailPanelMode,
    preview_content: &str,
) {
    match mode {
        DetailPanelMode::None => {}
        DetailPanelMode::Preview => {
            session::render_preview(frame, area, session, theme, preview_content);
        }
        DetailPanelMode::Metadata => {
            session::render_metadata(frame, area, session, theme);
        }
        DetailPanelMode::Both => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(area);
            session::render_preview(frame, chunks[0], session, theme, preview_content);
            session::render_metadata(frame, chunks[1], session, theme);
        }
    }
}

/// Compute the panel width based on mode and terminal width
pub fn panel_width(mode: DetailPanelMode, terminal_width: u16) -> u16 {
    match mode {
        DetailPanelMode::None => 0,
        DetailPanelMode::Metadata => NARROW_PANEL_WIDTH,
        DetailPanelMode::Preview | DetailPanelMode::Both => {
            (terminal_width * WIDE_PANEL_PERCENT / 100).max(NARROW_PANEL_WIDTH)
        }
    }
}

/// Render detail panel for a routine
pub fn render_routine_detail(
    frame: &mut Frame,
    area: Rect,
    routine: &Routine,
    theme: &Theme,
    mode: DetailPanelMode,
    preview_content: &str,
) {
    match mode {
        DetailPanelMode::None => {}
        DetailPanelMode::Preview => {
            routine::render_routine_preview(frame, area, theme, preview_content);
        }
        DetailPanelMode::Metadata => {
            routine::render_routine_metadata(frame, area, routine, theme);
        }
        DetailPanelMode::Both => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(area);
            routine::render_routine_preview(frame, chunks[0], theme, preview_content);
            routine::render_routine_metadata(frame, chunks[1], routine, theme);
        }
    }
}

/// Render detail panel for a run
pub fn render_run_detail(
    frame: &mut Frame,
    area: Rect,
    run: &RoutineRun,
    routine_name: &str,
    theme: &Theme,
    mode: DetailPanelMode,
    preview_content: &str,
) {
    match mode {
        DetailPanelMode::None => {}
        DetailPanelMode::Preview => {
            routine::render_routine_preview(frame, area, theme, preview_content);
        }
        DetailPanelMode::Metadata => {
            run::render_run_metadata(frame, area, run, routine_name, theme);
        }
        DetailPanelMode::Both => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(area);
            routine::render_routine_preview(frame, chunks[0], theme, preview_content);
            run::render_run_metadata(frame, chunks[1], run, routine_name, theme);
        }
    }
}
