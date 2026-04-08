//! Home screen rendering — stub (to be implemented in Task 10)

use crate::app::App;
use ratatui::prelude::*;
use ratatui::widgets::*;

/// Main render function for the home screen
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let text = if app.sessions.is_empty() {
        "No sessions. Press 'n' to create one.  Press 'q' to quit."
    } else {
        "agent-view — loading UI..."
    };
    frame.render_widget(
        Paragraph::new(text).alignment(Alignment::Center),
        area,
    );
}
