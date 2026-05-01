use ratatui::layout::Rect;
use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui::Frame;

use crate::ui::theme::Theme;

/// Render the routine permissions warning dialog
pub fn render_routine_warning(frame: &mut Frame, area: Rect, theme: &Theme) {
    let overlay_width = 50u16.min(area.width.saturating_sub(4));
    let overlay_height = 12u16.min(area.height.saturating_sub(4));

    let x = (area.width.saturating_sub(overlay_width)) / 2;
    let y = (area.height.saturating_sub(overlay_height)) / 2;
    let overlay_area = Rect::new(x, y, overlay_width, overlay_height);

    frame.render_widget(Clear, overlay_area);

    let block = Block::default()
        .title(" \u{26A0} Routines \u{26A0} ")
        .title_style(Style::default().fg(theme.warning).bold())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.warning));

    let inner = block.inner(overlay_area);
    frame.render_widget(block, overlay_area);

    let warn = "\u{26A0}";
    let lines = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            format!("  {}  PERMISSIONS BYPASSED  {}", warn, warn),
            Style::default().fg(theme.warning).bold(),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Routines run unattended. Claude steps",
            Style::default().fg(theme.text),
        )]),
        Line::from(vec![Span::styled(
            "  execute with all permission checks",
            Style::default().fg(theme.text),
        )]),
        Line::from(vec![Span::styled(
            "  bypassed \u{2014} commands, file edits, and",
            Style::default().fg(theme.text),
        )]),
        Line::from(vec![Span::styled(
            "  network access run without approval.",
            Style::default().fg(theme.text),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Enter ", Style::default().fg(theme.secondary).bold()),
            Span::styled("I understand", Style::default().fg(theme.text)),
            Span::styled("   Esc ", Style::default().fg(theme.secondary).bold()),
            Span::styled("go back", Style::default().fg(theme.text)),
        ]),
    ];

    frame.render_widget(Paragraph::new(lines), inner);
}
