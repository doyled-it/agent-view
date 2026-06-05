use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::app::McpSyncForm;
use crate::core::mcp::McpSyncAvailability;
use crate::ui::theme::Theme;

pub fn render_mcp_sync(frame: &mut Frame, area: Rect, form: &McpSyncForm, theme: &Theme) {
    let overlay_width = 84u16.min(area.width.saturating_sub(4));
    let overlay_height = 24u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let title = if form.confirming {
        " Confirm MCP Sync "
    } else {
        " MCP Sync "
    };
    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active));
    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(1),
            Constraint::Length(6),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    frame.render_widget(Paragraph::new(inventory_lines(form, theme)), chunks[0]);
    frame.render_widget(
        Paragraph::new("Actions").style(Style::default().fg(theme.text_muted)),
        chunks[1],
    );
    frame.render_widget(List::new(action_items(form, theme)), chunks[2]);
    frame.render_widget(
        Paragraph::new("Preview").style(Style::default().fg(theme.text_muted)),
        chunks[3],
    );
    frame.render_widget(Paragraph::new(preview_lines(form, theme)), chunks[4]);
}

fn inventory_lines(form: &McpSyncForm, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![
            Span::styled("Server", Style::default().fg(theme.text).bold()),
            Span::raw("        "),
            Span::styled("Claude", Style::default().fg(theme.text).bold()),
            Span::raw("   "),
            Span::styled("Codex", Style::default().fg(theme.text).bold()),
            Span::raw("   "),
            Span::styled("OpenCode", Style::default().fg(theme.text).bold()),
        ]),
        Line::from(""),
    ];

    for row in form.plan.inventory_rows.iter().take(5) {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<13}", row.server_id),
                Style::default().fg(theme.text),
            ),
            Span::styled(
                format!("{:<8}", availability_label(&row.claude)),
                availability_style(&row.claude, theme),
            ),
            Span::styled(
                format!("{:<8}", availability_label(&row.codex)),
                availability_style(&row.codex, theme),
            ),
            Span::styled(
                availability_label(&row.opencode),
                availability_style(&row.opencode, theme),
            ),
        ]));
    }

    if form.plan.inventory_rows.is_empty() {
        lines.push(Line::from(Span::styled(
            "No Claude or Codex MCP servers found",
            Style::default().fg(theme.text_muted),
        )));
    }

    lines
}

fn action_items(form: &McpSyncForm, theme: &Theme) -> Vec<ListItem<'static>> {
    if form.plan.proposals.is_empty() {
        return vec![ListItem::new(Line::from(Span::styled(
            "No sync actions available",
            Style::default().fg(theme.text_muted),
        )))];
    }

    form.plan
        .proposals
        .iter()
        .enumerate()
        .take(5)
        .map(|(idx, proposal)| {
            let selected = idx == form.selected;
            let style = if selected {
                Style::default()
                    .bg(theme.primary)
                    .fg(theme.selected_item_text)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(Line::from(Span::styled(
                format!(
                    "{} {} -> {}",
                    proposal.server_id, proposal.source, proposal.target
                ),
                style,
            )))
        })
        .collect()
}

fn preview_lines(form: &McpSyncForm, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if form.confirming {
        lines.push(Line::from(Span::styled(
            "Apply this change?",
            Style::default().fg(theme.warning).bold(),
        )));
    }
    match form.selected_proposal() {
        Some(proposal) => {
            for line in proposal.preview_lines.iter().take(8) {
                lines.push(Line::from(Span::styled(
                    line.clone(),
                    Style::default().fg(theme.text),
                )));
            }
        }
        None => lines.push(Line::from(Span::styled(
            "No changes selected",
            Style::default().fg(theme.text_muted),
        ))),
    }
    lines
}

fn availability_label(availability: &McpSyncAvailability) -> String {
    match availability {
        McpSyncAvailability::Configured => "enabled".to_string(),
        McpSyncAvailability::Missing => "missing".to_string(),
        McpSyncAvailability::Unsupported(_) => "unsupported".to_string(),
    }
}

fn availability_style(availability: &McpSyncAvailability, theme: &Theme) -> Style {
    match availability {
        McpSyncAvailability::Configured => Style::default().fg(theme.success),
        McpSyncAvailability::Missing => Style::default().fg(theme.warning),
        McpSyncAvailability::Unsupported(_) => Style::default().fg(theme.text_muted),
    }
}
