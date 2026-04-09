//! Context-sensitive footer with keybind hints

use crate::app::{App, Overlay};
use ratatui::prelude::*;
use ratatui::widgets::*;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;

    // Show toast if active and not expired
    if let Some(ref msg) = app.toast_message {
        if app.toast_expire.map_or(false, |t| t > std::time::Instant::now()) {
            let toast = Line::from(Span::styled(
                msg.as_str(),
                Style::default().fg(theme.info).bold(),
            ));
            frame.render_widget(Paragraph::new(toast), area);
            return;
        }
    }

    let hints: Vec<(&str, &str)> = match &app.overlay {
        Overlay::None => {
            if app.sessions.is_empty() {
                vec![("n", "new"), ("q", "quit")]
            } else {
                vec![
                    ("j/k", "navigate"),
                    ("Enter", "attach"),
                    ("n", "new"),
                    ("e", "export"),
                    ("s", "stop"),
                    ("r", "restart"),
                    ("d", "delete"),
                    ("!", "notify"),
                    ("i", "follow-up"),
                    ("q", "quit"),
                ]
            }
        }
        Overlay::NewSession(_) => {
            vec![
                ("Tab", "next field"),
                ("Enter", "create"),
                ("Esc", "cancel"),
            ]
        }
        Overlay::Confirm(_) => {
            vec![("y", "confirm"), ("n/Esc", "cancel")]
        }
        Overlay::Rename(_) => {
            vec![("Enter", "confirm"), ("Esc", "cancel")]
        }
        Overlay::Move(_) => {
            vec![("j/k", "navigate"), ("Enter", "move"), ("Esc", "cancel")]
        }
        Overlay::GroupManage(_) => {
            vec![("Enter", "create"), ("Esc", "cancel")]
        }
        Overlay::CommandPalette(_) => {
            vec![("Tab/arrows", "navigate"), ("Enter", "execute"), ("Esc", "close")]
        }
    };

    let len = hints.len();
    let spans: Vec<Span> = hints
        .iter()
        .enumerate()
        .flat_map(|(i, (key, action))| {
            let mut v = vec![
                Span::styled(
                    format!(" {} ", key),
                    Style::default().fg(theme.secondary).bold(),
                ),
                Span::styled(
                    format!("{} ", action),
                    Style::default().fg(theme.text_muted),
                ),
            ];
            if i < len - 1 {
                v.push(Span::styled(" ", Style::default().fg(theme.text_muted)));
            }
            v
        })
        .collect();

    let footer = Line::from(spans);
    frame.render_widget(Paragraph::new(footer), area);
}
