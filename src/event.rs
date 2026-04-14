//! Event types for the application event loop

use crossterm::event::KeyEvent;

#[allow(dead_code)]
pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    StatusRefresh,
}
