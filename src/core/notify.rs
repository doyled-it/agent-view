//! Desktop notifications via terminal-notifier (macOS) or osascript fallback
//! Uses std::process::Command directly — no external crate needed.

use std::process::Command;
use std::sync::OnceLock;

static HAS_TERMINAL_NOTIFIER: OnceLock<bool> = OnceLock::new();

/// Check if terminal-notifier is available (cached)
fn check_terminal_notifier() -> bool {
    *HAS_TERMINAL_NOTIFIER.get_or_init(|| {
        Command::new("which")
            .arg("terminal-notifier")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

pub struct NotificationOptions {
    pub title: String,
    pub body: String,
    pub subtitle: Option<String>,
    pub sound: bool,
}

/// Build the notification command string for macOS.
/// Returns the command and args as a Vec for use with Command.
pub fn build_notification_command(options: &NotificationOptions) -> (String, Vec<String>) {
    let safe_title = options.title.replace('"', "\\\"");
    let safe_body = options.body.replace('"', "\\\"");

    if cfg!(target_os = "macos") {
        if check_terminal_notifier() {
            let mut args = vec![
                "-title".to_string(),
                safe_title,
                "-message".to_string(),
                safe_body,
                "-timeout".to_string(),
                "30".to_string(),
            ];
            if let Some(ref subtitle) = options.subtitle {
                args.push("-subtitle".to_string());
                args.push(subtitle.replace('"', "\\\""));
            }
            if options.sound {
                args.push("-sound".to_string());
                args.push("default".to_string());
            }
            ("terminal-notifier".to_string(), args)
        } else {
            let sound_clause = if options.sound {
                " sound name \"default\""
            } else {
                ""
            };
            let subtitle_clause = options
                .subtitle
                .as_ref()
                .map(|s| format!(" subtitle \"{}\"", s.replace('"', "\\\"")))
                .unwrap_or_default();

            let script = format!(
                "display notification \"{}\" with title \"{}\"{}{}",
                safe_body, safe_title, subtitle_clause, sound_clause
            );
            ("osascript".to_string(), vec!["-e".to_string(), script])
        }
    } else {
        // Linux: notify-send
        (
            "notify-send".to_string(),
            vec![
                "-u".to_string(),
                "critical".to_string(),
                safe_title,
                safe_body,
            ],
        )
    }
}

/// Build an osascript fallback command (used when terminal-notifier fails)
pub fn build_osascript_fallback(options: &NotificationOptions) -> (String, Vec<String>) {
    let safe_title = options.title.replace('"', "\\\"");
    let safe_body = options.body.replace('"', "\\\"");
    let sound_clause = if options.sound {
        " sound name \"default\""
    } else {
        ""
    };
    let subtitle_clause = options
        .subtitle
        .as_ref()
        .map(|s| format!(" subtitle \"{}\"", s.replace('"', "\\\"")))
        .unwrap_or_default();

    let script = format!(
        "display notification \"{}\" with title \"{}\"{}{}",
        safe_body, safe_title, subtitle_clause, sound_clause
    );
    ("osascript".to_string(), vec!["-e".to_string(), script])
}

/// Send a desktop notification (non-blocking, spawns subprocess)
pub fn send_notification(options: NotificationOptions) {
    let (cmd, args) = build_notification_command(&options);

    let result = Command::new(&cmd).args(&args).output();

    match result {
        Ok(output) if !output.status.success() => {
            // terminal-notifier failed, try osascript fallback on macOS
            if cfg!(target_os = "macos") && check_terminal_notifier() {
                let (fallback_cmd, fallback_args) = build_osascript_fallback(&options);
                let _ = Command::new(&fallback_cmd).args(&fallback_args).output();
            }
            if options.sound {
                // Bell fallback
                print!("\x07");
            }
        }
        Err(_) => {
            if options.sound {
                print!("\x07");
            }
        }
        _ => {}
    }

    // Linux sound fallback
    if cfg!(target_os = "linux") && options.sound {
        print!("\x07");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_osascript_fallback_basic() {
        let options = NotificationOptions {
            title: "Test Title".to_string(),
            body: "Test Body".to_string(),
            subtitle: None,
            sound: false,
        };
        let (cmd, args) = build_osascript_fallback(&options);
        assert_eq!(cmd, "osascript");
        assert_eq!(args.len(), 2);
        assert_eq!(args[0], "-e");
        assert!(args[1].contains("Test Title"));
        assert!(args[1].contains("Test Body"));
    }

    #[test]
    fn test_build_osascript_fallback_with_sound() {
        let options = NotificationOptions {
            title: "Title".to_string(),
            body: "Body".to_string(),
            subtitle: None,
            sound: true,
        };
        let (_, args) = build_osascript_fallback(&options);
        assert!(args[1].contains("sound name"));
    }

    #[test]
    fn test_build_osascript_fallback_with_subtitle() {
        let options = NotificationOptions {
            title: "Title".to_string(),
            body: "Body".to_string(),
            subtitle: Some("Sub".to_string()),
            sound: false,
        };
        let (_, args) = build_osascript_fallback(&options);
        assert!(args[1].contains("subtitle"));
        assert!(args[1].contains("Sub"));
    }

    #[test]
    fn test_build_osascript_escapes_quotes() {
        let options = NotificationOptions {
            title: "Title with \"quotes\"".to_string(),
            body: "Body with \"quotes\"".to_string(),
            subtitle: None,
            sound: false,
        };
        let (_, args) = build_osascript_fallback(&options);
        assert!(args[1].contains("\\\""));
    }

    #[test]
    fn test_notification_options_struct() {
        let options = NotificationOptions {
            title: "\u{1F7E1} BIS".to_string(),
            body: "Needs approval".to_string(),
            subtitle: None,
            sound: false,
        };
        assert_eq!(options.title, "\u{1F7E1} BIS");
        assert_eq!(options.body, "Needs approval");
    }
}
