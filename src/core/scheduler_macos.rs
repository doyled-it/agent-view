//! macOS LaunchAgent scheduler implementation

use crate::core::scheduler::Scheduler;
use crate::types::Routine;
use std::path::PathBuf;

pub struct MacosScheduler {
    binary_path: String,
}

impl MacosScheduler {
    pub fn new() -> Self {
        let binary_path = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "agent-view".to_string());
        Self { binary_path }
    }

    #[cfg(test)]
    pub fn with_binary_path(binary_path: &str) -> Self {
        Self {
            binary_path: binary_path.to_string(),
        }
    }

    fn plist_path(routine_id: &str) -> PathBuf {
        let home = dirs::home_dir().expect("Cannot determine home directory");
        home.join("Library")
            .join("LaunchAgents")
            .join(format!("com.agent-view.routine.{}.plist", routine_id))
    }

    fn log_dir(routine_id: &str) -> PathBuf {
        let home = dirs::home_dir().expect("Cannot determine home directory");
        home.join(".agent-view")
            .join("routine-logs")
            .join(routine_id)
    }

    /// Generate plist XML content for a routine
    pub fn generate_plist(&self, routine: &Routine) -> String {
        let log_dir = Self::log_dir(&routine.id);
        let parts: Vec<&str> = routine.schedule.split_whitespace().collect();
        let mut calendar_entries = Vec::new();

        if parts.len() == 5 {
            let (minute, hour, dom, _month, dow) =
                (parts[0], parts[1], parts[2], parts[3], parts[4]);

            if let Some(hour_step) = hour.strip_prefix("*/") {
                if let Ok(n) = hour_step.parse::<u8>() {
                    let min_val: u8 = minute.parse().unwrap_or(0);
                    for h in (0..24u8).step_by(n as usize) {
                        calendar_entries.push(format!(
                            "    <dict>\n      <key>Hour</key>\n      <integer>{}</integer>\n      <key>Minute</key>\n      <integer>{}</integer>\n    </dict>",
                            h, min_val
                        ));
                    }
                }
            } else if dow != "*" {
                let min_val: u8 = minute.parse().unwrap_or(0);
                let hour_val: u8 = hour.parse().unwrap_or(9);
                for day in dow.split(',') {
                    if let Ok(d) = day.parse::<u8>() {
                        calendar_entries.push(format!(
                            "    <dict>\n      <key>Weekday</key>\n      <integer>{}</integer>\n      <key>Hour</key>\n      <integer>{}</integer>\n      <key>Minute</key>\n      <integer>{}</integer>\n    </dict>",
                            d, hour_val, min_val
                        ));
                    }
                }
            } else if dom != "*" {
                let min_val: u8 = minute.parse().unwrap_or(0);
                let hour_val: u8 = hour.parse().unwrap_or(9);
                let dom_val: u8 = dom.parse().unwrap_or(1);
                calendar_entries.push(format!(
                    "    <dict>\n      <key>Day</key>\n      <integer>{}</integer>\n      <key>Hour</key>\n      <integer>{}</integer>\n      <key>Minute</key>\n      <integer>{}</integer>\n    </dict>",
                    dom_val, hour_val, min_val
                ));
            } else {
                let min_val: u8 = minute.parse().unwrap_or(0);
                if hour == "*" {
                    calendar_entries.push(format!(
                        "    <dict>\n      <key>Minute</key>\n      <integer>{}</integer>\n    </dict>",
                        min_val
                    ));
                } else if let Ok(hour_val) = hour.parse::<u8>() {
                    calendar_entries.push(format!(
                        "    <dict>\n      <key>Hour</key>\n      <integer>{}</integer>\n      <key>Minute</key>\n      <integer>{}</integer>\n    </dict>",
                        hour_val, min_val
                    ));
                }
            }
        }

        let calendar_xml = if calendar_entries.len() == 1 {
            format!(
                "  <key>StartCalendarInterval</key>\n{}",
                calendar_entries[0]
            )
        } else {
            format!(
                "  <key>StartCalendarInterval</key>\n  <array>\n{}\n  </array>",
                calendar_entries.join("\n")
            )
        };

        // Capture current PATH so LaunchAgent can find tmux, claude, etc.
        let path_env = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".to_string());

        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.agent-view.routine.{id}</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>{path}</string>
  </dict>
  <key>ProgramArguments</key>
  <array>
    <string>{binary}</string>
    <string>exec-routine</string>
    <string>{id}</string>
  </array>
{calendar}
  <key>StandardOutPath</key>
  <string>{log_dir}/system.log</string>
  <key>StandardErrorPath</key>
  <string>{log_dir}/system.log</string>
</dict>
</plist>"#,
            id = routine.id,
            binary = self.binary_path,
            path = path_env,
            calendar = calendar_xml,
            log_dir = log_dir.to_string_lossy(),
        )
    }
}

impl Scheduler for MacosScheduler {
    fn install(&self, routine: &Routine) -> Result<(), String> {
        let plist_path = Self::plist_path(&routine.id);
        let log_dir = Self::log_dir(&routine.id);

        if let Some(parent) = plist_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create LaunchAgents dir: {}", e))?;
        }
        std::fs::create_dir_all(&log_dir)
            .map_err(|e| format!("Failed to create log dir: {}", e))?;

        if plist_path.exists() {
            let _ = std::process::Command::new("launchctl")
                .args(["unload", plist_path.to_str().unwrap_or_default()])
                .output();
        }

        let plist_content = self.generate_plist(routine);
        std::fs::write(&plist_path, &plist_content)
            .map_err(|e| format!("Failed to write plist: {}", e))?;

        let output = std::process::Command::new("launchctl")
            .args(["load", plist_path.to_str().unwrap_or_default()])
            .output()
            .map_err(|e| format!("Failed to run launchctl load: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("launchctl load failed: {}", stderr));
        }

        Ok(())
    }

    fn uninstall(&self, routine_id: &str) -> Result<(), String> {
        let plist_path = Self::plist_path(routine_id);
        if plist_path.exists() {
            let _ = std::process::Command::new("launchctl")
                .args(["unload", plist_path.to_str().unwrap_or_default()])
                .output();
            std::fs::remove_file(&plist_path)
                .map_err(|e| format!("Failed to remove plist: {}", e))?;
        }
        Ok(())
    }

    fn is_installed(&self, routine_id: &str) -> bool {
        Self::plist_path(routine_id).exists()
    }

    fn has_stale_binary_path(&self, routine_id: &str) -> bool {
        let plist_path = Self::plist_path(routine_id);
        if !plist_path.exists() {
            return false;
        }
        match std::fs::read_to_string(&plist_path) {
            Ok(content) => !content.contains(&self.binary_path),
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Routine, RoutineStep};

    fn make_test_routine(schedule: &str) -> Routine {
        Routine {
            id: "test-routine-123".to_string(),
            name: "Test Routine".to_string(),
            group_path: "my-routines".to_string(),
            sort_order: 0,
            working_dir: "/tmp/test".to_string(),
            default_tool: "claude".to_string(),
            schedule: schedule.to_string(),
            steps: vec![RoutineStep::Claude {
                prompt: "Do something".to_string(),
            }],
            enabled: true,
            created_at: 0,
            last_run_at: None,
            next_run_at: None,
            run_count: 0,
            pinned: false,
            notify: true,
            step_timeout_secs: 1800,
            expanded: false,
        }
    }

    #[test]
    fn test_plist_contains_routine_id() {
        let scheduler = MacosScheduler::with_binary_path("/usr/local/bin/agent-view");
        let routine = make_test_routine("0 9 * * *");
        let plist = scheduler.generate_plist(&routine);
        assert!(plist.contains("com.agent-view.routine.test-routine-123"));
        assert!(plist.contains("exec-routine"));
        assert!(plist.contains("test-routine-123"));
    }

    #[test]
    fn test_plist_contains_binary_path() {
        let scheduler = MacosScheduler::with_binary_path("/usr/local/bin/agent-view");
        let routine = make_test_routine("0 9 * * *");
        let plist = scheduler.generate_plist(&routine);
        assert!(plist.contains("/usr/local/bin/agent-view"));
    }

    #[test]
    fn test_plist_daily_schedule() {
        let scheduler = MacosScheduler::with_binary_path("/usr/local/bin/agent-view");
        let routine = make_test_routine("0 9 * * *");
        let plist = scheduler.generate_plist(&routine);
        assert!(plist.contains("<key>Hour</key>"));
        assert!(plist.contains("<integer>9</integer>"));
        assert!(plist.contains("<key>Minute</key>"));
        assert!(plist.contains("<integer>0</integer>"));
    }

    #[test]
    fn test_plist_weekly_schedule() {
        let scheduler = MacosScheduler::with_binary_path("/usr/local/bin/agent-view");
        let routine = make_test_routine("0 9 * * 1,3,5");
        let plist = scheduler.generate_plist(&routine);
        assert!(plist.contains("<key>Weekday</key>"));
        let dict_count = plist.matches("<key>Weekday</key>").count();
        assert_eq!(dict_count, 3);
    }

    #[test]
    fn test_plist_is_valid_xml() {
        let scheduler = MacosScheduler::with_binary_path("/usr/local/bin/agent-view");
        let routine = make_test_routine("0 9 * * *");
        let plist = scheduler.generate_plist(&routine);
        assert!(plist.starts_with("<?xml"));
        assert!(plist.contains("<!DOCTYPE plist"));
        assert!(plist.ends_with("</plist>"));
    }

    #[test]
    fn test_is_installed_returns_false_for_nonexistent() {
        let scheduler = MacosScheduler::with_binary_path("/usr/local/bin/agent-view");
        assert!(!scheduler.is_installed("nonexistent-routine-id"));
    }
}
