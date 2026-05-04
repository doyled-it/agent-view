use crossterm::event::{KeyCode, KeyEvent};

use crate::app::NewRoutineForm;
use crate::core::path_complete::complete_path;

pub(super) fn handle_text_input(text: &mut String, key: KeyEvent) {
    match key.code {
        KeyCode::Char(c) => text.push(c),
        KeyCode::Backspace => {
            text.pop();
        }
        _ => {}
    }
}

pub(super) fn handle_path_input(form: &mut NewRoutineForm, key: KeyEvent) {
    match key.code {
        KeyCode::Char(c) => {
            form.working_dir.push(c);
            form.completions = complete_path(&form.working_dir).candidates;
            form.completion_index = None;
        }
        KeyCode::Backspace => {
            form.working_dir.pop();
            form.completions = complete_path(&form.working_dir).candidates;
            form.completion_index = None;
        }
        KeyCode::Down if !form.completions.is_empty() => {
            form.completion_index = Some(
                form.completion_index
                    .map(|i| (i + 1) % form.completions.len())
                    .unwrap_or(0),
            );
            if let Some(idx) = form.completion_index {
                form.working_dir = form.completions[idx].clone();
            }
        }
        KeyCode::Up if !form.completions.is_empty() => {
            form.completion_index = Some(
                form.completion_index
                    .map(|i| {
                        if i == 0 {
                            form.completions.len() - 1
                        } else {
                            i - 1
                        }
                    })
                    .unwrap_or(0),
            );
            if let Some(idx) = form.completion_index {
                form.working_dir = form.completions[idx].clone();
            }
        }
        _ => {}
    }
}
