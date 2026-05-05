use crate::app::schedule_freq::ScheduleFrequency;

#[derive(Debug, Clone, PartialEq)]
pub struct NewRoutineForm {
    pub name: String,
    pub default_tool: String,
    pub working_dir: String,
    pub frequency: ScheduleFrequency,
    pub hour: u8,
    pub minute: u8,
    pub weekdays: [bool; 7], // Sun=0 through Sat=6
    pub month_day: u8,
    pub month: u8,
    pub cron_raw: String,
    pub steps: Vec<crate::types::RoutineStep>,
    pub editing_step: Option<String>, // text buffer when adding a step
    pub notify: bool,
    pub step_timeout_secs: i32,
    pub focused_field: usize,
    pub completions: Vec<String>,
    pub completion_index: Option<usize>,
    pub edit_routine_id: Option<String>, // Some(id) when editing existing routine
}

impl NewRoutineForm {
    pub fn new() -> Self {
        let home = dirs::home_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "/tmp".to_string());
        Self {
            name: String::new(),
            default_tool: "claude".to_string(),
            working_dir: home,
            frequency: ScheduleFrequency::Daily,
            hour: 9,
            minute: 0,
            weekdays: [false, true, true, true, true, true, false], // Mon-Fri
            month_day: 1,
            month: 1,
            cron_raw: String::new(),
            steps: Vec::new(),
            editing_step: None,
            notify: true,
            step_timeout_secs: 1800,
            focused_field: 0,
            completions: Vec::new(),
            completion_index: None,
            edit_routine_id: None,
        }
    }

    pub fn from_routine(routine: &crate::types::Routine) -> Self {
        let mut form = Self::new();
        form.name = routine.name.clone();
        form.default_tool = routine.default_tool.clone();
        form.working_dir = routine.working_dir.clone();
        form.steps = routine.steps.clone();
        form.notify = routine.notify;
        form.step_timeout_secs = routine.step_timeout_secs;
        form.edit_routine_id = Some(routine.id.clone());
        // Try to parse the cron expression back into frequency fields
        form.cron_raw = routine.schedule.clone();
        form.frequency = ScheduleFrequency::Advanced;
        form
    }

    pub fn cron_expression(&self) -> String {
        match self.frequency {
            ScheduleFrequency::Hourly => crate::core::schedule::build_hourly(self.minute),
            ScheduleFrequency::Daily => crate::core::schedule::build_daily(self.hour, self.minute),
            ScheduleFrequency::Weekly => {
                let days: Vec<crate::core::schedule::Weekday> = self
                    .weekdays
                    .iter()
                    .enumerate()
                    .filter(|(_, &selected)| selected)
                    .map(|(i, _)| crate::core::schedule::Weekday::all()[i])
                    .collect();
                if days.is_empty() {
                    crate::core::schedule::build_daily(self.hour, self.minute)
                } else {
                    crate::core::schedule::build_weekly(&days, self.hour, self.minute)
                }
            }
            ScheduleFrequency::Monthly => {
                crate::core::schedule::build_monthly_by_day(self.month_day, self.hour, self.minute)
            }
            ScheduleFrequency::Yearly => crate::core::schedule::build_yearly(
                self.month,
                self.month_day,
                self.hour,
                self.minute,
            ),
            ScheduleFrequency::Advanced => self.cron_raw.clone(),
        }
    }
}
