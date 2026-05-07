//! Claude status pane rendering

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::types::StatusIndicator;

pub(super) fn render_status_pane(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;

    let block = Block::default()
        .title(" Claude Status ")
        .title_style(Style::default().fg(theme.text_muted))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (icon, color, description) = match app.status_state.data {
        Some(ref s) => {
            let color = match s.indicator {
                StatusIndicator::None => theme.success,
                StatusIndicator::Minor | StatusIndicator::Maintenance => theme.warning,
                StatusIndicator::Major | StatusIndicator::Critical => theme.error,
            };
            let icon = match s.indicator {
                StatusIndicator::None => "\u{25CF}",
                StatusIndicator::Minor | StatusIndicator::Maintenance => "\u{25D0}",
                StatusIndicator::Major | StatusIndicator::Critical => "\u{26A0}",
            };
            (icon, color, s.description.as_str())
        }
        None => ("\u{25CB}", theme.text_muted, "status: unknown"),
    };

    let mut lines: Vec<Line> = vec![Line::from(vec![
        Span::styled(format!(" {} ", icon), Style::default().fg(color)),
        Span::styled(description.to_string(), Style::default().fg(theme.text)),
    ])];

    if let Some(ref s) = app.status_state.data {
        let max_incidents = inner.height.saturating_sub(1) as usize;
        for inc in s.incidents.iter().take(max_incidents) {
            lines.push(Line::from(vec![
                Span::styled("   \u{2022} ", Style::default().fg(theme.text_muted)),
                Span::styled(inc.name.clone(), Style::default().fg(theme.text)),
                Span::styled(
                    format!("  ({})", inc.status),
                    Style::default().fg(theme.text_muted),
                ),
            ]));
        }
    }

    frame.render_widget(Paragraph::new(lines), inner);
}
