//! Idempotent merge of agent-view's hook entries into Claude Code's
//! settings.json. Modeled on agent-deck's `internal/session/claude_hooks.go`.
//!
//! Identifies our entries by the trailing ` hook` suffix on the command
//! string (so absolute-path drift across binary moves is tolerated).

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const COMMAND_SUFFIX: &str = " hook";

#[derive(Debug, Clone)]
struct HookEventConfig {
    event: &'static str,
    matcher: &'static str, // "" = no matcher
    async_: bool,          // false = synchronous (Claude Code blocks on exit code)
}

const EVENTS: &[HookEventConfig] = &[
    HookEventConfig {
        event: "SessionStart",
        matcher: "",
        async_: true,
    },
    HookEventConfig {
        event: "UserPromptSubmit",
        matcher: "",
        async_: true,
    },
    HookEventConfig {
        event: "Stop",
        matcher: "",
        async_: true,
    },
    HookEventConfig {
        event: "PermissionRequest",
        matcher: "",
        async_: false,
    },
    HookEventConfig {
        event: "Notification",
        matcher: "permission_prompt|elicitation_dialog",
        async_: true,
    },
    HookEventConfig {
        event: "PreCompact",
        matcher: "",
        async_: false,
    },
    HookEventConfig {
        event: "SessionEnd",
        matcher: "",
        async_: true,
    },
];

/// Locate Claude config dir: $CLAUDE_CONFIG_DIR if set, else ~/.claude.
pub fn claude_config_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(d));
    }
    Some(dirs::home_dir()?.join(".claude"))
}

/// Resolve the full hook command: "<abs path to agent-view> hook".
pub fn resolve_hook_command() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe failed: {}", e))?;
    Ok(format!("{}{}", exe.display(), COMMAND_SUFFIX))
}

/// Merge our hook entries into `<config_dir>/settings.json`. Idempotent;
/// re-merges if drift is detected (matcher or async flag mismatch). Atomic
/// write via `<file>.tmp` + rename. Returns Ok(()) whether or not changes
/// were made; callers may treat repeated calls as no-ops.
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

/// Returns true if the on-disk file already encodes `expected`. Used to
/// avoid pointless rewrites.
fn file_already_matches(path: &Path, expected: &Value) -> bool {
    match fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str::<Value>(&s) {
            Ok(v) => &v == expected,
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// Merge our hook entry into the matchers array for one event. Returns
/// true if the array was mutated. Preserves all other matchers and hooks.
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
        install_hooks_in(dir.path(), "/bin/agent-view hook").unwrap();
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
        install_hooks_in(dir.path(), "/bin/agent-view hook").unwrap();
        let first = fs::read_to_string(dir.path().join("settings.json")).unwrap();
        install_hooks_in(dir.path(), "/bin/agent-view hook").unwrap();
        let second = fs::read_to_string(dir.path().join("settings.json")).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn test_install_preserves_third_party_hook() {
        let dir = tempfile::tempdir().unwrap();
        let initial = json!({
            "hooks": {
                "Stop": [
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

        install_hooks_in(dir.path(), "/bin/agent-view hook").unwrap();

        let s = fs::read_to_string(dir.path().join("settings.json")).unwrap();
        let v: Value = serde_json::from_str(&s).unwrap();
        let stop_matchers = v["hooks"]["Stop"].as_array().unwrap();
        let hooks_arr = stop_matchers[0]["hooks"].as_array().unwrap();
        assert!(hooks_arr
            .iter()
            .any(|h| h["command"] == "/usr/local/bin/other-tool"));
        assert!(hooks_arr
            .iter()
            .any(|h| h["command"] == "/bin/agent-view hook"));
    }

    #[test]
    fn test_install_repairs_drifted_async_flag() {
        let dir = tempfile::tempdir().unwrap();
        let initial = json!({
            "hooks": {
                "Stop": [
                    { "matcher": "", "hooks": [
                        { "type": "command", "command": "/bin/agent-view hook", "async": false }
                    ]}
                ]
            }
        });
        fs::write(
            dir.path().join("settings.json"),
            serde_json::to_string_pretty(&initial).unwrap(),
        )
        .unwrap();

        install_hooks_in(dir.path(), "/bin/agent-view hook").unwrap();

        let v: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("settings.json")).unwrap())
                .unwrap();
        let stop_hook = &v["hooks"]["Stop"][0]["hooks"][0];
        assert_eq!(stop_hook["async"], json!(true));
    }

    #[test]
    fn test_install_repairs_drifted_command_path() {
        let dir = tempfile::tempdir().unwrap();
        let initial = json!({
            "hooks": {
                "Stop": [
                    { "matcher": "", "hooks": [
                        { "type": "command", "command": "/old/path/agent-view hook", "async": true }
                    ]}
                ]
            }
        });
        fs::write(
            dir.path().join("settings.json"),
            serde_json::to_string_pretty(&initial).unwrap(),
        )
        .unwrap();

        install_hooks_in(dir.path(), "/new/path/agent-view hook").unwrap();
        let v: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(
            v["hooks"]["Stop"][0]["hooks"][0]["command"],
            "/new/path/agent-view hook"
        );
    }

    #[test]
    fn test_install_preserves_other_top_level_settings() {
        let dir = tempfile::tempdir().unwrap();
        let initial = json!({
            "theme": "dark",
            "model": "claude-opus-4-7"
        });
        fs::write(
            dir.path().join("settings.json"),
            serde_json::to_string_pretty(&initial).unwrap(),
        )
        .unwrap();

        install_hooks_in(dir.path(), "/bin/agent-view hook").unwrap();

        let v: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(v["theme"], "dark");
        assert_eq!(v["model"], "claude-opus-4-7");
    }

    #[test]
    fn test_install_handles_notification_matcher() {
        let dir = tempfile::tempdir().unwrap();
        install_hooks_in(dir.path(), "/bin/agent-view hook").unwrap();
        let v: Value =
            serde_json::from_str(&fs::read_to_string(dir.path().join("settings.json")).unwrap())
                .unwrap();
        let notif = v["hooks"]["Notification"].as_array().unwrap();
        assert_eq!(notif[0]["matcher"], "permission_prompt|elicitation_dialog");
    }
}
