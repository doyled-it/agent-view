//! Home screen rendering — session list with status icons

mod activity_feed;
mod claude_quota_pane;
mod codex_quota_pane;
mod header;
mod session_list;
mod session_usage_pane;
mod sparkline;
mod status_pane;

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph};
use ratatui::Frame;

use crate::app::{App, Overlay};

/// Main render function for the home screen
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // Fill entire screen with theme background so light theme works properly
    frame.render_widget(
        Block::default().style(Style::default().bg(app.theme.background)),
        area,
    );

    // When the terminal is wide enough, split horizontally: list on left, detail on right
    let detail_width = crate::ui::detail::panel_width(app.detail_mode, area.width);
    let (list_area, detail_area) =
        if area.width >= crate::ui::detail::DETAIL_PANEL_MIN_WIDTH && detail_width > 0 {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Min(0), Constraint::Length(detail_width)])
                .split(area);
            (cols[0], Some(cols[1]))
        } else {
            (area, None)
        };

    // Session-scoped chrome (activity feed, quota, usage, status) is
    // suppressed on the Costs tab — that view owns its full body area.
    let on_costs_tab = app.active_tab == crate::app::ActiveTab::Costs;

    // Layout: header, body, activity feed, usage pane, footer
    let show_feed = !on_costs_tab && app.activity.show_feed && !app.activity.feed.is_empty();
    let feed_height = if show_feed {
        // 1 for border + 1 per event, capped at 8 lines total
        let events = app.activity.feed.len().min(7) as u16;
        events + 1
    } else {
        0
    };
    let selected_tool = if on_costs_tab {
        None
    } else {
        app.selected_session().map(|s| s.tool)
    };
    let has_usage = !on_costs_tab && app.usage_state.data.is_some();
    // Claude quota only renders when the meta-tmux scrape has produced
    // data AND the selected session is Claude. Codex quota renders
    // unconditionally for Codex sessions (it can show a placeholder while
    // waiting for the first token_count event).
    let quota_height = match selected_tool {
        Some(crate::types::Tool::Claude) if has_usage => 4u16,
        Some(crate::types::Tool::Codex) => 4u16,
        _ => 0,
    };
    // Session usage pane: 1 border + up to 3 content lines (context,
    // session totals, optional $-estimate).
    let session_height: u16 = if selected_tool.is_some() { 4 } else { 0 };
    let status_incidents = app
        .status_state
        .data
        .as_ref()
        .map(|s| s.incidents.len())
        .unwrap_or(0);
    let status_height = if !on_costs_tab && app.status_state.data.is_some() {
        // 1 border + 1 description + min(incidents, 3)
        2u16 + (status_incidents.min(3) as u16)
    } else {
        0
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(6),              // ASCII header + tab bar
            Constraint::Min(0),                 // session/routine list
            Constraint::Length(feed_height),    // activity feed
            Constraint::Length(quota_height),   // tool-specific quota pane
            Constraint::Length(session_height), // per-session usage pane
            Constraint::Length(status_height),  // status pane
            Constraint::Length(1),              // footer
        ])
        .split(list_area);

    header::render_header(frame, chunks[0], app);
    match app.active_tab {
        crate::app::ActiveTab::Sessions => session_list::render_session_list(frame, chunks[1], app),
        crate::app::ActiveTab::Routines => {
            crate::ui::routines::render_routine_list(frame, chunks[1], app)
        }
        crate::app::ActiveTab::Costs => crate::ui::costs::render_costs_tab(frame, chunks[1], app),
    }
    if show_feed {
        activity_feed::render_activity_feed(frame, chunks[2], app);
    }
    if let Some(tool) = selected_tool {
        match tool {
            crate::types::Tool::Claude if has_usage => {
                claude_quota_pane::render_claude_quota_pane(frame, chunks[3], app);
            }
            crate::types::Tool::Codex => {
                codex_quota_pane::render_codex_quota_pane(frame, chunks[3], app);
            }
            _ => {}
        }
        session_usage_pane::render_session_usage_pane(frame, chunks[4], app);
    }
    if !on_costs_tab && app.status_state.data.is_some() {
        status_pane::render_status_pane(frame, chunks[5], app);
    }
    if let Some(ref query) = app.search_query {
        let matches = app.search_matches();
        let match_count = matches.len();
        let search_line = Line::from(vec![
            Span::styled(" / ", Style::default().fg(app.theme.primary).bold()),
            Span::styled(query.as_str(), Style::default().fg(app.theme.text)),
            Span::styled("\u{2588}", Style::default().fg(app.theme.primary)),
            Span::styled(
                format!(
                    "  {} match{}",
                    match_count,
                    if match_count == 1 { "" } else { "es" }
                ),
                Style::default().fg(app.theme.text_muted),
            ),
        ]);
        frame.render_widget(Paragraph::new(search_line), chunks[6]);
    } else {
        crate::ui::footer::render(frame, chunks[6], app);
    }

    // Render detail panel when wide enough
    if let Some(detail_rect) = detail_area {
        match app.active_tab {
            crate::app::ActiveTab::Sessions => {
                if let Some(session) = app.selected_session() {
                    crate::ui::detail::render_detail_panel(
                        frame,
                        detail_rect,
                        session,
                        &app.theme,
                        app.detail_mode,
                        &app.preview.content,
                    );
                }
            }
            crate::app::ActiveTab::Routines => {
                match app
                    .routine_state
                    .list_rows
                    .get(app.routine_state.selected_index)
                {
                    Some(crate::app::RoutineListRow::Routine(routine)) => {
                        crate::ui::detail::render_routine_detail(
                            frame,
                            detail_rect,
                            routine,
                            &app.theme,
                            app.detail_mode,
                            &app.preview.content,
                        );
                    }
                    Some(crate::app::RoutineListRow::Run { run, routine_name }) => {
                        crate::ui::detail::render_run_detail(
                            frame,
                            detail_rect,
                            run,
                            routine_name,
                            &app.theme,
                            app.detail_mode,
                            &app.preview.content,
                        );
                    }
                    _ => {}
                }
            }
            crate::app::ActiveTab::Costs => {
                // Costs tab has no per-item detail panel.
            }
        }
    }

    // Render overlay on top if active
    match &app.overlay {
        Overlay::NewSession(form) => {
            crate::ui::overlay::render_new_session(frame, area, form, &app.theme);
        }
        Overlay::NewRoutine(form) => {
            crate::ui::overlay::render_new_routine(frame, area, form, &app.theme);
        }
        Overlay::Confirm(dialog) => {
            crate::ui::overlay::render_confirm(frame, area, dialog, &app.theme);
        }
        Overlay::Rename(form) => {
            crate::ui::overlay::render_rename(frame, area, form, &app.theme);
        }
        Overlay::Move(form) => {
            crate::ui::overlay::render_move(frame, area, form, &app.theme);
        }
        Overlay::GroupManage(form) => {
            crate::ui::overlay::render_group_manage(frame, area, form, &app.theme);
        }
        Overlay::CommandPalette(palette) => {
            crate::ui::overlay::render_command_palette(frame, area, palette, &app.theme);
        }
        Overlay::Help => {
            crate::ui::overlay::render_help(frame, area, app);
        }
        Overlay::ThemeSelect(form) => {
            crate::ui::overlay::render_theme_select(frame, area, form, &app.theme);
        }
        Overlay::AddNote(form) => {
            crate::ui::overlay::render_add_note(frame, area, form, &app.theme);
        }
        Overlay::RoutineWarning => {
            crate::ui::overlay::render_routine_warning(frame, area, &app.theme);
        }
        Overlay::None => {}
    }
}
