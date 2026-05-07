use crossterm::event::{KeyCode, KeyEvent};

use crate::app::{NewRoutineForm, ScheduleFrequency};

pub(super) fn handle_schedule_input(form: &mut NewRoutineForm, key: KeyEvent) {
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
                if form.frequency == ScheduleFrequency::Hourly {
                    form.minute = if form.minute >= 59 {
                        0
                    } else {
                        form.minute + 1
                    };
                } else {
                    form.hour = if form.hour == 23 { 0 } else { form.hour + 1 };
                }
            }
            KeyCode::Down => {
                if form.frequency == ScheduleFrequency::Hourly {
                    form.minute = if form.minute == 0 {
                        59
                    } else {
                        form.minute - 1
                    };
                } else {
                    form.hour = if form.hour == 0 { 23 } else { form.hour - 1 };
                }
            }
            KeyCode::Char(c) if c.is_ascii_digit() => {
                let digit = c.to_digit(10).expect("matched is_ascii_digit guard") as u8;
                form.minute = ((form.minute * 10 + digit) % 60).min(59);
            }
            KeyCode::Char(' ') if form.frequency == ScheduleFrequency::Weekly => {
                let idx = form.month_day as usize % 7;
                form.weekdays[idx] = !form.weekdays[idx];
                form.month_day = ((form.month_day as usize + 1) % 7) as u8;
            }
            KeyCode::Char('+') => {
                // Increment month_day (for Monthly/Yearly)
                match form.frequency {
                    ScheduleFrequency::Monthly => {
                        form.month_day = if form.month_day >= 31 {
                            1
                        } else {
                            form.month_day + 1
                        };
                    }
                    ScheduleFrequency::Yearly => {
                        form.month = if form.month >= 12 { 1 } else { form.month + 1 };
                    }
                    _ => {}
                }
            }
            KeyCode::Char('-') => {
                // Decrement month_day (for Monthly/Yearly)
                match form.frequency {
                    ScheduleFrequency::Monthly => {
                        form.month_day = if form.month_day <= 1 {
                            31
                        } else {
                            form.month_day - 1
                        };
                    }
                    ScheduleFrequency::Yearly => {
                        form.month = if form.month <= 1 { 12 } else { form.month - 1 };
                    }
                    _ => {}
                }
            }
            KeyCode::Char(']') if form.frequency == ScheduleFrequency::Yearly => {
                form.month_day = if form.month_day >= 31 {
                    1
                } else {
                    form.month_day + 1
                };
            }
            KeyCode::Char('[') if form.frequency == ScheduleFrequency::Yearly => {
                form.month_day = if form.month_day <= 1 {
                    31
                } else {
                    form.month_day - 1
                };
            }
            _ => {}
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::app::ScheduleFrequency;

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
}
