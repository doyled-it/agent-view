use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Overlay};
use crate::core::schedule::next_run;
use crate::core::scheduler::platform_scheduler;
use crate::core::storage::Storage;
use crate::types::{Routine, RoutineStep};

use super::new_schedule::handle_schedule_input;
use super::new_steps::handle_steps_input;
use super::new_text::{handle_path_input, handle_text_input};

/// Handle key input for the NewRoutine overlay form
pub fn handle_new_routine_key(app: &mut App, key: KeyEvent, storage: &Storage) {
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
            form.focused_field = (form.focused_field + 1) % 7;
            form.completions.clear();
            form.completion_index = None;
        }
        // Shift+Tab: previous field
        (KeyModifiers::SHIFT, KeyCode::BackTab) => {
            form.focused_field = if form.focused_field == 0 {
                6
            } else {
                form.focused_field - 1
            };
            form.completions.clear();
            form.completion_index = None;
        }
        // Enter: confirm step edit if active, otherwise submit form
        (KeyModifiers::NONE, KeyCode::Enter) => {
            // If actively editing a step, confirm it
            if let Some(ref text) = form.editing_step.clone() {
                if !text.is_empty() {
                    let step = if form.default_tool == "claude" {
                        RoutineStep::Claude {
                            prompt: text.clone(),
                        }
                    } else {
                        RoutineStep::Shell {
                            command: text.clone(),
                        }
                    };
                    form.steps.push(step);
                }
                form.editing_step = None;
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
            let next = next_run(&cron);

            if let Some(ref edit_id) = form.edit_routine_id.clone() {
                // Editing existing routine — preserve fields not in the form
                if let Ok(Some(mut existing)) = storage.get_routine(edit_id) {
                    existing.name = form.name.clone();
                    existing.working_dir = form.working_dir.clone();
                    existing.default_tool = form.default_tool.clone();
                    existing.schedule = cron;
                    existing.steps = form.steps.clone();
                    existing.notify = form.notify;
                    existing.step_timeout_secs = form.step_timeout_secs;
                    existing.next_run_at = next;
                    let _ = storage.save_routine(&existing);
                }
            } else {
                // New routine
                let routine = Routine {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: form.name.clone(),
                    group_path: "my-routines".to_string(),
                    sort_order: 0,
                    working_dir: form.working_dir.clone(),
                    default_tool: form.default_tool.clone(),
                    schedule: cron,
                    steps: form.steps.clone(),
                    enabled: true,
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

                // Install system job immediately
                let scheduler = platform_scheduler();
                let _ = scheduler.install(&routine);
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
                1 if key.code == KeyCode::Left
                    || key.code == KeyCode::Right
                    || key.code == KeyCode::Char(' ') =>
                {
                    form.default_tool = if form.default_tool == "claude" {
                        "shell".to_string()
                    } else {
                        "claude".to_string()
                    };
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
                5 if key.code == KeyCode::Char(' ')
                    || key.code == KeyCode::Left
                    || key.code == KeyCode::Right =>
                {
                    form.notify = !form.notify;
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

#[cfg(test)]
mod tests {
    use crate::app::NewRoutineForm;

    #[test]
    fn test_new_routine_form_defaults() {
        use crate::app::ScheduleFrequency;
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
        use crate::app::ScheduleFrequency;
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
        use crate::app::ScheduleFrequency;
        let mut form = NewRoutineForm::new();
        form.frequency = ScheduleFrequency::Hourly;
        form.minute = 30;
        let expr = form.cron_expression();
        assert!(!expr.is_empty());
        assert!(expr.contains("30"));
    }

    #[test]
    fn test_cron_expression_advanced_returns_raw() {
        use crate::app::ScheduleFrequency;
        let mut form = NewRoutineForm::new();
        form.frequency = ScheduleFrequency::Advanced;
        form.cron_raw = "0 */6 * * *".to_string();
        assert_eq!(form.cron_expression(), "0 */6 * * *");
    }

    #[test]
    fn test_cron_expression_weekly_no_days_falls_back_to_daily() {
        use crate::app::ScheduleFrequency;
        let mut form = NewRoutineForm::new();
        form.frequency = ScheduleFrequency::Weekly;
        form.weekdays = [false; 7];
        let expr = form.cron_expression();
        assert!(!expr.is_empty());
    }
}
