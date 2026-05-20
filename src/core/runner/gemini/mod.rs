//! Gemini CLI runner. Launches `gemini`, captures status via two
//! complementary signals:
//!
//! - **Hooks** (primary): `SessionStart` / `BeforeAgent` / `AfterAgent` /
//!   `SessionEnd` events installed into `~/.gemini/settings.json` produce
//!   `BeforeAgent` → Running, `AfterAgent` → Idle transitions
//!   authoritatively.
//! - **Pane scraping** (secondary, this module): detects Draft (typed
//!   input not yet sent), Paused (tool-confirmation dialog open), Idle
//!   (empty prompt frame), and Running (mid-turn loading indicator) from
//!   tmux pane content.
//!
//! Pane markers below are derived from the Gemini CLI source
//! (`google-gemini/gemini-cli`) — see the regex/literal constants for
//! exact upstream references. Synthesized fixtures rather than live
//! captures, so a follow-up commit on real captured panes is welcome.
//!
//! Gemini CLI 0.9 has no resume flag (no `--resume`, no `--chat <id>`,
//! no `--continue`), so `restart_command` always returns the original
//! command. The session id captured from hooks is preserved in
//! `tool_data` for analytics linking even though it can't be used for
//! relaunch.

pub mod cost_handler;
pub mod hook_handler;
pub mod hooks;

use super::{Runner, ToolStatus};

/// `esc to cancel,` — printed by `LoadingIndicator.tsx` whenever Gemini is
/// `StreamingState.Responding` (mid-turn). The comma is intentional: the
/// formatting is `(esc to cancel, 5s)`. Including the comma anchors the
/// match against conversation prose that might otherwise contain the
/// phrase "esc to cancel".
const BUSY_INDICATOR_MARKER: &str = "esc to cancel,";

/// `Thinking...` — `LoadingIndicator.tsx`'s default loading phrase when
/// no custom `currentLoadingPhrase` / `thought` is set. Secondary busy
/// signal; the primary is `BUSY_INDICATOR_MARKER` because it's stickier.
const BUSY_PHRASE_THINKING: &str = "Thinking...";

/// `●` (U+25CF) — `BaseSelectionList.tsx`'s `selectedIndicator`. Marks
/// the currently-selected option in a `RadioButtonSelect`, which is what
/// `ToolConfirmationMessage` renders when Gemini needs the user to
/// approve a tool call (Allow once / Allow for this session / etc.).
const RADIO_SELECTED_GLYPH: char = '\u{25cf}';

/// `╭` and `╰` — Ink's rounded-border top-left and bottom-left glyphs.
/// `InputPrompt.tsx` wraps the input area in a `borderStyle="round"`
/// box, so the bottom corner is the anchor for "where the input
/// frame ends". We scan upward from there for the body line.
const FRAME_BOTTOM_LEFT: char = '\u{2570}';

/// The default placeholder text rendered inside the input box when the
/// buffer is empty — see `InputPrompt.tsx` (`placeholder = '  Type your
/// message or @path/to/file'`). When present in a body line, the input
/// is empty (Idle); any other content represents typed input (Draft).
/// Matching on the leading literal makes the check robust against the
/// voice-mode variant ("  Type your message or hold space to talk
/// (Esc to exit)") which begins with the same prefix.
const PLACEHOLDER_PREFIX: &str = "Type your message or";

pub struct GeminiRunner;

impl Runner for GeminiRunner {
    fn name(&self) -> &'static str {
        "gemini"
    }

    fn launch_command(&self) -> Option<&'static str> {
        Some("gemini")
    }

    fn parse_status(&self, pane_content: &str) -> ToolStatus {
        // Gemini's TUI uses absolute cursor positioning (like Codex), so
        // tmux capture-pane pads the bottom with blank lines. Strip
        // trailing blanks before windowing.
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

        let mut status = ToolStatus::default();

        // Tier 1: busy check wins outright. The compose_status pipeline
        // gates Running on !has_idle_prompt, so we deliberately leave
        // has_idle_prompt unset here even though the input frame may
        // still be drawn mid-turn (Gemini keeps the box on screen with
        // a non-interactive border color).
        let busy = (scan_start..end).any(|i| {
            let line = cleaned_lines[i].as_str();
            line.contains(BUSY_INDICATOR_MARKER) || line.contains(BUSY_PHRASE_THINKING)
        });
        if busy {
            status.is_busy = true;
            return status;
        }

        // Tier 2: tool-confirmation dialog. The `●` glyph at the start of
        // a non-blank line is `BaseSelectionList`'s selectedIndicator.
        // Treat as Paused (has_idle_prompt + has_question) so the UI
        // surfaces it instead of reading as plain Idle.
        let confirm_visible = (scan_start..end).any(|i| {
            cleaned_lines[i]
                .trim_start()
                .starts_with(RADIO_SELECTED_GLYPH)
        });
        if confirm_visible {
            status.has_idle_prompt = true;
            status.has_question = true;
            return status;
        }

        // Tier 3: locate the input frame's bottom-left corner. We search
        // the last non-blank lines for `╰`. If we don't find one, the
        // pane probably hasn't reached an interactive state (early
        // startup / banner) — return default.
        let Some(bottom_idx) = (scan_start..end)
            .rev()
            .find(|i| cleaned_lines[*i].contains(FRAME_BOTTOM_LEFT))
        else {
            return status;
        };

        status.has_idle_prompt = true;

        // The line directly above the bottom-left corner carries the
        // input body (placeholder text when empty, typed input when not).
        // `Ink` renders the input as a single body row by default; if
        // the user has typed multiple lines it grows upward. We only
        // need to know whether body is empty / placeholder / typed.
        let body_idx = bottom_idx.checked_sub(1);
        let Some(body_idx) = body_idx else {
            // Frame is malformed (no line above bottom-left) — surface
            // as plain Idle.
            return status;
        };

        let body_line = cleaned_lines[body_idx].trim();
        // Strip leading + trailing vertical-border glyphs so we look at
        // the body text directly. Both ends are present when Ink draws a
        // full-frame; just the leading one is present when the body
        // overflows the frame width.
        let body_inner = body_line
            .trim_start_matches('\u{2502}')
            .trim_end_matches('\u{2502}')
            .trim();

        // Strip cursor + zero-width glyphs the same way the Codex parser
        // does (mirrors codex/mod.rs::parse_status: NBSP, full-block
        // cursor).
        let meaningful: String = body_inner
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '\u{00a0}' && *c != '\u{2588}')
            .collect();

        if meaningful.is_empty() {
            // Empty input — Idle (placeholder is rendered but cursor is
            // at column 0 with nothing typed).
            return status;
        }

        if body_inner.starts_with(PLACEHOLDER_PREFIX) || body_inner.contains(PLACEHOLDER_PREFIX) {
            // The placeholder occupies the body — still Idle. (The
            // `contains` check covers cases where a leading cursor /
            // inverse-styled character is between `│` and the
            // placeholder.)
            return status;
        }

        // Anything else inside the input frame is typed-but-unsent
        // input → Draft.
        status.has_draft = true;
        status
    }

    fn extract_session_id(&self, _pane_content: &str) -> Option<String> {
        // Gemini doesn't print the session id to the pane. Captured from
        // the hook payload's `session_id` field by hook_handler.rs.
        None
    }

    fn restart_command(&self, original_command: &str, _tool_data: &str) -> String {
        // Gemini CLI 0.9 has no resume flag. Re-launch fresh.
        original_command.to_string()
    }

    fn install_hooks(&self) -> Result<(), String> {
        let dir = hooks::gemini_config_dir().ok_or_else(|| "no home directory".to_string())?;
        let cmd = hooks::resolve_hook_command()?;
        hooks::install_hooks_in(&dir, &cmd)
    }

    fn tool_data_session_id_key(&self) -> &'static str {
        "gemini_session_id"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic Gemini input frame around a body line. Mirrors
    /// the Ink rounded-border layout described in `InputPrompt.tsx`.
    fn frame(body: &str) -> String {
        format!(
            "╭─────────────────────────────────────╮\n\
             │ {} │\n\
             ╰─────────────────────────────────────╯\n",
            body
        )
    }

    #[test]
    fn test_name_and_launch_command() {
        let r = GeminiRunner;
        assert_eq!(r.name(), "gemini");
        assert_eq!(r.launch_command(), Some("gemini"));
    }

    #[test]
    fn test_parse_status_no_frame_returns_default() {
        let pane = "Gemini starting up...\nLoaded 3 extensions\n";
        let s = GeminiRunner.parse_status(pane);
        assert!(!s.has_idle_prompt);
        assert!(!s.has_draft);
        assert!(!s.is_busy);
    }

    #[test]
    fn test_parse_status_empty_frame_is_idle() {
        let pane = frame("");
        let s = GeminiRunner.parse_status(&pane);
        assert!(s.has_idle_prompt);
        assert!(!s.has_draft);
        assert!(!s.has_question);
        assert!(!s.is_busy);
    }

    #[test]
    fn test_parse_status_placeholder_is_idle_not_draft() {
        let pane = frame("  Type your message or @path/to/file");
        let s = GeminiRunner.parse_status(&pane);
        assert!(s.has_idle_prompt);
        assert!(!s.has_draft, "placeholder must not register as draft");
    }

    #[test]
    fn test_parse_status_voice_mode_placeholder_is_idle_not_draft() {
        // InputPrompt.tsx voice-mode variant: same prefix, different
        // suffix. Should still read as Idle.
        let pane = frame("  Type your message or hold space to talk (Esc to exit)");
        let s = GeminiRunner.parse_status(&pane);
        assert!(s.has_idle_prompt);
        assert!(!s.has_draft);
    }

    #[test]
    fn test_parse_status_typed_text_is_draft() {
        let pane = frame("fix the auth bug");
        let s = GeminiRunner.parse_status(&pane);
        assert!(s.has_idle_prompt);
        assert!(s.has_draft);
    }

    #[test]
    fn test_parse_status_single_char_is_draft() {
        let pane = frame("x");
        let s = GeminiRunner.parse_status(&pane);
        assert!(s.has_draft);
    }

    #[test]
    fn test_parse_status_only_cursor_block_is_not_draft() {
        // Ink renders the cursor as an inverse-styled full block when
        // showCursor is true and the buffer is empty. Strip it like
        // Codex's parser does.
        let pane = frame("\u{2588}");
        let s = GeminiRunner.parse_status(&pane);
        assert!(s.has_idle_prompt);
        assert!(!s.has_draft);
    }

    #[test]
    fn test_parse_status_busy_indicator_marks_running() {
        // LoadingIndicator.tsx prints this whenever StreamingState.Responding.
        let pane = "Some prior output\n● Thinking... (esc to cancel, 5s)\n";
        let s = GeminiRunner.parse_status(pane);
        assert!(s.is_busy, "`esc to cancel,` must set is_busy");
        assert!(
            !s.has_idle_prompt,
            "busy must suppress has_idle_prompt so Tier 3 reaches Running"
        );
    }

    #[test]
    fn test_parse_status_thinking_phrase_marks_running() {
        // Secondary signal: `Thinking...` alone (e.g., the indicator was
        // captured before the cancel timer rendered, or showCancelAndTimer
        // is suppressed).
        let pane = "Some prior output\nThinking...\n";
        let s = GeminiRunner.parse_status(pane);
        assert!(s.is_busy);
    }

    #[test]
    fn test_parse_status_radio_select_is_paused_not_idle() {
        // Tool-confirmation dialog. `●` marks the highlighted option.
        let pane = "Run command: `rm -rf /tmp/cache`?\n\
                    ● Allow once\n  Allow for this session\n  Cancel\n";
        let s = GeminiRunner.parse_status(pane);
        assert!(s.has_idle_prompt);
        assert!(s.has_question);
        assert!(!s.has_draft);
    }

    #[test]
    fn test_parse_status_radio_select_overrides_frame_below() {
        // Even if a stale input frame is visible above the confirmation,
        // the dialog takes precedence (has_question wins in resolve).
        let pane = format!(
            "{}\n● Allow once\n  Cancel\n",
            frame("  Type your message or @path/to/file")
        );
        let s = GeminiRunner.parse_status(&pane);
        assert!(s.has_question);
        assert!(!s.has_draft);
    }

    #[test]
    fn test_parse_status_busy_overrides_radio_select() {
        // If both `esc to cancel,` and a `●` are in window, busy wins
        // because turn-in-progress > waiting-on-confirmation visually
        // (the confirmation is from a prior tool call still on screen).
        let pane = "● Allow once\n  Cancel\nThinking... (esc to cancel, 2s)\n";
        let s = GeminiRunner.parse_status(pane);
        assert!(s.is_busy);
        assert!(!s.has_question);
    }

    #[test]
    fn test_parse_status_draft_with_trailing_blank_padding() {
        // Codex pitfall reproduced for Gemini: TUI absolute positioning
        // means tmux pads the bottom with blank lines. The frame must
        // still be located after stripping.
        let mut pane = frame("fix the bug");
        for _ in 0..120 {
            pane.push('\n');
        }
        let s = GeminiRunner.parse_status(&pane);
        assert!(s.has_idle_prompt);
        assert!(s.has_draft, "trailing blank padding must not hide frame");
    }

    #[test]
    fn test_parse_status_busy_keyword_in_prose_outside_window_ignored() {
        // The busy markers can appear in conversation text. We only scan
        // the bottom 30 lines so old prose mentioning "Thinking..." or
        // "esc to cancel," doesn't false-positive once the turn is done.
        let mut pane = String::new();
        pane.push_str("Earlier the agent said: Thinking... never finished.\n");
        for _ in 0..40 {
            pane.push_str("scrollback line\n");
        }
        pane.push_str(&frame(""));
        let s = GeminiRunner.parse_status(&pane);
        assert!(!s.is_busy, "old `Thinking...` outside scan window");
        assert!(s.has_idle_prompt);
    }

    #[test]
    fn test_extract_session_id_returns_none() {
        assert_eq!(GeminiRunner.extract_session_id("anything"), None);
    }

    #[test]
    fn test_restart_command_falls_back_to_original_when_no_tool_data() {
        assert_eq!(GeminiRunner.restart_command("gemini", "{}"), "gemini");
    }

    #[test]
    fn test_restart_command_ignores_tool_data_no_resume_supported() {
        // Gemini 0.9 has no resume CLI. Pin this so a future Gemini
        // release with `--resume` is intentional, not accidental.
        assert_eq!(
            GeminiRunner.restart_command("gemini", r#"{"gemini_session_id":"abc-123"}"#),
            "gemini"
        );
    }

    #[test]
    fn test_is_implemented_returns_true() {
        assert!(GeminiRunner.is_implemented());
    }

    #[test]
    fn test_tool_data_session_id_key() {
        assert_eq!(GeminiRunner.tool_data_session_id_key(), "gemini_session_id");
    }

    #[test]
    fn test_wants_ansi_escapes_default_false() {
        // Placeholder text is a stable literal regardless of theme color,
        // so we don't need SGR codes to distinguish placeholder from
        // typed input. ANSI-stripped capture is fine.
        assert!(!GeminiRunner.wants_ansi_escapes());
    }
}
