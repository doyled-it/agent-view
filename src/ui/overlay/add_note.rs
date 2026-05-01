use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui::Frame;

use crate::app::NoteForm;
use crate::ui::theme::Theme;

/// Render the add note overlay
pub fn render_add_note(frame: &mut Frame, area: Rect, form: &NoteForm, theme: &Theme) {
    let overlay_width = 60u16.min(area.width.saturating_sub(4));
    let overlay_height = 12u16.min(area.height.saturating_sub(4));

    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" Add Note ")
        .title_style(Style::default().fg(theme.primary).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border_active));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    // Text area with cursor
    let display_text = format!("{}\u{2588}", form.text);
    let text_widget = Paragraph::new(display_text)
        .style(Style::default().fg(theme.text))
        .wrap(Wrap { trim: false });
    frame.render_widget(text_widget, inner);
}
