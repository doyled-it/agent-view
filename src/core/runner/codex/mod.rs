//! Codex runner. Launches `codex`, captures session ids from notify
//! payloads (see `notify.rs`), and resumes via `codex resume <sid>`
//! gated on the on-disk rollout file (agent-deck issue #756).
//!
//! `parse_status` does light pane scraping to detect when the user has
//! typed but not yet sent input — this powers the Draft session status.
//! Codex shows a rotating placeholder (e.g. `Find and fix a bug in
//! @filename`, `Implement {feature}`) when the input is empty, which we
//! distinguish from real input by its template markers.

pub mod hooks;
pub mod notify;
pub mod notify_handler;

use super::{Runner, ToolStatus};
use regex::Regex;
use std::path::PathBuf;
use std::sync::LazyLock;

/// Codex's input prompt sigil — U+203A SINGLE RIGHT-POINTING ANGLE QUOTATION MARK.
const PROMPT_SIGIL: char = '\u{203a}';

/// Matches Codex placeholder template markers like `{feature}`, `{file}`,
/// `{description}` — a lowercase identifier in curly braces. Real user
/// input rarely contains this exact pattern.
static PLACEHOLDER_TEMPLATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\{[a-z_][a-z0-9_]*\}").expect("static regex must compile"));

/// Literal substrings that appear in Codex's rotating placeholders but are
/// extremely unlikely in real user input. `@filename` is the placeholder
/// for "supply a path here" — real file refs would have an extension
/// (`@foo.rs`), so we match only the bare literal.
const PLACEHOLDER_LITERALS: &[&str] = &["@filename", "@filepath", "@directory"];

pub struct CodexRunner;

impl Runner for CodexRunner {
    fn name(&self) -> &'static str {
        "codex"
    }

    fn launch_command(&self) -> Option<&'static str> {
        Some("codex")
    }

    fn parse_status(&self, pane_content: &str) -> ToolStatus {
        // Running/Waiting come from notify hooks; this only surfaces the
        // Draft overlay (and the idle-prompt flag that gates it). See
        // compose_status in runner/mod.rs for the precedence rules.
        //
        // Codex draws its TUI with absolute cursor positioning and pads the
        // bottom of the capture buffer with blank lines (unlike a scrolling
        // shell where the latest content is at the tail). Strip trailing
        // blanks before windowing or the prompt line falls outside the scan.
        let mut status = ToolStatus::default();
        let mut lines: Vec<&str> = pane_content.lines().collect();
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        let scan_start = lines.len().saturating_sub(30);
        let recent = &lines[scan_start..];

        let Some(rel_idx) = recent
            .iter()
            .rposition(|l| l.trim_start().starts_with(PROMPT_SIGIL))
        else {
            return status;
        };

        status.has_idle_prompt = true;
        let prompt_line = recent[rel_idx].trim_start();
        let after = prompt_line.strip_prefix(PROMPT_SIGIL).unwrap_or("");
        let body = after.trim();

        // Strip whitespace, NBSP, and the cursor block before checking for
        // meaningful content (mirrors claude/mod.rs:166).
        let meaningful: String = body
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '\u{00a0}' && *c != '\u{2588}')
            .collect();
        if meaningful.is_empty() {
            return status;
        }

        if is_codex_placeholder(body) {
            return status;
        }

        status.has_draft = true;
        status
    }

    fn extract_session_id(&self, _pane_content: &str) -> Option<String> {
        // Codex doesn't print its session id to the pane. Captured from
        // notify payloads by notify_handler.rs.
        None
    }

    fn restart_command(&self, original_command: &str, tool_data: &str) -> String {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(tool_data) {
            if let Some(sid) = data.get("codex_session_id").and_then(|v| v.as_str()) {
                if codex_rollout_exists(sid) {
                    return format!("codex resume {}", sid);
                }
                // Stale sid: fall through to fresh launch. The poller clears
                // the sid on the next hook tick.
            }
        }
        original_command.to_string()
    }

    fn install_hooks(&self) -> Result<(), String> {
        let dir = hooks::codex_config_dir().ok_or_else(|| "no home directory".to_string())?;
        let cmd = hooks::resolve_notify_command()?;
        hooks::install_hooks_in(&dir, &cmd)
    }

    fn tool_data_session_id_key(&self) -> &'static str {
        "codex_session_id"
    }
}

/// True if Codex has flushed a rollout JSONL for the given session id under
/// `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<ts>-<sid>.jsonl`. Used to gate
/// `codex resume <sid>` — without this check, a session that died before its
/// first rollout flush would loop forever (issue #756 in agent-deck).
pub(crate) fn codex_rollout_exists(sid: &str) -> bool {
    let sid = sid.trim();
    if sid.is_empty() {
        return false;
    }
    let codex_home = std::env::var("CODEX_HOME")
        .ok()
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|h| h.join(".codex")))
        .unwrap_or_default();
    let pattern = codex_home
        .join("sessions")
        .join("*")
        .join("*")
        .join("*")
        .join(format!("rollout-*-{}.jsonl", sid));
    glob::glob(pattern.to_str().unwrap_or(""))
        .map(|mut it| it.next().is_some())
        .unwrap_or(false)
}

/// True if `body` looks like one of Codex's rotating empty-input placeholders.
/// Matches the `{lowercase_token}` template form and a small set of bare
/// literal markers (`@filename` etc.) that Codex uses but real input rarely
/// contains verbatim.
fn is_codex_placeholder(body: &str) -> bool {
    if PLACEHOLDER_LITERALS.iter().any(|lit| body.contains(lit)) {
        return true;
    }
    PLACEHOLDER_TEMPLATE_RE.is_match(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_name_and_launch_command() {
        let r = CodexRunner;
        assert_eq!(r.name(), "codex");
        assert_eq!(r.launch_command(), Some("codex"));
    }

    #[test]
    fn test_parse_status_no_prompt_returns_default() {
        let pane = "Codex starting up...\nMCP startup incomplete (failed: gitlab)\n";
        let s = CodexRunner.parse_status(pane);
        assert!(!s.has_idle_prompt);
        assert!(!s.has_draft);
        assert!(!s.is_busy);
        assert!(!s.has_error);
    }

    #[test]
    fn test_parse_status_empty_prompt_is_idle_not_draft() {
        let pane = "› \n  gpt-5.5 default fast · ~\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_idle_prompt);
        assert!(!s.has_draft);
    }

    #[test]
    fn test_parse_status_just_sigil_no_space_is_idle_not_draft() {
        let pane = "›\n  gpt-5.5 default fast\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_idle_prompt);
        assert!(!s.has_draft);
    }

    #[test]
    fn test_parse_status_only_cursor_block_is_not_draft() {
        let pane = "› \u{2588}\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_idle_prompt);
        assert!(!s.has_draft);
    }

    #[test]
    fn test_parse_status_template_placeholder_is_not_draft() {
        // Real Codex 0.128 placeholder seen on fresh session.
        let pane = "› Implement {feature}\n  gpt-5.5 default fast · ~\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_idle_prompt);
        assert!(!s.has_draft);
    }

    #[test]
    fn test_parse_status_filename_placeholder_is_not_draft() {
        // Real Codex 0.128 placeholder seen on fresh session.
        let pane = "› Find and fix a bug in @filename\n  gpt-5.5 default fast · ~\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_idle_prompt);
        assert!(!s.has_draft);
    }

    #[test]
    fn test_parse_status_typed_text_is_draft() {
        let pane = "› fix the auth bug\n  gpt-5.5 default fast · ~\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_idle_prompt);
        assert!(s.has_draft);
    }

    #[test]
    fn test_parse_status_single_typed_char_is_draft() {
        let pane = "› x\n  gpt-5.5 default fast · ~\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_draft);
    }

    #[test]
    fn test_parse_status_user_file_ref_with_extension_is_draft() {
        // Real file refs have an extension; only the bare `@filename` literal
        // is a placeholder.
        let pane = "› fix the bug in @auth.rs\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_draft);
    }

    #[test]
    fn test_parse_status_draft_with_trailing_blank_padding() {
        // Codex's TUI pads the bottom of `tmux capture-pane` with blank
        // lines. The prompt line must still be found after the blanks are
        // stripped, or live Draft detection breaks under the poller's real
        // capture window (-S -100 → ~80+ trailing blanks for an 80-row pane).
        let mut pane = String::from(
            "╭───────────────╮\n\
             │ Codex header  │\n\
             ╰───────────────╯\n\
             › hello there\n\
               gpt-5.5 default fast · ~\n",
        );
        for _ in 0..120 {
            pane.push('\n');
        }
        let s = CodexRunner.parse_status(&pane);
        assert!(s.has_idle_prompt);
        assert!(
            s.has_draft,
            "trailing blank padding must not hide the prompt"
        );
    }

    #[test]
    fn test_parse_status_last_prompt_line_wins() {
        // Older typed-then-sent prompt above; current input is empty.
        let pane = "› old typed text\nresponse\n\n› \n  gpt-5.5 default fast\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_idle_prompt);
        assert!(!s.has_draft);
    }

    #[test]
    fn test_parse_status_typed_text_with_curly_braces_treated_as_placeholder() {
        // Known false-negative: real input containing a `{token}` pattern
        // is misclassified as a placeholder. Pin the current behavior so any
        // future refinement is intentional.
        let pane = "› {feature}\n";
        let s = CodexRunner.parse_status(pane);
        assert!(
            !s.has_draft,
            "documented limitation: literal {{token}} reads as placeholder"
        );
    }

    #[test]
    fn test_is_codex_placeholder_literals() {
        assert!(is_codex_placeholder("Find and fix a bug in @filename"));
        assert!(is_codex_placeholder("Open @filepath"));
        assert!(!is_codex_placeholder("fix @auth.rs"));
        assert!(!is_codex_placeholder("anything else"));
    }

    #[test]
    fn test_is_codex_placeholder_template_re() {
        assert!(is_codex_placeholder("Implement {feature}"));
        assert!(is_codex_placeholder("Refactor {old} into {new}"));
        assert!(
            !is_codex_placeholder("fix {THIS} bug"),
            "uppercase token is not a Codex placeholder"
        );
        assert!(!is_codex_placeholder("see }leftover{"), "unbalanced braces");
    }

    #[test]
    fn test_extract_session_id_returns_none() {
        assert_eq!(CodexRunner.extract_session_id("anything"), None);
    }

    #[test]
    fn test_is_implemented_returns_true() {
        assert!(CodexRunner.is_implemented());
    }

    #[test]
    fn test_tool_data_session_id_key() {
        assert_eq!(CodexRunner.tool_data_session_id_key(), "codex_session_id");
    }

    #[test]
    fn test_restart_command_falls_back_when_no_tool_data() {
        assert_eq!(CodexRunner.restart_command("codex", "{}"), "codex");
    }

    #[test]
    fn test_restart_command_falls_back_when_rollout_missing() {
        let _guard = crate::core::runner::hook_io::lock_env();
        let dir = TempDir::new().unwrap();
        std::env::set_var("CODEX_HOME", dir.path());
        let cmd = CodexRunner.restart_command("codex", r#"{"codex_session_id": "stale-uuid-xyz"}"#);
        std::env::remove_var("CODEX_HOME");
        assert_eq!(cmd, "codex");
    }

    #[test]
    fn test_restart_command_resumes_when_rollout_exists() {
        let _guard = crate::core::runner::hook_io::lock_env();
        let dir = TempDir::new().unwrap();
        let sid = "abc-123";
        let rollout_dir = dir
            .path()
            .join("sessions")
            .join("2026")
            .join("05")
            .join("10");
        fs::create_dir_all(&rollout_dir).unwrap();
        fs::write(
            rollout_dir.join(format!("rollout-1234567890-{}.jsonl", sid)),
            "",
        )
        .unwrap();

        std::env::set_var("CODEX_HOME", dir.path());
        let cmd =
            CodexRunner.restart_command("codex", &format!(r#"{{"codex_session_id": "{}"}}"#, sid));
        std::env::remove_var("CODEX_HOME");
        assert_eq!(cmd, format!("codex resume {}", sid));
    }
}
