//! Idempotent merge of agent-view's notify hook into Codex's `config.toml`.
//! Mirrors `claude/hooks.rs` but writes a marker-bracketed TOML block
//! containing a single `notify = ["<exe>", "codex-notify"]` line.
//!
//! Codex's notify config supports exactly one program. If a foreign
//! `notify =` line is already present we refuse to clobber it and return
//! an Err with merge guidance.

use crate::core::runner::hook_io::atomic_write;
use std::fs;
use std::path::{Path, PathBuf};

const MARKER_BEGIN: &str = "# BEGIN AGENTVIEW CODEX NOTIFY";
const MARKER_END: &str = "# END AGENTVIEW CODEX NOTIFY";

/// Locate Codex config dir: `$CODEX_HOME` if set, else `~/.codex`.
pub fn codex_config_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("CODEX_HOME") {
        let trimmed = d.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    Some(dirs::home_dir()?.join(".codex"))
}

/// Resolve the notify command: `<abs path to agent-view> codex-notify`,
/// formatted as a TOML array body for the `notify = ...` line.
pub fn resolve_notify_command() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe failed: {}", e))?;
    Ok(format!(
        "[{:?}, \"codex-notify\"]",
        exe.display().to_string()
    ))
}

/// Merge our notify block into `<config_dir>/config.toml`. Idempotent.
/// Atomic write via `<file>.tmp` + rename.
///
/// Returns Err if a foreign `notify =` line is already present (Codex
/// supports only one notify program; we will not silently overwrite the
/// user's config).
pub fn install_hooks_in(config_dir: &Path, notify_command_body: &str) -> Result<(), String> {
    fs::create_dir_all(config_dir).map_err(|e| format!("create config dir: {}", e))?;
    let config_path = config_dir.join("config.toml");

    let existing = match fs::read_to_string(&config_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("read config.toml: {}", e)),
    };

    let new_block = format!(
        "{}\nnotify = {}\n{}\n",
        MARKER_BEGIN, notify_command_body, MARKER_END
    );

    let new_contents = if let Some(start) = existing.find(MARKER_BEGIN) {
        // Marker exists — replace the block, preserving surrounding content.
        let after_start = &existing[start..];
        let end_rel = after_start
            .find(MARKER_END)
            .ok_or_else(|| "marker block start present but end missing".to_string())?;
        let end = start + end_rel + MARKER_END.len();
        let before = &existing[..start];
        let after = &existing[end..];
        let after = after.strip_prefix('\n').unwrap_or(after);
        format!("{}{}{}", before, new_block, after)
    } else if foreign_notify_line(&existing) {
        return Err(format!(
            "{} already contains a `notify = ...` line not owned by agent-view; \
             please merge manually or remove the existing line before retrying",
            config_path.display()
        ));
    } else if existing.trim().is_empty() {
        new_block
    } else {
        // Prepend our block, preserving the rest.
        format!("{}\n{}", new_block, existing.trim_start())
    };

    atomic_write(&config_path, new_contents.as_bytes())
        .map_err(|e| format!("write config.toml: {}", e))
}

/// True if `content` contains a `notify = ...` line not surrounded by our
/// marker block. Detects bare `notify =` at the start of any line, ignoring
/// our own block.
///
/// Known limitation: this is a line-based heuristic, not a TOML parser. A
/// `notify = ...` key nested inside a `[some.section]` table would be
/// flagged as "foreign" even though it doesn't conflict with our top-level
/// key. In practice Codex's `notify` is documented as a top-level setting
/// and we have never seen it scoped under a table; if that becomes a
/// problem we should swap in `toml_edit` for a structure-aware check.
fn foreign_notify_line(content: &str) -> bool {
    let without_marker_block = strip_marker_block(content);
    without_marker_block.lines().any(|line| {
        line.trim_start().starts_with("notify ") || line.trim_start().starts_with("notify=")
    })
}

fn strip_marker_block(content: &str) -> String {
    let Some(start) = content.find(MARKER_BEGIN) else {
        return content.to_string();
    };
    let after_start = &content[start..];
    let Some(end_rel) = after_start.find(MARKER_END) else {
        return content.to_string();
    };
    let end = start + end_rel + MARKER_END.len();
    format!("{}{}", &content[..start], &content[end..])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn cmd() -> String {
        r#"["/usr/bin/agent-view", "codex-notify"]"#.to_string()
    }

    #[test]
    fn test_install_writes_marker_block_to_fresh_config() {
        let dir = TempDir::new().unwrap();
        install_hooks_in(dir.path(), &cmd()).unwrap();
        let content = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(content.contains(MARKER_BEGIN));
        assert!(content.contains(MARKER_END));
        assert!(content.contains(r#"notify = ["/usr/bin/agent-view", "codex-notify"]"#));
    }

    #[test]
    fn test_install_is_idempotent() {
        let dir = TempDir::new().unwrap();
        install_hooks_in(dir.path(), &cmd()).unwrap();
        install_hooks_in(dir.path(), &cmd()).unwrap();
        let content = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert_eq!(content.matches(MARKER_BEGIN).count(), 1);
    }

    #[test]
    fn test_install_replaces_block_on_binary_path_drift() {
        let dir = TempDir::new().unwrap();
        install_hooks_in(dir.path(), &cmd()).unwrap();
        let new_cmd = r#"["/new/path/agent-view", "codex-notify"]"#;
        install_hooks_in(dir.path(), new_cmd).unwrap();
        let content = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(content.contains("/new/path/agent-view"));
        assert!(!content.contains("/usr/bin/agent-view"));
    }

    #[test]
    fn test_install_errors_on_foreign_notify_line() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            "notify = [\"some-other-program\"]\n",
        )
        .unwrap();
        let err = install_hooks_in(dir.path(), &cmd()).unwrap_err();
        assert!(err.contains("merge manually"));
    }

    #[test]
    fn test_install_preserves_other_content() {
        let dir = TempDir::new().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            "[model]\nname = \"gpt-5\"\n",
        )
        .unwrap();
        install_hooks_in(dir.path(), &cmd()).unwrap();
        let content = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(content.contains(MARKER_BEGIN));
        assert!(content.contains("[model]"));
        assert!(content.contains("name = \"gpt-5\""));
    }

    #[test]
    fn test_codex_config_dir_uses_env() {
        let _guard = crate::core::runner::hook_io::lock_env();
        std::env::set_var("CODEX_HOME", "/tmp/my-codex-test");
        let dir = codex_config_dir().unwrap();
        assert_eq!(dir, PathBuf::from("/tmp/my-codex-test"));
        std::env::remove_var("CODEX_HOME");
    }
}
