use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{McpProfilesForm, McpProfilesMode};
use crate::ui::theme::Theme;

pub fn render_mcp_profiles(frame: &mut Frame, area: Rect, form: &McpProfilesForm, theme: &Theme) {
    let overlay_width = 72.min(area.width.saturating_sub(4));
    let overlay_height = 18.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" MCP Profiles ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active));
    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    let lines = match &form.mode {
        McpProfilesMode::List => list_lines(form, theme),
        McpProfilesMode::Edit(_) => edit_lines(form, theme),
    };
    frame.render_widget(Paragraph::new(lines), chunks[0]);

    let help = match &form.mode {
        McpProfilesMode::List => "n new - Enter/e edit - c duplicate - d/Del delete - Esc close",
        McpProfilesMode::Edit(_) => "Ctrl+S save - Tab field - Space toggle - Esc cancel",
    };
    frame.render_widget(
        Paragraph::new(help).style(Style::default().fg(theme.text_muted)),
        chunks[1],
    );
}

fn list_lines(form: &McpProfilesForm, theme: &Theme) -> Vec<Line<'static>> {
    if form.profiles.is_empty() {
        return vec![Line::from(Span::styled(
            "No MCP profiles yet",
            Style::default().fg(theme.text_muted),
        ))];
    }

    form.profiles
        .iter()
        .enumerate()
        .map(|(idx, profile)| {
            let style = if idx == form.selected_profile {
                Style::default()
                    .bg(theme.primary)
                    .fg(theme.selected_item_text)
                    .bold()
            } else {
                Style::default().fg(theme.text)
            };
            Line::from(Span::styled(
                format!("{}  {}", profile.name, profile.selection.summary()),
                style,
            ))
        })
        .collect()
}

fn edit_lines(form: &McpProfilesForm, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            format!("Name: {}", form.name_input),
            if form.focused_field == 0 {
                Style::default().fg(theme.primary)
            } else {
                Style::default().fg(theme.text)
            },
        )),
        Line::from(""),
    ];

    for (idx, row) in form.server_rows().iter().enumerate() {
        let marker = if row.enabled { "[x]" } else { "[ ]" };
        let missing = if row.missing { " missing" } else { "" };
        let style = if form.focused_field == 1 && idx == form.selected_server {
            Style::default()
                .bg(theme.primary)
                .fg(theme.selected_item_text)
                .bold()
        } else if row.missing {
            Style::default().fg(theme.warning)
        } else {
            Style::default().fg(theme.text)
        };
        lines.push(Line::from(Span::styled(
            format!("  {} {}{}", marker, row.display_name, missing),
            style,
        )));
    }

    if let Some(error) = &form.error {
        lines.push(Line::from(Span::styled(
            format!("Error: {error}"),
            Style::default().fg(theme.error),
        )));
    }

    lines
}
