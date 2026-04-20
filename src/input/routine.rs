//! Input handling for routine-related overlays

use crate::app::{App, NewRoutineForm, Overlay, ScheduleFrequency};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Handle key input for the NewRoutine overlay form
pub fn handle_new_routine_key(
    app: &mut App,
    key: KeyEvent,
    storage: &crate::core::storage::Storage,
) {
    let form = match &mut app.overlay {
        Overlay::NewRoutine(f) => f,
        _ => return,
    };

    match (key.modifiers, key.code) {
        // Escape: cancel
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.overlay = Overlay::None;
        }
        // Tab: next field
        (KeyModifiers::NONE, KeyCode::Tab) => {
            form.focused_field = (form.focused_field + 1) % 8;
            form.completions.clear();
            form.completion_index = None;
        }
        // Shift+Tab: previous field
        (KeyModifiers::SHIFT, KeyCode::BackTab) => {
            form.focused_field = if form.focused_field == 0 {
                7
            } else {
                form.focused_field - 1
            };
            form.completions.clear();
            form.completion_index = None;
        }
        // Enter: submit form or add step
        (KeyModifiers::NONE, KeyCode::Enter) => {
            // If we're editing a step, add it
            if form.focused_field == 4 {
                if let Some(ref text) = form.editing_step.clone() {
                    if !text.is_empty() {
                        let step = if form.default_tool == "claude" {
                            crate::types::RoutineStep::Claude {
                                prompt: text.clone(),
                            }
                        } else {
                            crate::types::RoutineStep::Shell {
                                command: text.clone(),
                            }
                        };
                        form.steps.push(step);
                        form.editing_step = None;
                    }
                } else {
                    form.editing_step = Some(String::new());
                }
                return;
            }

            // Submit the form
            if form.name.is_empty() {
                return; // Don't submit with empty name
            }
            let cron = form.cron_expression();
            if cron.is_empty() {
                return;
            }

            let now = chrono::Utc::now().timestamp_millis();
            let next = crate::core::schedule::next_run(&cron);

            if let Some(ref edit_id) = form.edit_routine_id.clone() {
                // Editing existing routine
                let routine = crate::types::Routine {
                    id: edit_id.clone(),
                    name: form.name.clone(),
                    group_path: "my-routines".to_string(),
                    sort_order: 0,
                    working_dir: form.working_dir.clone(),
                    default_tool: form.default_tool.clone(),
                    schedule: cron,
                    steps: form.steps.clone(),
                    enabled: false,
                    created_at: now,
                    last_run_at: None,
                    next_run_at: next,
                    run_count: 0,
                    pinned: false,
                    notify: form.notify,
                    step_timeout_secs: form.step_timeout_secs,
                    expanded: false,
                };
                let _ = storage.save_routine(&routine);
            } else {
                // New routine
                let routine = crate::types::Routine {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: form.name.clone(),
                    group_path: "my-routines".to_string(),
                    sort_order: 0,
                    working_dir: form.working_dir.clone(),
                    default_tool: form.default_tool.clone(),
                    schedule: cron,
                    steps: form.steps.clone(),
                    enabled: false,
                    created_at: now,
                    last_run_at: None,
                    next_run_at: next,
                    run_count: 0,
                    pinned: false,
                    notify: form.notify,
                    step_timeout_secs: form.step_timeout_secs,
                    expanded: false,
                };
                let _ = storage.save_routine(&routine);
            }

            // Reload and close
            app.routines = storage.load_routines().unwrap_or_default();
            app.rebuild_routine_list_rows();
            app.overlay = Overlay::None;
            storage.touch().ok();
        }
        _ => {
            // Field-specific input
            match form.focused_field {
                0 => handle_text_input(&mut form.name, key), // Name
                1 => {
                    // Default tool toggle
                    if key.code == KeyCode::Left
                        || key.code == KeyCode::Right
                        || key.code == KeyCode::Char(' ')
                    {
                        form.default_tool = if form.default_tool == "claude" {
                            "shell".to_string()
                        } else {
                            "claude".to_string()
                        };
                    }
                }
                2 => {
                    // Working dir with autocomplete
                    handle_path_input(form, key);
                }
                3 => {
                    // Schedule frequency and params
                    handle_schedule_input(form, key);
                }
                4 => {
                    // Steps
                    handle_steps_input(form, key);
                }
                5 => {
                    // Notifications toggle
                    if key.code == KeyCode::Char(' ')
                        || key.code == KeyCode::Left
                        || key.code == KeyCode::Right
                    {
                        form.notify = !form.notify;
                    }
                }
                6 => {
                    // Step timeout
                    match key.code {
                        KeyCode::Left => {
                            form.step_timeout_secs = (form.step_timeout_secs - 300).max(60);
                        }
                        KeyCode::Right => {
                            form.step_timeout_secs = (form.step_timeout_secs + 300).min(7200);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
}

fn handle_text_input(text: &mut String, key: KeyEvent) {
    match key.code {
        KeyCode::Char(c) => text.push(c),
        KeyCode::Backspace => {
            text.pop();
        }
        _ => {}
    }
}

fn handle_path_input(form: &mut NewRoutineForm, key: KeyEvent) {
    match key.code {
        KeyCode::Char(c) => {
            form.working_dir.push(c);
            form.completions =
                crate::core::path_complete::complete_path(&form.working_dir).candidates;
            form.completion_index = None;
        }
        KeyCode::Backspace => {
            form.working_dir.pop();
            form.completions =
                crate::core::path_complete::complete_path(&form.working_dir).candidates;
            form.completion_index = None;
        }
        KeyCode::Down => {
            if !form.completions.is_empty() {
                form.completion_index = Some(
                    form.completion_index
                        .map(|i| (i + 1) % form.completions.len())
                        .unwrap_or(0),
                );
                if let Some(idx) = form.completion_index {
                    form.working_dir = form.completions[idx].clone();
                }
            }
        }
        KeyCode::Up => {
            if !form.completions.is_empty() {
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
        }
        _ => {}
    }
}

fn handle_schedule_input(form: &mut NewRoutineForm, key: KeyEvent) {
    match form.frequency {
        ScheduleFrequency::Advanced => match key.code {
            KeyCode::Char(c) => form.cron_raw.push(c),
            KeyCode::Backspace => {
                form.cron_raw.pop();
            }
            KeyCode::Left => form.frequency = form.frequency.prev(),
            KeyCode::Right => form.frequency = form.frequency.next(),
            _ => {}
        },
        _ => match key.code {
            KeyCode::Left => form.frequency = form.frequency.prev(),
            KeyCode::Right => form.frequency = form.frequency.next(),
            KeyCode::Up => {
                form.hour = if form.hour == 23 { 0 } else { form.hour + 1 };
            }
            KeyCode::Down => {
                form.hour = if form.hour == 0 { 23 } else { form.hour - 1 };
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                // Adjust minute
                let digit = c.to_digit(10).unwrap() as u8;
                form.minute = ((form.minute * 10 + digit) % 60).min(59);
            }
            KeyCode::Char(' ') => {
                // Toggle weekday (for weekly) — cycle through days 0-6
                if form.frequency == ScheduleFrequency::Weekly {
                    let idx = form.month_day as usize % 7;
                    form.weekdays[idx] = !form.weekdays[idx];
                    form.month_day = ((form.month_day as usize + 1) % 7) as u8;
                }
            }
            _ => {}
        },
    }
}

fn handle_steps_input(form: &mut NewRoutineForm, key: KeyEvent) {
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
            KeyCode::Char('d') => {
                if !form.steps.is_empty() {
                    form.steps.pop();
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{NewRoutineForm, ScheduleFrequency};

    #[test]
    fn test_schedule_frequency_next_cycles() {
        assert_eq!(ScheduleFrequency::Hourly.next(), ScheduleFrequency::Daily);
        assert_eq!(ScheduleFrequency::Daily.next(), ScheduleFrequency::Weekly);
        assert_eq!(ScheduleFrequency::Weekly.next(), ScheduleFrequency::Monthly);
        assert_eq!(ScheduleFrequency::Monthly.next(), ScheduleFrequency::Yearly);
        assert_eq!(
            ScheduleFrequency::Yearly.next(),
            ScheduleFrequency::Advanced
        );
        assert_eq!(
            ScheduleFrequency::Advanced.next(),
            ScheduleFrequency::Hourly
        );
    }

    #[test]
    fn test_schedule_frequency_prev_cycles() {
        assert_eq!(ScheduleFrequency::Daily.prev(), ScheduleFrequency::Hourly);
        assert_eq!(
            ScheduleFrequency::Hourly.prev(),
            ScheduleFrequency::Advanced
        );
        assert_eq!(
            ScheduleFrequency::Advanced.prev(),
            ScheduleFrequency::Yearly
        );
    }

    #[test]
    fn test_new_routine_form_defaults() {
        let form = NewRoutineForm::new();
        assert_eq!(form.default_tool, "claude");
        assert_eq!(form.frequency, ScheduleFrequency::Daily);
        assert_eq!(form.hour, 9);
        assert_eq!(form.minute, 0);
        assert!(form.notify);
        assert_eq!(form.step_timeout_secs, 1800);
        assert!(form.steps.is_empty());
        assert!(form.edit_routine_id.is_none());
    }

    #[test]
    fn test_cron_expression_daily() {
        let mut form = NewRoutineForm::new();
        form.frequency = ScheduleFrequency::Daily;
        form.hour = 9;
        form.minute = 0;
        let expr = form.cron_expression();
        assert!(!expr.is_empty());
        assert!(expr.contains("9"));
    }

    #[test]
    fn test_cron_expression_hourly() {
        let mut form = NewRoutineForm::new();
        form.frequency = ScheduleFrequency::Hourly;
        form.minute = 30;
        let expr = form.cron_expression();
        assert!(!expr.is_empty());
        assert!(expr.contains("30"));
    }

    #[test]
    fn test_cron_expression_advanced_returns_raw() {
        let mut form = NewRoutineForm::new();
        form.frequency = ScheduleFrequency::Advanced;
        form.cron_raw = "0 */6 * * *".to_string();
        assert_eq!(form.cron_expression(), "0 */6 * * *");
    }

    #[test]
    fn test_cron_expression_weekly_no_days_falls_back_to_daily() {
        let mut form = NewRoutineForm::new();
        form.frequency = ScheduleFrequency::Weekly;
        form.weekdays = [false; 7];
        let expr = form.cron_expression();
        assert!(!expr.is_empty());
    }
}
