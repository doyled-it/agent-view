//! OpenCode runner. Launches `opencode`; hooks are the primary status
//! source, while pane scraping provides a conservative fallback for prompt,
//! permission, busy, and exit markers.

pub mod hook_handler;
pub mod hooks;

use super::{Runner, ToolStatus};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

static NUMBERED_CHOICE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\d+[.)]\s+\S").expect("static regex must compile"));

static SESSION_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:--session|-s|session\s+id)\s*[:=]?\s+([A-Za-z0-9_.:-]+)")
        .expect("static regex must compile")
});

static SAFE_SESSION_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.:-]+$").expect("static regex must compile"));

const PROMPT_SIGIL: char = '>';
const FRAME_BORDER: char = '┃';
const FRAME_BOTTOM_LEFT: char = '╹';
const BUSY_MARKERS: &[&str] = &["esc to interrupt", "working ("];
const EXIT_MARKERS: &[&str] = &["opencode session ended", "opencode exited"];
const PLACEHOLDER_PREFIXES: &[&str] = &[
    "type a message",
    "enter a prompt",
    "what can i help",
    "ask anything",
];

pub struct OpencodeRunner;

impl Runner for OpencodeRunner {
    fn name(&self) -> &'static str {
        "opencode"
    }

    fn launch_command(&self) -> Option<&'static str> {
        Some("opencode")
    }

    fn parse_status(&self, pane_content: &str) -> ToolStatus {
        let mut status = ToolStatus::default();
        let cleaned_lines: Vec<String> = pane_content
            .lines()
            .map(crate::core::tmux::strip_ansi)
            .collect();

        let mut end = cleaned_lines.len();
        while end > 0 && cleaned_lines[end - 1].trim().is_empty() {
            end -= 1;
        }
        let scan_start = end.saturating_sub(40);

        let recent_lower: Vec<String> = cleaned_lines[scan_start..end]
            .iter()
            .map(|line| line.to_ascii_lowercase())
            .collect();

        if recent_lower
            .iter()
            .any(|line| BUSY_MARKERS.iter().any(|marker| line.contains(marker)))
        {
            status.is_busy = true;
            return status;
        }

        if recent_lower
            .iter()
            .any(|line| EXIT_MARKERS.iter().any(|marker| line.contains(marker)))
        {
            status.has_exited = true;
            return status;
        }

        let permission_visible = recent_lower.iter().any(|line| line.contains("permission"))
            && recent_lower
                .iter()
                .any(|line| line.contains("allow") || line.contains("deny"));
        if permission_visible {
            status.has_idle_prompt = true;
            status.has_question = true;
            return status;
        }

        if let Some(frame_status) = parse_frame_prompt(&cleaned_lines, scan_start, end) {
            return frame_status;
        }

        let Some(prompt_idx) = (scan_start..end)
            .rev()
            .find(|i| cleaned_lines[*i].trim_start().starts_with(PROMPT_SIGIL))
        else {
            return status;
        };

        status.has_idle_prompt = true;
        let prompt_line = cleaned_lines[prompt_idx].trim_start();
        let body = prompt_line.strip_prefix(PROMPT_SIGIL).unwrap_or("").trim();

        if body.is_empty() {
            return status;
        }

        if NUMBERED_CHOICE_RE.is_match(body) {
            status.has_question = true;
            return status;
        }

        let body_lower = body.to_ascii_lowercase();
        if PLACEHOLDER_PREFIXES
            .iter()
            .any(|prefix| body_lower.starts_with(prefix))
        {
            return status;
        }

        status.has_draft = true;
        status
    }

    fn extract_session_id(&self, pane_content: &str) -> Option<String> {
        let cleaned = crate::core::tmux::strip_ansi(pane_content);
        SESSION_ID_RE
            .captures(&cleaned)
            .and_then(|caps| caps.get(1).map(|m| m.as_str().trim().to_string()))
            .filter(|sid| is_safe_session_id(sid))
    }

    fn restart_command(&self, original_command: &str, _tool_data: &str) -> String {
        if let Ok(data) = serde_json::from_str::<Value>(_tool_data) {
            if let Some(sid) = data
                .get("opencode_session_id")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|sid| is_safe_session_id(sid))
            {
                return format!("opencode --session {}", sid);
            }
        }
        original_command.to_string()
    }

    fn tool_data_session_id_key(&self) -> &'static str {
        "opencode_session_id"
    }

    fn install_hooks(&self) -> Result<(), String> {
        let dir = hooks::opencode_config_dir().ok_or_else(|| "no home directory".to_string())?;
        let cmd = hooks::resolve_hook_command()?;
        hooks::install_hooks_in(&dir, &cmd)
    }
}

fn is_safe_session_id(sid: &str) -> bool {
    !sid.is_empty() && !sid.contains("..") && SAFE_SESSION_ID_RE.is_match(sid)
}

fn parse_frame_prompt(lines: &[String], scan_start: usize, end: usize) -> Option<ToolStatus> {
    let bottom_idx = (scan_start..end)
        .rev()
        .find(|i| lines[*i].contains(FRAME_BOTTOM_LEFT))?;

    let mut status = ToolStatus {
        has_idle_prompt: true,
        ..Default::default()
    };
    let body_start = bottom_idx.saturating_sub(8).max(scan_start);
    for line in lines[body_start..bottom_idx].iter().rev() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with(FRAME_BORDER) {
            continue;
        }
        let body = trimmed.strip_prefix(FRAME_BORDER).unwrap_or("").trim();
        if body.is_empty() || is_frame_metadata(body) {
            continue;
        }

        let body_lower = body.to_ascii_lowercase();
        if PLACEHOLDER_PREFIXES
            .iter()
            .any(|prefix| body_lower.starts_with(prefix))
        {
            return Some(status);
        }

        status.has_draft = true;
        return Some(status);
    }

    Some(status)
}

fn is_frame_metadata(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    lower.starts_with("build ")
        || lower.starts_with("plan ")
        || lower.starts_with("edit ")
        || lower.starts_with("debug ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_and_launch_command() {
        let r = OpencodeRunner;
        assert_eq!(r.name(), "opencode");
        assert_eq!(r.launch_command(), Some("opencode"));
    }

    #[test]
    fn test_parse_status_idle_prompt_from_fixture() {
        let s = OpencodeRunner.parse_status(include_str!("test_fixture_idle_pane.txt"));
        assert!(s.has_idle_prompt);
        assert!(!s.has_draft);
        assert!(!s.is_busy);
        assert!(!s.has_question);
    }

    #[test]
    fn test_parse_status_busy_from_fixture() {
        let s = OpencodeRunner.parse_status(include_str!("test_fixture_busy_pane.txt"));
        assert!(s.is_busy);
        assert!(
            !s.has_idle_prompt,
            "busy status must suppress idle prompt so Running wins"
        );
    }

    #[test]
    fn test_parse_status_waiting_from_fixture() {
        let s = OpencodeRunner.parse_status(include_str!("test_fixture_waiting_pane.txt"));
        assert!(s.has_idle_prompt);
        assert!(s.has_question);
        assert!(!s.has_draft);
    }

    #[test]
    fn test_parse_status_exited_from_fixture() {
        let s = OpencodeRunner.parse_status(include_str!("test_fixture_exited_pane.txt"));
        assert!(s.has_exited);
        assert!(!s.has_draft);
    }

    #[test]
    fn test_parse_status_typed_input_is_draft() {
        let s = OpencodeRunner.parse_status("> fix the status parser\n");
        assert!(s.has_idle_prompt);
        assert!(s.has_draft);
    }

    #[test]
    fn test_parse_status_framed_typed_input_is_draft() {
        let pane = include_str!("test_fixture_idle_pane.txt").replace(
            "Ask anything... \"What is the tech stack of this project?\"",
            "fix the status parser",
        );
        let s = OpencodeRunner.parse_status(&pane);
        assert!(s.has_idle_prompt);
        assert!(s.has_draft);
    }

    #[test]
    fn test_extract_session_id_from_resume_hint() {
        assert_eq!(
            OpencodeRunner.extract_session_id(include_str!("test_fixture_exited_pane.txt")),
            Some("ses_01HV7Y5P7RE4Q3H8M9K0N2ABCD".to_string())
        );
    }

    #[test]
    fn test_restart_command_uses_session_id_from_tool_data() {
        assert_eq!(
            OpencodeRunner.restart_command(
                "opencode",
                r#"{"opencode_session_id":"ses_01HV7Y5P7RE4Q3H8M9K0N2ABCD"}"#
            ),
            "opencode --session ses_01HV7Y5P7RE4Q3H8M9K0N2ABCD"
        );
    }

    #[test]
    fn test_restart_command_falls_back_when_no_session_id() {
        assert_eq!(
            OpencodeRunner.restart_command("opencode --model provider/model", "{}"),
            "opencode --model provider/model"
        );
    }

    #[test]
    fn test_tool_data_session_id_key() {
        assert_eq!(
            OpencodeRunner.tool_data_session_id_key(),
            "opencode_session_id"
        );
    }

    #[test]
    fn test_wants_ansi_escapes_default_false() {
        assert!(!OpencodeRunner.wants_ansi_escapes());
    }
}
