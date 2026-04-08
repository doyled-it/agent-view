//! Event types for the application event loop

use crossterm::event::KeyEvent;

pub enum AppEvent {
    Key(KeyEvent),
    Tick,
    StatusRefresh,
}
