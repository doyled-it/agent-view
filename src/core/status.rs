//! Claude Code status detection via regex pattern matching
//! Ports the exact patterns from the TypeScript tmux.ts

use lazy_static::lazy_static;
use regex::Regex;

/// Result of parsing tmux pane output for tool status
#[derive(Debug, Clone, Default)]
pub struct ToolStatus {
    pub is_active: bool,
    pub is_waiting: bool,
    pub is_compacting: bool,
    pub is_busy: bool,
    pub has_error: bool,
    pub has_exited: bool,
    pub has_idle_prompt: bool,
    pub has_question: bool,
}

/// Spinner characters used by Claude Code when processing
const SPINNER_CHARS: &[&str] = &[
    "\u{280b}", "\u{2819}", "\u{2839}", "\u{2838}", "\u{283c}", "\u{2834}", "\u{2826}", "\u{2827}",
    "\u{2807}", "\u{280f}", "\u{2733}", "\u{273d}", "\u{2736}", "\u{2722}",
];

lazy_static! {
    // Claude Code busy indicators — agent is actively working
    static ref CLAUDE_BUSY_PATTERNS: Vec<Regex> = vec![
        Regex::new(r"(?i)ctrl\+c to interrupt").unwrap(),
        Regex::new(r"(?i)esc to interrupt").unwrap(),
        Regex::new(r"(?i)\u{2026}.*tokens").unwrap(),
    ];

    // Claude Code waiting indicators — needs user input
    static ref CLAUDE_WAITING_PATTERNS: Vec<Regex> = vec![
        Regex::new(r"(?i)Do you want to proceed\?").unwrap(),
        Regex::new(r"(?i)\d\.\s*Yes\b").unwrap(),
        Regex::new(r"(?i)Esc to cancel.*Tab to amend").unwrap(),
        Regex::new(r"(?i)Enter to select.*to navigate").unwrap(),
        Regex::new(r"(?i)\(Y/n\)").unwrap(),
        Regex::new(r"(?i)Continue\?").unwrap(),
        Regex::new(r"(?i)Approve this plan\?").unwrap(),
        Regex::new(r"(?i)\[Y/n\]").unwrap(),
        Regex::new(r"(?i)\[y/N\]").unwrap(),
        Regex::new(r"(?i)Yes,? allow once").unwrap(),
        Regex::new(r"(?i)Allow always").unwrap(),
        Regex::new(r"(?i)No,? and tell Claude").unwrap(),
    ];

    // Claude exited patterns (shell returned)
    static ref CLAUDE_EXITED_PATTERNS: Vec<Regex> = vec![
        Regex::new(r"(?i)Resume this session with:").unwrap(),
        Regex::new(r"(?i)claude --resume").unwrap(),
        Regex::new(r"(?i)Press Ctrl-C again to exit").unwrap(),
    ];

    // Claude compacting patterns
    static ref CLAUDE_COMPACTING_PATTERNS: Vec<Regex> = vec![
        Regex::new(r"(?i)compacting conversation").unwrap(),
        Regex::new(r"(?i)summarizing conversation").unwrap(),
        Regex::new(r"(?i)context window.*(compact|compress)").unwrap(),
    ];

    // Error patterns
    static ref ERROR_PATTERNS: Vec<Regex> = vec![
        Regex::new(r"(?i)error:").unwrap(),
        Regex::new(r"(?i)failed:").unwrap(),
        Regex::new(r"(?i)exception:").unwrap(),
        Regex::new(r"(?i)traceback").unwrap(),
        Regex::new(r"(?i)panic:").unwrap(),
    ];

    // Idle prompt pattern
    static ref IDLE_PROMPT_RE: Regex = Regex::new(r"(?m)^\u{276f}\s").unwrap();

    // Question detection
    static ref QUESTION_RE: Regex = Regex::new(r"\?\s*$").unwrap();

    // Non-content line patterns (for question scanning)
    static ref SEPARATOR_RE: Regex = Regex::new(r"^[\u{2500}\u{2501}\u{2550}]{10,}").unwrap();
    static ref COMPANION_RE: Regex = Regex::new(r"Thistle").unwrap();
    static ref ART_LINE_RE: Regex = Regex::new(r"^\.\-\-\.$|^\\|^\\_|^~+$").unwrap();
    static ref SPINNER_LINE_RE: Regex = Regex::new(
        r"^[\u{273b}\u{273d}\u{2736}\u{2722}\u{280b}\u{2819}\u{2839}\u{2838}\u{283c}\u{2834}\u{2826}\u{2827}\u{2807}\u{280f}\u{00b7}]"
    ).unwrap();
    static ref USER_INPUT_RE: Regex = Regex::new(r"^\u{276f}").unwrap();
    static ref SHORTCUTS_RE: Regex = Regex::new(r"^\u{23f5}\u{23f5}|^\? for shortcuts").unwrap();
}

/// Check if text contains spinner characters
fn has_spinner(text: &str) -> bool {
    SPINNER_CHARS.iter().any(|c| text.contains(c))
}

/// Parse tmux pane output to detect Claude Code tool status.
/// The `tool` argument should be "claude" for Claude-specific detection.
pub fn parse_tool_status(output: &str, tool: Option<&str>) -> ToolStatus {
    let cleaned = crate::core::tmux::strip_ansi(output);

    // Filter out trailing empty lines
    let all_lines: Vec<&str> = cleaned.split('\n').collect();
    let mut last_non_empty = all_lines.len();
    while last_non_empty > 0 && all_lines[last_non_empty - 1].trim().is_empty() {
        last_non_empty -= 1;
    }
    let trimmed_lines: Vec<&str> = all_lines[..last_non_empty].to_vec();
    let last_30_start = if trimmed_lines.len() > 30 {
        trimmed_lines.len() - 30
    } else {
        0
    };
    let last_lines = trimmed_lines[last_30_start..].join("\n");
    let last_10_start = if trimmed_lines.len() > 10 {
        trimmed_lines.len() - 10
    } else {
        0
    };
    let last_few_lines = trimmed_lines[last_10_start..].join("\n");

    let mut status = ToolStatus::default();

    if tool == Some("claude") {
        // Check if Claude has exited
        status.has_exited = CLAUDE_EXITED_PATTERNS
            .iter()
            .any(|p| p.is_match(&last_lines));

        if !status.has_exited {
            // Compacting
            status.is_compacting = CLAUDE_COMPACTING_PATTERNS
                .iter()
                .any(|p| p.is_match(&last_lines));

            // Busy (actively working)
            status.is_busy = CLAUDE_BUSY_PATTERNS.iter().any(|p| p.is_match(&last_lines))
                || has_spinner(&last_few_lines);

            // Idle prompt detection — BEFORE waiting patterns
            if !status.is_busy && !status.is_compacting {
                status.has_idle_prompt = IDLE_PROMPT_RE.is_match(&last_few_lines);
            }

            // Waiting — only when there's NO idle prompt
            if !status.has_idle_prompt {
                status.is_waiting = CLAUDE_WAITING_PATTERNS
                    .iter()
                    .any(|p| p.is_match(&last_few_lines));
            }

            // Question detection when at idle prompt
            if status.has_idle_prompt && !status.is_busy && !status.is_compacting {
                // Find the prompt line index and scan lines above it
                if let Some(prompt_idx) = trimmed_lines
                    .iter()
                    .rposition(|l| IDLE_PROMPT_RE.is_match(l))
                {
                    let scan_start = if prompt_idx > 20 { prompt_idx - 20 } else { 0 };
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
    }

    // Error detection — only when not busy and not at idle prompt
    if !status.is_busy && !status.has_idle_prompt {
        status.has_error = ERROR_PATTERNS.iter().any(|p| p.is_match(&last_lines));
    }

    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_running_ctrl_c_to_interrupt() {
        let output = "Some output\nctrl+c to interrupt\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_busy);
        assert!(!status.is_waiting);
    }

    #[test]
    fn test_running_esc_to_interrupt() {
        let output = "Working...\nesc to interrupt\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_busy);
    }

    #[test]
    fn test_running_spinner_characters() {
        let output = "Processing \u{280b} loading...\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_busy);
    }

    #[test]
    fn test_running_tokens_indicator() {
        let output = "Processing\n\u{2026} 20.4k tokens\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_busy);
    }

    #[test]
    fn test_waiting_yn_prompt() {
        let output = "Do something? (Y/n)\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
        assert!(!status.is_busy);
    }

    #[test]
    fn test_waiting_proceed_prompt() {
        let output = "Do you want to proceed?\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_numbered_yes() {
        let output = "Choose an option:\n1. Yes\n2. No\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_allow_once() {
        let output = "Permission needed:\nYes, allow once\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_approve_plan() {
        let output = "Here's the plan:\nApprove this plan?\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_continue() {
        let output = "Continue?\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_esc_tab_footer() {
        let output = "Permission prompt\nEsc to cancel  Tab to amend\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
    }

    #[test]
    fn test_waiting_enter_to_select() {
        let output = "Select option:\nEnter to select, arrows to navigate\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_waiting);
    }

    #[test]
    fn test_idle_prompt_overrides_waiting_patterns() {
        // If the idle prompt is visible, waiting patterns should NOT match
        // because they'd be from historical conversational output
        let output = "Earlier output with (Y/n) text\n\u{276f} \n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_idle_prompt);
        assert!(!status.is_waiting);
    }

    #[test]
    fn test_idle_prompt_detected() {
        let output = "Claude finished.\n\u{276f} \n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_idle_prompt);
        assert!(!status.is_busy);
    }

    #[test]
    fn test_paused_question_at_prompt() {
        let output = "What file should I edit?\n\u{276f} \n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_idle_prompt);
        assert!(status.has_question);
    }

    #[test]
    fn test_no_question_when_no_question_mark() {
        let output = "I have completed the task.\n\u{276f} \n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_idle_prompt);
        assert!(!status.has_question);
    }

    #[test]
    fn test_exited_resume_session() {
        let output = "Session ended.\nResume this session with:\nclaude --resume abc123\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_exited);
        assert!(!status.is_busy);
        assert!(!status.is_waiting);
    }

    #[test]
    fn test_exited_claude_resume() {
        let output = "Done.\nclaude --resume session-id\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_exited);
    }

    #[test]
    fn test_exited_ctrl_c_exit() {
        let output = "Shutting down...\nPress Ctrl-C again to exit\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_exited);
    }

    #[test]
    fn test_compacting_conversation() {
        let output = "Context getting large...\ncompacting conversation\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_compacting);
        assert!(!status.is_busy);
    }

    #[test]
    fn test_compacting_summarizing() {
        let output = "summarizing conversation to save space\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_compacting);
    }

    #[test]
    fn test_error_not_detected_when_busy() {
        let output = "error: something failed\nctrl+c to interrupt\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.is_busy);
        assert!(!status.has_error);
    }

    #[test]
    fn test_error_not_detected_at_idle_prompt() {
        let output = "error: something failed earlier\n\u{276f} \n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_idle_prompt);
        assert!(!status.has_error);
    }

    #[test]
    fn test_error_detected_when_not_busy() {
        let output = "Running task...\nerror: compilation failed\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_error);
    }

    #[test]
    fn test_error_failed_pattern() {
        let output = "Trying something...\nfailed: connection refused\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_error);
    }

    #[test]
    fn test_error_traceback() {
        let output = "Running script...\nTraceback (most recent call last):\n  File...\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_error);
    }

    #[test]
    fn test_empty_output_is_not_busy() {
        let output = "\n\n\n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(!status.is_busy);
        assert!(!status.is_waiting);
        assert!(!status.has_error);
    }

    #[test]
    fn test_non_claude_tool_no_claude_patterns() {
        // Non-Claude tools should not trigger Claude-specific patterns
        let output = "ctrl+c to interrupt\n";
        let status = parse_tool_status(output, Some("shell"));
        assert!(!status.is_busy); // Claude busy patterns don't apply
    }

    #[test]
    fn test_question_several_lines_above_prompt() {
        let output =
            "Would you like me to proceed with this approach?\n\nSome blank lines\n\n\u{276f} \n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_idle_prompt);
        assert!(status.has_question);
    }

    #[test]
    fn test_separator_lines_skipped_in_question_scan() {
        let output = "Done with that.\n\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\u{276f} \n";
        let status = parse_tool_status(output, Some("claude"));
        assert!(status.has_idle_prompt);
        assert!(!status.has_question);
    }
}
