use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui::Frame;

use crate::app::ThemeSelectForm;
use crate::ui::theme::Theme;

/// Render the theme selection overlay with live preview
pub fn render_theme_select(frame: &mut Frame, area: Rect, form: &ThemeSelectForm, theme: &Theme) {
    let width = area.width.min(30);
    let height = (form.options.len() as u16 + 2).min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(area.x + x, area.y + y, width, height);

    frame.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Theme ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let items: Vec<ListItem> = form
        .options
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let is_selected = i == form.selected;
            let style = if is_selected {
                Style::default()
                    .fg(theme.selected_item_text)
                    .bg(theme.primary)
                    .bold()
            } else {
                Style::default().fg(theme.text)
            };
            ListItem::new(format!("  {}  ", name)).style(style)
        })
        .collect();

    frame.render_widget(List::new(items), inner);
}
