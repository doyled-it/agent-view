//! Idempotent merge of agent-view's hook entries into Gemini CLI's
//! settings.json. Mirrors `claude/hooks.rs` — Gemini's hook config schema
//! is the same JSON shape (`hooks.{Event}` → matchers → hooks list).
//!
//! Differences from Claude:
//! - Config dir is `~/.gemini` with no env-var override (per agent-deck's
//!   `internal/session/gemini.go`: "Unlike Claude, Gemini has no
//!   GEMINI_CONFIG_DIR env var override").
//! - Event set is narrower: `SessionStart`, `BeforeAgent`, `AfterAgent`,
//!   `SessionEnd` — the BeforeAgent/AfterAgent pair gives us turn-level
//!   Running/Idle transitions; the SessionStart/End pair brackets lifecycle.
//! - Identifies our entries by trailing ` gemini-hook` command suffix.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const COMMAND_SUFFIX: &str = " gemini-hook";

#[derive(Debug, Clone)]
struct HookEventConfig {
    event: &'static str,
    matcher: &'static str,
    async_: bool,
}

const EVENTS: &[HookEventConfig] = &[
    HookEventConfig {
        event: "SessionStart",
        matcher: "",
        async_: true,
    },
    HookEventConfig {
        event: "BeforeAgent",
        matcher: "",
        async_: true,
    },
    HookEventConfig {
        event: "AfterAgent",
        matcher: "",
        async_: true,
    },
    HookEventConfig {
        event: "SessionEnd",
        matcher: "",
        async_: true,
    },
];

/// Locate Gemini config dir. Gemini has no env-var override (unlike Claude's
/// `CLAUDE_CONFIG_DIR`), so this is always `~/.gemini`.
pub fn gemini_config_dir() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".gemini"))
}

/// Resolve the full hook command: "<abs path to agent-view> gemini-hook".
pub fn resolve_hook_command() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe failed: {}", e))?;
    Ok(format!("{}{}", exe.display(), COMMAND_SUFFIX))
}

/// Merge our hook entries into `<config_dir>/settings.json`. Idempotent;
/// re-merges if drift is detected (matcher or async flag mismatch). Atomic
/// write via `<file>.tmp` + rename.
pub fn install_hooks_in(config_dir: &Path, hook_command: &str) -> Result<(), String> {
    fs::create_dir_all(config_dir).map_err(|e| format!("create config dir: {}", e))?;

    let settings_path = config_dir.join("settings.json");
    let mut root: Value = match fs::read_to_string(&settings_path) {
        Ok(s) if !s.trim().is_empty() => {
            serde_json::from_str(&s).map_err(|e| format!("parse settings.json: {}", e))?
        }
        _ => json!({}),
    };

    if !root.is_object() {
        return Err("settings.json root is not an object".to_string());
    }

    let hooks_value = root
        .as_object_mut()
        .unwrap()
        .entry("hooks".to_string())
        .or_insert_with(|| json!({}));

    if !hooks_value.is_object() {
        *hooks_value = json!({});
    }
    let hooks_obj = hooks_value.as_object_mut().unwrap();

    let mut changed = false;
    for cfg in EVENTS {
        let entry = hooks_obj
            .entry(cfg.event.to_string())
            .or_insert_with(|| json!([]));
        if !entry.is_array() {
            *entry = json!([]);
            changed = true;
        }
        if merge_event(entry.as_array_mut().unwrap(), cfg, hook_command) {
            changed = true;
        }
    }

    if !changed && file_already_matches(&settings_path, &root) {
        return Ok(());
    }

    let pretty = serde_json::to_vec_pretty(&root).map_err(|e| format!("serialize: {}", e))?;
    let tmp = settings_path.with_extension("json.tmp");
    fs::write(&tmp, &pretty).map_err(|e| format!("write tmp: {}", e))?;
    fs::rename(&tmp, &settings_path).map_err(|e| format!("rename: {}", e))?;
    Ok(())
}

fn file_already_matches(path: &Path, expected: &Value) -> bool {
    match fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str::<Value>(&s) {
            Ok(v) => &v == expected,
            Err(_) => false,
        },
        Err(_) => false,
    }
}

fn merge_event(matchers: &mut Vec<Value>, cfg: &HookEventConfig, hook_command: &str) -> bool {
    let target_idx = matchers
        .iter()
        .position(|m| m.get("matcher").and_then(|v| v.as_str()).unwrap_or("") == cfg.matcher);

    let matcher_idx = match target_idx {
        Some(i) => i,
        None => {
            matchers.push(json!({
                "matcher": cfg.matcher,
                "hooks": []
            }));
            matchers.len() - 1
        }
    };

    let matcher_block = &mut matchers[matcher_idx];
    let hooks = matcher_block
        .get_mut("hooks")
        .and_then(|v| v.as_array_mut());
    let hooks = match hooks {
        Some(h) => h,
        None => {
            matcher_block["hooks"] = json!([]);
            matcher_block["hooks"].as_array_mut().unwrap()
        }
    };

    if let Some(existing) = hooks.iter_mut().find(|h| {
        h.get("command")
            .and_then(|v| v.as_str())
            .map(|c| c.ends_with(COMMAND_SUFFIX))
            .unwrap_or(false)
    }) {
        let want = json!({
            "type": "command",
            "command": hook_command,
            "async": cfg.async_,
        });
        if existing != &want {
            *existing = want;
            return true;
        }
        return false;
    }

    hooks.push(json!({
        "type": "command",
        "command": hook_command,
        "async": cfg.async_,
    }));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_install_creates_settings_when_missing() {
        let dir = tempfile::tempdir().unwrap();
        install_hooks_in(dir.path(), "/bin/agent-view gemini-hook").unwrap();
        let s = fs::read_to_string(dir.path().join("settings.json")).unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        let hooks = v.get("hooks").unwrap().as_object().unwrap();
        for cfg in EVENTS {
            assert!(hooks.contains_key(cfg.event), "missing event {}", cfg.event);
        }
    }

    #[test]
    fn test_install_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        install_hooks_in(dir.path(), "/bin/agent-view gemini-hook").unwrap();
        let first = fs::read_to_string(dir.path().join("settings.json")).unwrap();
        install_hooks_in(dir.path(), "/bin/agent-view gemini-hook").unwrap();
        let second = fs::read_to_string(dir.path().join("settings.json")).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn test_install_preserves_third_party_hook() {
        let dir = tempfile::tempdir().unwrap();
        let initial = json!({
            "hooks": {
                "BeforeAgent": [
                    { "matcher": "", "hooks": [
                        { "type": "command", "command": "/usr/local/bin/other-tool" }
                    ]}
                ]
            }
        });
        fs::write(
            dir.path().join("settings.json"),
            serde_json::to_string_pretty(&initial).unwrap(),
        )
        .unwrap();

        install_hooks_in(dir.path(), "/bin/agent-view gemini-hook").unwrap();

        let s = fs::read_to_string(dir.path().join("settings.json")).unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        let arr = v["hooks"]["BeforeAgent"].as_array().unwrap();
        let hooks_arr = arr[0]["hooks"].as_array().unwrap();
        assert!(hooks_arr
            .iter()
            .any(|h| h["command"] == "/usr/local/bin/other-tool"));
        assert!(hooks_arr
            .iter()
            .any(|h| h["command"] == "/bin/agent-view gemini-hook"));
    }

    #[test]
    fn test_install_repairs_drifted_command_path() {
        let dir = tempfile::tempdir().unwrap();
        let initial = json!({
            "hooks": {
                "AfterAgent": [
                    { "matcher": "", "hooks": [
                        { "type": "command", "command": "/old/path/agent-view gemini-hook", "async": true }
                    ]}
                ]
            }
        });
        fs::write(
            dir.path().join("settings.json"),
            serde_json::to_string_pretty(&initial).unwrap(),
        )
        .unwrap();

        install_hooks_in(dir.path(), "/new/path/agent-view gemini-hook").unwrap();
        let v: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(
            v["hooks"]["AfterAgent"][0]["hooks"][0]["command"],
            "/new/path/agent-view gemini-hook"
        );
    }

    #[test]
    fn test_install_preserves_other_top_level_settings() {
        let dir = tempfile::tempdir().unwrap();
        let initial = json!({
            "theme": "dark",
            "model": "gemini-2.5-pro"
        });
        fs::write(
            dir.path().join("settings.json"),
            serde_json::to_string_pretty(&initial).unwrap(),
        )
        .unwrap();

        install_hooks_in(dir.path(), "/bin/agent-view gemini-hook").unwrap();

        let v: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["model"], "gemini-2.5-pro");
    }

    #[test]
    fn test_install_writes_all_four_lifecycle_events() {
        let dir = tempfile::tempdir().unwrap();
        install_hooks_in(dir.path(), "/bin/agent-view gemini-hook").unwrap();
        let v: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("settings.json")).unwrap())
                .unwrap();
        for ev in ["SessionStart", "BeforeAgent", "AfterAgent", "SessionEnd"] {
            assert!(
                v["hooks"][ev].is_array(),
                "missing or non-array event {}",
                ev
            );
        }
    }

    #[test]
    fn test_gemini_config_dir_is_dot_gemini_under_home() {
        let got = gemini_config_dir().unwrap();
        let want = dirs::home_dir().unwrap().join(".gemini");
        assert_eq!(got, want);
    }
}
