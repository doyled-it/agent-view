use crossterm::event::{KeyCode, KeyEvent};

use crate::app::NewRoutineForm;

pub(super) fn handle_steps_input(form: &mut NewRoutineForm, key: KeyEvent) {
    if let Some(ref mut text) = form.editing_step {
        match key.code {
            KeyCode::Char(c) => text.push(c),
            KeyCode::Backspace => {
                text.pop();
            }
            KeyCode::Esc => form.editing_step = None,
            _ => {}
        }
    } else {
        match key.code {
            KeyCode::Char('a') => form.editing_step = Some(String::new()),
            KeyCode::Char('d') if !form.steps.is_empty() => {
                form.steps.pop();
            }
            _ => {}
        }
    }
}
