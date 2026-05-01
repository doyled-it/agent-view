use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph};
use ratatui::Frame;

use crate::app::CommandPalette;
use crate::ui::theme::Theme;

/// Render the command palette overlay — centered searchable list of actions
pub fn render_command_palette(
    frame: &mut Frame,
    area: Rect,
    palette: &CommandPalette,
    theme: &Theme,
) {
    let max_items = 10;
    let visible = palette.filtered.len().min(max_items);
    let overlay_height = (visible as u16 + 4).min(area.height.saturating_sub(4));
    let overlay_width = 50u16.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = area.height / 6; // Near the top
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" Commands ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    // Search input
    let input_line = Line::from(vec![
        Span::styled("> ", Style::default().fg(theme.primary)),
        Span::styled(palette.query.as_str(), Style::default().fg(theme.text)),
        Span::styled("\u{2588}", Style::default().fg(theme.primary)),
    ]);
    frame.render_widget(Paragraph::new(input_line), chunks[0]);

    // Filtered items
    let items: Vec<ListItem> = palette
        .filtered
        .iter()
        .enumerate()
        .take(max_items)
        .map(|(i, &idx)| {
            let item = &palette.items[idx];
            let style = if i == palette.selected {
                Style::default()
                    .bg(theme.primary)
                    .fg(theme.selected_item_text)
            } else {
                Style::default().fg(theme.text)
            };
            let line = Line::from(vec![
                Span::styled(format!("  {} ", item.label), style),
                Span::styled(
                    format!("  {}", item.key_hint),
                    if i == palette.selected {
                        Style::default()
                            .bg(theme.primary)
                            .fg(theme.selected_item_text)
                    } else {
                        Style::default().fg(theme.text_muted)
                    },
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    frame.render_widget(List::new(items), chunks[1]);
}
