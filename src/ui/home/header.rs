//! Header panel rendering (logo + tab bar)

use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::Frame;

use crate::app::{ActiveTab, App};

const LOGO: [&str; 4] = [
    r"  __    ___  ____  __ _  ____    _  _  __  ____  _  _ ",
    r" / _\  / __)(  __)(  ( \(_  _)  / )( \(  )(  __)/ )( \",
    r"/    \( (_ \ ) _) /    /  )(    \ \/ / )(  ) _) \ /\ /",
    r"\_/\_/ \___/(____)\_)__) (__)    \__/ (__)(____)(_/\_)",
];

pub(super) fn render_header(frame: &mut Frame, area: Rect, app: &App) {
    let version = env!("CARGO_PKG_VERSION");

    let logo_lines: &[&str] = &LOGO;

    let theme = &app.theme;
    let active_tab = app.active_tab;
    let primary_style = Style::default().fg(theme.primary).bold();
    let muted_style = Style::default().fg(theme.text_muted);

    let area_width = area.width as usize;
    let mut lines: Vec<Line> = logo_lines
        .iter()
        .map(|line| {
            let pad = area_width.saturating_sub(line.len()) / 2;
            Line::from(Span::styled(
                format!("{:>width$}{}", "", line, width = pad),
                primary_style,
            ))
        })
        .collect();
    lines.push(Line::from(""));

    // Tab bar line
    let tab_line = Line::from(vec![
        Span::styled("  ", muted_style),
        Span::styled(
            " Sessions ",
            if active_tab == ActiveTab::Sessions {
                Style::default()
                    .fg(theme.selected_item_text)
                    .bg(theme.primary)
                    .bold()
            } else {
                muted_style
            },
        ),
        Span::styled(" ", muted_style),
        Span::styled(
            " Routines ",
            if active_tab == ActiveTab::Routines {
                Style::default()
                    .fg(theme.selected_item_text)
                    .bg(theme.primary)
                    .bold()
            } else {
                muted_style
            },
        ),
        Span::styled(
            format!("  v{}", version),
            Style::default().fg(theme.text_muted),
        ),
    ]);
    lines.push(tab_line);

    frame.render_widget(Paragraph::new(lines), area);
}
