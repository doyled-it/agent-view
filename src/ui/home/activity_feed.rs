//! Activity feed panel rendering

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::app::App;
use crate::ui::theme::status_color;

pub(super) fn render_activity_feed(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let block = Block::default()
        .title(" Activity ")
        .title_style(super::pane_title_style(theme))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.border));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let events: Vec<_> = app
        .activity
        .feed
        .iter()
        .take(inner.height as usize)
        .map(|event| (format_activity_age(event.timestamp), event))
        .collect();

    let age_width = events
        .iter()
        .map(|(age, _)| age.chars().count())
        .max()
        .unwrap_or(0);
    let title_width = events
        .iter()
        .map(|(_, event)| event.session_title.chars().count())
        .max()
        .unwrap_or(0);

    let lines: Vec<Line> = events
        .iter()
        .map(|(age, event)| {
            let status_color = status_color(theme, event.new_status);
            Line::from(vec![
                Span::styled(
                    format!(" {age:>age_width$}  "),
                    Style::default().fg(theme.text_muted),
                ),
                Span::styled(
                    format!("{:<title_width$} ", event.session_title),
                    Style::default().fg(theme.text),
                ),
                Span::styled("-> ", Style::default().fg(theme.text_muted)),
                Span::styled(event.new_status.as_str(), Style::default().fg(status_color)),
            ])
        })
        .collect();

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn format_activity_age(timestamp: i64) -> String {
    let now = chrono::Utc::now().timestamp_millis();
    let ago_ms = now - timestamp;
    if ago_ms < 60_000 {
        "<1m".to_string()
    } else if ago_ms < 3_600_000 {
        format!("{}m", ago_ms / 60_000)
    } else {
        format!("{}h", ago_ms / 3_600_000)
    }
}
