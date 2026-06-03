//! Installs a global OpenCode plugin that forwards selected session events
//! to `agent-view opencode-hook`.

use std::fs;
use std::path::{Path, PathBuf};

const SUBCOMMAND: &str = "opencode-hook";

pub fn opencode_config_dir() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".config/opencode"))
}

pub fn resolve_hook_command() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe failed: {}", e))?;
    Ok(format!("{} {}", exe.display(), SUBCOMMAND))
}

pub fn install_hooks_in(config_dir: &Path, hook_command: &str) -> Result<(), String> {
    let plugins_dir = config_dir.join("plugins");
    fs::create_dir_all(&plugins_dir).map_err(|e| format!("create plugins dir: {}", e))?;

    let plugin_path = plugins_dir.join("agent-view.js");
    let body = render_plugin(hook_command)?;
    if fs::read_to_string(&plugin_path)
        .map(|existing| existing == body)
        .unwrap_or(false)
    {
        return Ok(());
    }

    let tmp = plugin_path.with_extension("js.tmp");
    fs::write(&tmp, body).map_err(|e| format!("write tmp: {}", e))?;
    fs::rename(&tmp, &plugin_path).map_err(|e| format!("rename: {}", e))?;
    Ok(())
}

fn render_plugin(hook_command: &str) -> Result<String, String> {
    let (binary, subcommand) = split_hook_command(hook_command);
    let binary_json =
        serde_json::to_string(binary).map_err(|e| format!("serialize binary: {}", e))?;
    let subcommand_json =
        serde_json::to_string(subcommand).map_err(|e| format!("serialize subcommand: {}", e))?;
    Ok(format!(
        r#"const AGENT_VIEW_BINARY = {binary_json};
const AGENT_VIEW_SUBCOMMAND = {subcommand_json};

function statusType(status) {{
  if (!status) return "";
  if (typeof status === "string") return status;
  if (typeof status.type === "string") return status.type;
  return "";
}}

function sessionID(properties) {{
  if (!properties) return "";
  if (typeof properties.sessionID === "string") return properties.sessionID;
  if (typeof properties.session_id === "string") return properties.session_id;
  if (properties.info && typeof properties.info.id === "string") return properties.info.id;
  return "";
}}

function payloadFor(event) {{
  const properties = event.properties || {{}};
  switch (event.type) {{
    case "session.created":
      return {{ event: event.type, session_id: sessionID(properties.info ? {{ info: properties.info }} : properties) }};
    case "session.status":
      return {{ event: event.type, session_id: sessionID(properties), status: statusType(properties.status) }};
    case "session.idle":
    case "session.compacted":
    case "session.error":
    case "permission.asked":
    case "permission.updated":
    case "permission.replied":
      return {{ event: event.type, session_id: sessionID(properties) }};
    default:
      return null;
  }}
}}

export const AgentViewPlugin = async ({{ $ }}) => {{
  return {{
    event: async ({{ event }}) => {{
      const payload = payloadFor(event);
      if (!payload) return;
      await $`${{AGENT_VIEW_BINARY}} ${{AGENT_VIEW_SUBCOMMAND}} ${{JSON.stringify(payload)}}`;
    }},
  }};
}};
"#
    ))
}

fn split_hook_command(hook_command: &str) -> (&str, &str) {
    hook_command
        .strip_suffix(&format!(" {}", SUBCOMMAND))
        .map(|binary| (binary, SUBCOMMAND))
        .unwrap_or((hook_command, SUBCOMMAND))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn install_creates_global_plugin_file() {
        let dir = tempfile::tempdir().unwrap();
        install_hooks_in(dir.path(), "/bin/agent-view opencode-hook").unwrap();

        let plugin = fs::read_to_string(dir.path().join("plugins/agent-view.js")).unwrap();
        assert!(plugin.contains("AgentViewPlugin"));
        assert!(plugin.contains("/bin/agent-view"));
        assert!(plugin.contains("opencode-hook"));
        assert!(plugin.contains("session.status"));
        assert!(plugin.contains("permission.asked"));
    }

    #[test]
    fn install_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        install_hooks_in(dir.path(), "/bin/agent-view opencode-hook").unwrap();
        let first = fs::read_to_string(dir.path().join("plugins/agent-view.js")).unwrap();
        install_hooks_in(dir.path(), "/bin/agent-view opencode-hook").unwrap();
        let second = fs::read_to_string(dir.path().join("plugins/agent-view.js")).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn install_repairs_command_path() {
        let dir = tempfile::tempdir().unwrap();
        install_hooks_in(dir.path(), "/old/agent-view opencode-hook").unwrap();
        install_hooks_in(dir.path(), "/new/agent-view opencode-hook").unwrap();

        let plugin = fs::read_to_string(dir.path().join("plugins/agent-view.js")).unwrap();
        assert!(plugin.contains("/new/agent-view"));
        assert!(!plugin.contains("/old/agent-view"));
    }

    #[test]
    fn opencode_config_dir_is_dot_config_opencode_under_home() {
        let got = opencode_config_dir().unwrap();
        let want = dirs::home_dir().unwrap().join(".config/opencode");
        assert_eq!(got, want);
    }
}
