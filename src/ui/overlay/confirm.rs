use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::ConfirmDialog;
use crate::ui::theme::Theme;

/// Render a confirmation dialog as a centered overlay
pub fn render_confirm(frame: &mut Frame, area: Rect, dialog: &ConfirmDialog, theme: &Theme) {
    let overlay_width = 50u16.min(area.width.saturating_sub(4));
    let overlay_height = 5u16.min(area.height.saturating_sub(4));

    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" Confirm ")
        .title_style(Style::default().fg(theme.warning).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.warning));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(dialog.message.as_str()).style(Style::default().fg(theme.text)),
        chunks[0],
    );

    frame.render_widget(
        Paragraph::new("y/Enter = yes, n/Esc = no").style(Style::default().fg(theme.text_muted)),
        chunks[1],
    );
}
