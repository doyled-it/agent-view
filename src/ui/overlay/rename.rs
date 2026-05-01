use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use ratatui::Frame;

use crate::app::{RenameForm, RenameTarget};
use crate::ui::theme::Theme;

/// Render the rename overlay for sessions and groups
pub fn render_rename(frame: &mut Frame, area: Rect, form: &RenameForm, theme: &Theme) {
    let overlay_width = 50u16.min(area.width.saturating_sub(4));
    let overlay_height = 5u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let title = match form.target_type {
        RenameTarget::Session => " Rename Session ",
        RenameTarget::Group => " Rename Group ",
        RenameTarget::Routine => " Rename Routine ",
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
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new("New name:").style(Style::default().fg(theme.text_muted)),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(format!("{}\u{2588}", form.input)).style(Style::default().fg(theme.text)),
        chunks[1],
    );
}
