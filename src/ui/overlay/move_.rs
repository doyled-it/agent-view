use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui::Frame;

use crate::app::MoveForm;
use crate::ui::theme::Theme;

/// Render the move session overlay — list of groups to choose from
pub fn render_move(frame: &mut Frame, area: Rect, form: &MoveForm, theme: &Theme) {
    let overlay_height = (form.groups.len() as u16 + 4).min(area.height.saturating_sub(4));
    let overlay_width = 50u16.min(area.width.saturating_sub(4));
    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let title = format!(" Move \"{}\" ", form.session_title);
    let block = Block::default()
        .title(title)
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let items: Vec<ListItem> = form
        .groups
        .iter()
        .enumerate()
        .map(|(i, (_, name))| {
            let style = if i == form.selected {
                Style::default()
                    .bg(theme.primary)
                    .fg(theme.selected_item_text)
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(format!("  {}", name)).style(style)
        })
        .collect();

    frame.render_widget(List::new(items), inner);
}
