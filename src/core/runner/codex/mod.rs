//! Codex runner. Launches `codex`, captures session ids from notify
//! payloads (see `notify.rs`), and resumes via `codex resume <sid>`
//! gated on the on-disk rollout file (agent-deck issue #756).
//!
//! `parse_status` does light pane scraping to detect when the user has
//! typed but not yet sent input — this powers the Draft session status.
//! Codex renders empty-input suggestions with the SGR dim attribute
//! (`\e[2m`); real typed input is rendered with default formatting. We
//! capture the pane with `-e` (see `wants_ansi_escapes`) so the parser
//! can use this contrast as the primary signal. The older
//! template/literal allowlist (`{feature}`, `@filename`) is kept as a
//! defensive fallback for cases where ANSI capture fails.

pub mod cost_handler;
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

/// A numbered confirmation choice like `1. Yes, continue` or `1) No`.
/// When Codex shows a yes/no/confirm dialog (trust-directory, sandbox
/// escalation, apply-changes), the prompt-sigil line carries the first
/// option. Treat it as a question awaiting input, not draft input.
/// Requires whitespace after the separator so `1.5x speedup` (decimal)
/// doesn't false-positive.
static NUMBERED_CHOICE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\d+[.)]\s+\S").expect("static regex must compile"));

/// SGR "faint/dim" attribute. Codex wraps empty-input placeholders with
/// `\e[2m ... \e[0m`; real user input never carries this on the prompt line.
const SGR_DIM: &str = "\u{1b}[2m";

/// Default busy indicator. Codex prints `• Working (Xs • esc to interrupt)`
/// above the input area while a turn is in progress. This is the universal
/// signal that works on every Codex setup, but it disappears once the
/// model starts streaming response content (gone by ~frame 10 of a
/// 30-frame mid-turn capture).
const BUSY_INDICATOR_MARKER: &str = "esc to interrupt";

/// Footer state marker present on some setups via statusline customization.
/// When configured, the model/status line at the very bottom reads
/// `gpt-X default fast · ~ · Working · Context …` mid-turn (vs `· Ready ·`
/// when idle). This persists for the entire turn, so when present it
/// bridges the streaming gap that `BUSY_INDICATOR_MARKER` leaves uncovered.
/// Checked only on the last non-blank line — the substring could otherwise
/// appear in conversation prose ("the agent reported · Working · earlier").
///
/// Codex 0.128 fires notify only on turn-complete (no turn-started event
/// reaches the shim), so pane scraping is the sole Running signal during
/// a turn.
const BUSY_FOOTER_MARKER: &str = "\u{00b7} Working \u{00b7}";

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
        //
        // The poller captures with -e (see wants_ansi_escapes) so SGR codes
        // are preserved here. We strip them for content matching but inspect
        // the raw line for the dim attribute that marks placeholder hints.
        let mut status = ToolStatus::default();
        let raw_lines: Vec<&str> = pane_content.lines().collect();
        let cleaned_lines: Vec<String> = raw_lines
            .iter()
            .map(|l| crate::core::tmux::strip_ansi(l))
            .collect();

        let mut end = cleaned_lines.len();
        while end > 0 && cleaned_lines[end - 1].trim().is_empty() {
            end -= 1;
        }
        let scan_start = end.saturating_sub(30);

        // Busy check first. We deliberately don't set has_idle_prompt
        // because resolve_session_status gates Running behind
        // !has_idle_prompt, and Codex keeps the prompt sigil drawn even
        // mid-turn. Two markers with different scoping:
        //   - BUSY_INDICATOR_MARKER (`esc to interrupt`): default signal,
        //     scan the recent window because the `• Working` indicator can
        //     appear on any line above the prompt.
        //   - BUSY_FOOTER_MARKER (`· Working ·`): present only on setups
        //     with the statusline customization. Check just the last
        //     non-blank line so the substring doesn't false-positive on
        //     conversation prose.
        let indicator_busy =
            (scan_start..end).any(|i| cleaned_lines[i].contains(BUSY_INDICATOR_MARKER));
        let footer_busy = end > 0 && cleaned_lines[end - 1].contains(BUSY_FOOTER_MARKER);
        if indicator_busy || footer_busy {
            status.is_busy = true;
            return status;
        }

        let Some(rel_idx) = (scan_start..end)
            .rev()
            .find(|i| cleaned_lines[*i].trim_start().starts_with(PROMPT_SIGIL))
        else {
            return status;
        };

        status.has_idle_prompt = true;
        let prompt_line = cleaned_lines[rel_idx].trim_start();
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

        // Primary placeholder signal: the raw (ANSI-bearing) prompt line
        // contains `\e[2m`. Codex wraps every empty-input suggestion in dim,
        // including phrases that have no template marker ("Explain this
        // codebase", "Refactor X", etc.). Numbered-choice detection runs
        // first because trust-directory prompts may also use dim styling
        // and we want them surfaced as Paused, not silently swallowed.
        if NUMBERED_CHOICE_RE.is_match(body) {
            // Confirmation dialog awaiting a numbered choice — Paused, not Draft.
            // resolve_session_status maps has_question + has_idle_prompt → Paused.
            status.has_question = true;
            return status;
        }

        if raw_lines[rel_idx].contains(SGR_DIM) {
            return status;
        }

        // Fallback: known template/literal markers, for cases where the
        // pane was captured without ANSI (e.g. legacy callers, tests).
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

    fn wants_ansi_escapes(&self) -> bool {
        true
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
    fn test_parse_status_numbered_choice_dot_is_paused_not_draft() {
        // Codex trust-directory dialog and sandbox/apply prompts.
        let pane = "› 1. Yes, continue\n  2. No, quit\n  Press enter to continue\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_idle_prompt);
        assert!(s.has_question);
        assert!(
            !s.has_draft,
            "numbered confirmation must be Paused, not Draft"
        );
    }

    #[test]
    fn test_parse_status_numbered_choice_paren_is_paused() {
        let pane = "› 1) Yes\n  2) No\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_question);
        assert!(!s.has_draft);
    }

    #[test]
    fn test_parse_status_choice_other_than_first_is_paused() {
        // Whichever option happens to be highlighted/echoed on the sigil line.
        let pane = "› 2. No, quit\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_question);
        assert!(!s.has_draft);
    }

    #[test]
    fn test_parse_status_decimal_in_typed_text_is_draft() {
        // `1.5x` is a decimal, not a numbered choice — must remain Draft.
        let pane = "› we got a 1.5x speedup\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_draft);
        assert!(!s.has_question);
    }

    #[test]
    fn test_parse_status_digit_without_separator_is_draft() {
        // No `.` or `)` after the digit — just typed text.
        let pane = "› 1 yes please\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_draft);
        assert!(!s.has_question);
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
    fn test_parse_status_dim_placeholder_without_template_marker_is_not_draft() {
        // Real bug: Codex's "Explain this codebase" suggestion (no {token},
        // no @filename — just a complete sentence) was being read as Draft
        // because the literal/template allowlist didn't catch it. With ANSI
        // capture, the dim attribute is the authoritative signal.
        let pane = "\u{1b}[0;1m\u{203a}\u{1b}[0m \u{1b}[2mExplain this codebase\u{1b}[0m\n  gpt-5.5 default fast · ~ · Ready · Context 56% left\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_idle_prompt);
        assert!(
            !s.has_draft,
            "dim-wrapped placeholder must not register as draft"
        );
    }

    #[test]
    fn test_parse_status_real_typed_input_with_ansi_is_draft() {
        // User typed real input — no dim wrapper on the body.
        let pane =
            "\u{1b}[0;1m\u{203a}\u{1b}[0m fix the auth bug\n  gpt-5.5 default fast · ~ · Ready\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_idle_prompt);
        assert!(s.has_draft);
    }

    #[test]
    fn test_parse_status_ansi_numbered_choice_is_paused_not_swallowed() {
        // Trust-directory prompts may also be styled dim; numbered-choice
        // detection runs before the dim check so they still surface as Paused.
        let pane =
            "\u{1b}[0;1m\u{203a}\u{1b}[0m \u{1b}[2m1. Yes, continue\u{1b}[0m\n  2. No, quit\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_question);
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
    fn test_parse_status_indicator_esc_to_interrupt_marks_busy() {
        // Default Codex busy signal — the `• Working (Xs • esc to interrupt)`
        // line above the prompt. Works on every setup without customization.
        let pane = "› do it again\n\n• Working (1s • esc to interrupt)\n\n› Explain this codebase\n  gpt-5.5 default fast · ~ · Context 57% left\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.is_busy, "`esc to interrupt` indicator must set is_busy");
        assert!(
            !s.has_idle_prompt,
            "busy state must suppress has_idle_prompt so Tier 3 reaches the Running branch"
        );
    }

    #[test]
    fn test_parse_status_footer_working_marks_busy() {
        // Customized statusline footer — present when the user has the
        // optional Codex statusline override. Bridges the streaming gap
        // where the default indicator disappears.
        let pane =
            "› Explain this codebase\n\n  gpt-5.5 default fast · ~ · Working · Context 57% left\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.is_busy, "`· Working ·` footer must set is_busy");
        assert!(!s.has_idle_prompt);
    }

    #[test]
    fn test_parse_status_streaming_response_with_default_footer_is_idle() {
        // Mid-turn on a default Codex (no statusline customization): the
        // `• Working` indicator has vanished while the model streams
        // response content, and the footer doesn't carry a state word.
        // Documents a known gap — without the statusline customization,
        // long streaming responses briefly read as Idle. We accept this
        // because the alternative (state tracking) is a bigger lift.
        let pane = "  73. Crab\n  74. Lobster\n  75. Shrimp\n› Explain this codebase\n  gpt-5.5 default fast · ~ · Context 57% left\n";
        let s = CodexRunner.parse_status(pane);
        assert!(
            !s.is_busy,
            "documented limitation: streaming without footer customization reads as Idle"
        );
        assert!(s.has_idle_prompt);
    }

    #[test]
    fn test_parse_status_streaming_response_with_customized_footer_is_busy() {
        // Same mid-turn frame as above but with the statusline override
        // applied — footer keeps `· Working ·` for the full turn, so
        // streaming stays Running.
        let pane = "  73. Crab\n  74. Lobster\n  75. Shrimp\n› Explain this codebase\n  gpt-5.5 default fast · ~ · Working · Context 57% left\n";
        let s = CodexRunner.parse_status(pane);
        assert!(s.is_busy);
        assert!(!s.has_idle_prompt);
    }

    #[test]
    fn test_parse_status_footer_ready_is_not_busy() {
        let pane = "› \n\n  gpt-5.5 default fast · ~ · Ready · Context 56% left\n";
        let s = CodexRunner.parse_status(pane);
        assert!(!s.is_busy);
        assert!(s.has_idle_prompt);
    }

    #[test]
    fn test_parse_status_working_in_conversation_history_is_not_busy() {
        // The substring `· Working ·` could appear in conversation text but
        // only matters when it's on the footer (last non-blank) line. Idle
        // pane with prose containing `Working` must not flip to busy.
        let pane = "The agent reported · Working · earlier today.\n› \n  gpt-5.5 default fast · ~ · Ready · Context 56% left\n";
        let s = CodexRunner.parse_status(pane);
        assert!(!s.is_busy);
    }

    #[test]
    fn test_parse_status_against_real_captured_pane() {
        // Captured from a live Codex 0.128 session sitting idle on the
        // "Explain this codebase" suggestion — the case the homepage was
        // misreading as Draft. Asserts the dim-attribute fix catches it.
        let pane = include_str!("test_fixture_real_pane.txt");
        let s = CodexRunner.parse_status(pane);
        assert!(s.has_idle_prompt, "real fixture must reach the prompt line");
        assert!(
            !s.has_draft,
            "real fixture (dim placeholder) must not be Draft — got {:?}",
            s
        );
    }

    #[test]
    fn test_parse_status_against_real_running_pane() {
        // Captured mid-turn from a live Codex 0.128 session streaming a
        // response — the `• Working` indicator is gone but the footer
        // still shows `· Working ·`. This is the case the homepage was
        // missing entirely (no Running detection at all).
        let pane = include_str!("test_fixture_running_pane.txt");
        let s = CodexRunner.parse_status(pane);
        assert!(
            s.is_busy,
            "real running fixture must set is_busy — got {:?}",
            s
        );
        assert!(
            !s.has_idle_prompt,
            "busy must suppress has_idle_prompt so Running wins in resolve_session_status"
        );
    }

    #[test]
    fn test_wants_ansi_escapes_true() {
        // The parser uses SGR dim as the authoritative placeholder signal.
        assert!(CodexRunner.wants_ansi_escapes());
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
