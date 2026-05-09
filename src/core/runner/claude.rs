//! Claude Code runner. Detection logic ported from the former
//! `src/core/status.rs` (TypeScript tmux.ts patterns).

use super::{Runner, ToolStatus};
use regex::Regex;
use std::sync::LazyLock;

pub struct ClaudeRunner;

const SPINNER_CHARS: &[&str] = &[
    "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}",
    "\u{2807}", "\u{280f}", "\u{2733}", "\u{273d}", "\u{2736}", "\u{2722}",
];

static FOOTER_BUSY_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)ctrl\+c to interrupt").expect("static regex must compile"),
        Regex::new(r"(?i)esc to interrupt").expect("static regex must compile"),
        Regex::new(r"(?i)\u{2026}.*tokens").expect("static regex must compile"),
    ]
});

static WAITING_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)Do you want to proceed\?").expect("static regex must compile"),
        Regex::new(r"(?i)\d\.\s*Yes\b").expect("static regex must compile"),
        Regex::new(r"(?i)Esc to cancel.*Tab to amend").expect("static regex must compile"),
        Regex::new(r"(?i)Enter to select.*to navigate").expect("static regex must compile"),
        Regex::new(r"(?i)\(Y/n\)").expect("static regex must compile"),
        Regex::new(r"(?i)Continue\?").expect("static regex must compile"),
        Regex::new(r"(?i)Approve this plan\?").expect("static regex must compile"),
        Regex::new(r"(?i)\[Y/n\]").expect("static regex must compile"),
        Regex::new(r"(?i)\[y/N\]").expect("static regex must compile"),
        Regex::new(r"(?i)Yes,? allow once").expect("static regex must compile"),
        Regex::new(r"(?i)Allow always").expect("static regex must compile"),
        Regex::new(r"(?i)No,? and tell Claude").expect("static regex must compile"),
    ]
});

static EXITED_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)Resume this session with:").expect("static regex must compile"),
        Regex::new(r"(?i)claude --resume").expect("static regex must compile"),
        Regex::new(r"(?i)Press Ctrl-C again to exit").expect("static regex must compile"),
    ]
});

static COMPACTING_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)compacting conversation").expect("static regex must compile"),
        Regex::new(r"(?i)summarizing conversation").expect("static regex must compile"),
        Regex::new(r"(?i)context window.*(compact|compress)").expect("static regex must compile"),
    ]
});

static ERROR_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)error:").expect("static regex must compile"),
        Regex::new(r"(?i)failed:").expect("static regex must compile"),
        Regex::new(r"(?i)exception:").expect("static regex must compile"),
        Regex::new(r"(?i)traceback").expect("static regex must compile"),
        Regex::new(r"(?i)panic:").expect("static regex must compile"),
    ]
});

static IDLE_PROMPT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\u{276f}").expect("static regex must compile"));
static CLAUDE_SESSION_ID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"claude\s+--resume\s+([\w-]+)").expect("static regex must compile")
});
static QUESTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\?\s*$").expect("static regex must compile"));
static MONITOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\u{00b7}\s*\d+\s+monitors?\s*\u{00b7}").expect("static regex must compile")
});
static SEPARATOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[\u{2500}\u{2501}\u{2550}]{10,}").expect("static regex must compile")
});
static COMPANION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"Thistle").expect("static regex must compile"));
static ART_LINE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\.\-\-\.$|^\\|^\\_|^~+$").expect("static regex must compile"));
static SPINNER_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^[\u{273b}\u{273d}\u{2736}\u{2722}\u{280b}\u{2819}\u{2839}\u{2838}\u{283c}\u{2834}\u{2826}\u{2827}\u{2807}\u{280f}\u{00b7}]",
    )
    .expect("static regex must compile")
});
static USER_INPUT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\u{276f}").expect("static regex must compile"));
static SHORTCUTS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\u{23f5}\u{23f5}|^\? for shortcuts").expect("static regex must compile")
});

fn has_spinner(text: &str) -> bool {
    SPINNER_CHARS.iter().any(|c| text.contains(c))
}

fn extract_footer(trimmed_lines: &[&str]) -> String {
    if let Some(idx) = trimmed_lines
        .iter()
        .rposition(|l| SEPARATOR_RE.is_match(l.trim_start()))
    {
        trimmed_lines[idx + 1..].join("\n")
    } else {
        let start = trimmed_lines.len().saturating_sub(2);
        trimmed_lines[start..].join("\n")
    }
}

impl Runner for ClaudeRunner {
    fn name(&self) -> &'static str {
        "claude"
    }
    fn launch_command(&self) -> &'static str {
        "claude"
    }

    fn parse_status(&self, pane_content: &str) -> ToolStatus {
        let cleaned = crate::core::tmux::strip_ansi(pane_content);

        let all_lines: Vec<&str> = cleaned.split('\n').collect();
        let mut last_non_empty = all_lines.len();
        while last_non_empty > 0 && all_lines[last_non_empty - 1].trim().is_empty() {
            last_non_empty -= 1;
        }
        let trimmed_lines: Vec<&str> = all_lines[..last_non_empty].to_vec();
        let last_30_start = trimmed_lines.len().saturating_sub(30);
        let last_lines = trimmed_lines[last_30_start..].join("\n");
        let last_10_start = trimmed_lines.len().saturating_sub(10);
        let last_few_lines = trimmed_lines[last_10_start..].join("\n");
        let footer = extract_footer(&trimmed_lines);

        let has_exited = EXITED_PATTERNS.iter().any(|p| p.is_match(&last_lines));
        let mut status = ToolStatus {
            has_exited,
            ..ToolStatus::default()
        };

        if !status.has_exited {
            status.is_monitoring = MONITOR_RE.is_match(&footer);

            status.is_compacting = COMPACTING_PATTERNS.iter().any(|p| p.is_match(&last_lines));

            status.is_busy = FOOTER_BUSY_PATTERNS.iter().any(|p| p.is_match(&footer))
                || has_spinner(&last_few_lines);

            if !status.is_busy && !status.is_compacting {
                let recent_lines = &trimmed_lines[last_10_start..];
                if let Some(rel_idx) = recent_lines.iter().rposition(|l| l.starts_with('\u{276f}'))
                {
                    let prompt_line = recent_lines[rel_idx];
                    let after_prompt = prompt_line.strip_prefix('\u{276f}').unwrap_or("");
                    let meaningful: String = after_prompt
                        .chars()
                        .filter(|c| !c.is_whitespace() && *c != '\u{00a0}' && *c != '\u{2588}')
                        .collect();
                    status.has_idle_prompt = true;
                    status.has_draft = !meaningful.is_empty();
                }
            }

            if !status.has_idle_prompt {
                status.is_waiting = WAITING_PATTERNS.iter().any(|p| p.is_match(&last_few_lines));
            }

            if status.has_idle_prompt && !status.is_busy && !status.is_compacting {
                if let Some(prompt_idx) = trimmed_lines
                    .iter()
                    .rposition(|l| IDLE_PROMPT_RE.is_match(l))
                {
                    let scan_start = prompt_idx.saturating_sub(20);
                    let lines_above = &trimmed_lines[scan_start..prompt_idx];
                    let mut content_checked = 0;
                    for line in lines_above.iter().rev() {
                        if content_checked >= 8 {
                            break;
                        }
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if SEPARATOR_RE.is_match(trimmed) || COMPANION_RE.is_match(trimmed) {
                            continue;
                        }
                        if ART_LINE_RE.is_match(trimmed) {
                            continue;
                        }
                        if SPINNER_LINE_RE.is_match(trimmed) {
                            continue;
                        }
                        if USER_INPUT_RE.is_match(trimmed) {
                            continue;
                        }
                        if SHORTCUTS_RE.is_match(trimmed) {
                            continue;
                        }
                        content_checked += 1;
                        if QUESTION_RE.is_match(trimmed) {
                            status.has_question = true;
                            break;
                        }
                    }
                }
            }
        }

        if !status.is_busy && !status.has_idle_prompt {
            status.has_error = ERROR_PATTERNS.iter().any(|p| p.is_match(&last_lines));
        }

        status
    }

    fn extract_session_id(&self, pane_content: &str) -> Option<String> {
        let cleaned = crate::core::tmux::strip_ansi(pane_content);
        CLAUDE_SESSION_ID_RE
            .captures(&cleaned)
            .and_then(|caps| caps.get(1))
            .map(|m| m.as_str().to_string())
    }

    fn restart_command(&self, _original_command: &str, tool_data: &str) -> String {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(tool_data) {
            if let Some(session_id) = data.get("claude_session_id").and_then(|v| v.as_str()) {
                return format!("claude --resume {}", session_id);
            }
        }
        "claude --continue".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> ToolStatus {
        ClaudeRunner.parse_status(s)
    }

    #[test]
    fn test_running_ctrl_c_to_interrupt() {
        let status = parse("Some output\nctrl+c to interrupt\n");
        assert!(status.is_busy);
        assert!(!status.is_waiting);
    }

    #[test]
    fn test_running_esc_to_interrupt() {
        let status = parse("Working...\nesc to interrupt\n");
        assert!(status.is_busy);
    }

    #[test]
    fn test_running_spinner_characters() {
        let status = parse("Processing \u{280b} loading...\n");
        assert!(status.is_busy);
    }

    #[test]
    fn test_running_tokens_indicator() {
        let status = parse("Processing\n\u{2026} 20.4k tokens\n");
        assert!(status.is_busy);
    }

    #[test]
    fn test_waiting_yn_prompt() {
        let status = parse("Do something? (Y/n)\n");
        assert!(status.is_waiting);
        assert!(!status.is_busy);
    }

    #[test]
    fn test_waiting_proceed_prompt() {
        let status = parse("Do you want to proceed?\n");
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_numbered_yes() {
        let status = parse("Choose an option:\n1. Yes\n2. No\n");
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_allow_once() {
        let status = parse("Permission needed:\nYes, allow once\n");
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_approve_plan() {
        let status = parse("Here's the plan:\nApprove this plan?\n");
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_continue() {
        let status = parse("Continue?\n");
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_esc_tab_footer() {
        let status = parse("Permission prompt\nEsc to cancel  Tab to amend\n");
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_enter_to_select() {
        let status = parse("Select option:\nEnter to select, arrows to navigate\n");
        assert!(status.is_waiting);
    }

    #[test]
    fn test_idle_prompt_overrides_waiting_patterns() {
        let status = parse("Earlier output with (Y/n) text\n\u{276f} \n");
        assert!(status.has_idle_prompt);
        assert!(!status.is_waiting);
    }

    #[test]
    fn test_idle_prompt_detected() {
        let status = parse("Claude finished.\n\u{276f} \n");
        assert!(status.has_idle_prompt);
        assert!(!status.is_busy);
    }

    #[test]
    fn test_paused_question_at_prompt() {
        let status = parse("What file should I edit?\n\u{276f} \n");
        assert!(status.has_idle_prompt);
        assert!(status.has_question);
    }

    #[test]
    fn test_no_question_when_no_question_mark() {
        let status = parse("I have completed the task.\n\u{276f} \n");
        assert!(status.has_idle_prompt);
        assert!(!status.has_question);
    }

    #[test]
    fn test_exited_resume_session() {
        let status = parse("Session ended.\nResume this session with:\nclaude --resume abc123\n");
        assert!(status.has_exited);
        assert!(!status.is_busy);
        assert!(!status.is_waiting);
    }

    #[test]
    fn test_exited_claude_resume() {
        let status = parse("Done.\nclaude --resume session-id\n");
        assert!(status.has_exited);
    }

    #[test]
    fn test_exited_ctrl_c_exit() {
        let status = parse("Shutting down...\nPress Ctrl-C again to exit\n");
        assert!(status.has_exited);
    }

    #[test]
    fn test_compacting_conversation() {
        let status = parse("Context getting large...\ncompacting conversation\n");
        assert!(status.is_compacting);
        assert!(!status.is_busy);
    }

    #[test]
    fn test_compacting_summarizing() {
        let status = parse("summarizing conversation to save space\n");
        assert!(status.is_compacting);
    }

    #[test]
    fn test_error_not_detected_when_busy() {
        let status = parse("error: something failed\nctrl+c to interrupt\n");
        assert!(status.is_busy);
        assert!(!status.has_error);
    }

    #[test]
    fn test_error_not_detected_at_idle_prompt() {
        let status = parse("error: something failed earlier\n\u{276f} \n");
        assert!(status.has_idle_prompt);
        assert!(!status.has_error);
    }

    #[test]
    fn test_error_detected_when_not_busy() {
        let status = parse("Running task...\nerror: compilation failed\n");
        assert!(status.has_error);
    }

    #[test]
    fn test_error_failed_pattern() {
        let status = parse("Trying something...\nfailed: connection refused\n");
        assert!(status.has_error);
    }

    #[test]
    fn test_error_traceback() {
        let status = parse("Running script...\nTraceback (most recent call last):\n  File...\n");
        assert!(status.has_error);
    }

    #[test]
    fn test_empty_output_is_not_busy() {
        let status = parse("\n\n\n");
        assert!(!status.is_busy);
        assert!(!status.is_waiting);
        assert!(!status.has_error);
    }

    #[test]
    fn test_question_several_lines_above_prompt() {
        let status = parse(
            "Would you like me to proceed with this approach?\n\nSome blank lines\n\n\u{276f} \n",
        );
        assert!(status.has_idle_prompt);
        assert!(status.has_question);
    }

    #[test]
    fn test_separator_lines_skipped_in_question_scan() {
        let status = parse("Done with that.\n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} \n");
        assert!(status.has_idle_prompt);
        assert!(!status.has_question);
    }

    #[test]
    fn test_draft_detected_when_text_after_prompt() {
        let status = parse("Claude finished.\n\u{276f} fix the bug in\n");
        assert!(status.has_draft);
        assert!(status.has_idle_prompt);
    }

    #[test]
    fn test_draft_not_detected_at_empty_prompt() {
        let status = parse("Claude finished.\n\u{276f} \n");
        assert!(!status.has_draft);
        assert!(status.has_idle_prompt);
    }

    #[test]
    fn test_draft_not_detected_at_cursor_only_prompt() {
        let status = parse("Claude finished.\n\u{276f} \u{2588}\n");
        assert!(!status.has_draft);
        assert!(status.has_idle_prompt);
    }

    #[test]
    fn test_draft_not_detected_at_nbsp_prompt() {
        let status = parse("Claude finished.\n\u{276f}\u{00a0}\n");
        assert!(!status.has_draft);
        assert!(status.has_idle_prompt);
    }

    #[test]
    fn test_draft_overrides_question() {
        let status = parse("What file should I edit?\n\u{276f} src/main\n");
        assert!(status.has_draft);
        assert!(status.has_idle_prompt);
    }

    #[test]
    fn test_draft_not_detected_when_busy() {
        let status = parse("\u{276f} some text\nctrl+c to interrupt\n");
        assert!(status.is_busy);
        assert!(!status.has_draft);
    }

    #[test]
    fn test_monitoring_singular_in_footer() {
        let status = parse("\u{276f}\u{00a0}\n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n  \u{23f5}\u{23f5} accept edits on \u{00b7} 1 monitor \u{00b7} \u{2193} to manage\n");
        assert!(status.is_monitoring);
        assert!(status.has_idle_prompt);
    }

    #[test]
    fn test_monitoring_plural_in_footer() {
        let status = parse("\u{276f}\u{00a0}\n  \u{23f5}\u{23f5} accept edits on \u{00b7} 3 monitors \u{00b7} \u{2193} to manage\n");
        assert!(status.is_monitoring);
    }

    #[test]
    fn test_monitoring_detected_alongside_busy() {
        let status =
            parse("Working on it...\n\u{00b7} 1 monitor \u{00b7} esc to interrupt \u{00b7}\n");
        assert!(status.is_monitoring);
        assert!(status.is_busy);
    }

    #[test]
    fn test_busy_not_detected_when_phrase_quoted_above_footer() {
        let separator = "\u{2500}".repeat(20);
        let output = format!(
            "Behavior note: assumes the idle footer omits esc to interrupt.\n\
             {separator}\n\
             \u{276f}\u{00a0}\n\
             {separator}\n\
               \u{23f5}\u{23f5} accept edits on (shift+tab to cycle)\n"
        );
        let status = parse(&output);
        assert!(!status.is_busy, "footer has no busy indicator");
        assert!(status.has_idle_prompt);
    }

    #[test]
    fn test_monitoring_not_detected_when_phrase_quoted_above_footer() {
        let separator = "\u{2500}".repeat(20);
        let output = format!(
            "Summary: a session with \u{00b7} 1 monitor \u{00b7} in its footer would resolve to Monitoring.\n\
             {separator}\n\
             \u{276f}\u{00a0}\n\
             {separator}\n\
               \u{23f5}\u{23f5} accept edits on (shift+tab to cycle)\n"
        );
        let status = parse(&output);
        assert!(!status.is_monitoring);
    }

    #[test]
    fn test_busy_detected_when_in_actual_footer() {
        let separator = "\u{2500}".repeat(20);
        let output = format!(
            "Calling tool...\n\
             {separator}\n\
             \u{276f}\u{00a0}\n\
             {separator}\n\
             \u{273d} Working\u{2026} (esc to interrupt \u{00b7} ctrl+t to show todos)\n"
        );
        let status = parse(&output);
        assert!(status.is_busy);
    }

    #[test]
    fn test_monitoring_not_detected_in_prose() {
        let status = parse("I have 2 monitors on my desk.\n\u{276f}\u{00a0}\n");
        assert!(!status.is_monitoring);
    }

    #[test]
    fn test_monitoring_not_detected_in_normal_idle() {
        let status = parse("Claude finished.\n\u{276f}\u{00a0}\n");
        assert!(!status.is_monitoring);
        assert!(status.has_idle_prompt);
    }

    #[test]
    fn test_monitoring_not_detected_after_exit() {
        let status = parse(
            "Resume this session with:\nclaude --resume abc123\n  \u{00b7} 1 monitor \u{00b7}\n",
        );
        assert!(status.has_exited);
        assert!(!status.is_monitoring);
    }

    #[test]
    fn test_extract_session_id() {
        let id = ClaudeRunner.extract_session_id(
            "Some output\nResume this session with: claude --resume abc123-def456\nMore output",
        );
        assert_eq!(id, Some("abc123-def456".to_string()));
    }

    #[test]
    fn test_extract_session_id_no_match() {
        let id = ClaudeRunner.extract_session_id("Normal claude output with no resume line");
        assert_eq!(id, None);
    }

    #[test]
    fn test_extract_session_id_from_exited_output() {
        let id = ClaudeRunner.extract_session_id("Task completed.\n\n  Resume this session with:\n    claude --resume 7a3f2b1e-4c5d-6e7f-8a9b-0c1d2e3f4a5b\n\n");
        assert_eq!(id, Some("7a3f2b1e-4c5d-6e7f-8a9b-0c1d2e3f4a5b".to_string()));
    }

    #[test]
    fn test_restart_command_with_session_id() {
        let cmd = ClaudeRunner.restart_command("claude", r#"{"claude_session_id": "abc123"}"#);
        assert_eq!(cmd, "claude --resume abc123");
    }

    #[test]
    fn test_restart_command_without_session_id() {
        let cmd = ClaudeRunner.restart_command("claude", "{}");
        assert_eq!(cmd, "claude --continue");
    }
}
