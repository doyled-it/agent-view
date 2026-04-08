//! Context-sensitive footer with keybind hints

use crate::app::{App, Overlay};
use ratatui::prelude::*;
use ratatui::widgets::*;

pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let hints: Vec<(&str, &str)> = match &app.overlay {
        Overlay::None => {
            if app.sessions.is_empty() {
                vec![("n", "new"), ("q", "quit")]
            } else {
                vec![
                    ("j/k", "navigate"),
                    ("Enter", "attach"),
                    ("n", "new"),
                    ("s", "stop"),
                    ("r", "restart"),
                    ("d", "delete"),
                    ("!", "notify"),
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
    };

    let len = hints.len();
    let spans: Vec<Span> = hints
        .iter()
        .enumerate()
        .flat_map(|(i, (key, action))| {
            let mut v = vec![
                Span::styled(
                    format!(" {} ", key),
                    Style::default().fg(Color::Cyan).bold(),
                ),
                Span::styled(
                    format!("{} ", action),
                    Style::default().fg(Color::DarkGray),
                ),
            ];
            if i < len - 1 {
                v.push(Span::styled(" ", Style::default().fg(Color::DarkGray)));
            }
            v
        })
        .collect();

    let footer = Line::from(spans);
    frame.render_widget(Paragraph::new(footer), area);
}
