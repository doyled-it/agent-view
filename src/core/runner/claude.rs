//! Claude Code runner. Detection logic ported from the former
//! `src/core/status.rs` (TypeScript tmux.ts patterns).

#![allow(dead_code)]

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
    // Filled in by Task 3.
}
