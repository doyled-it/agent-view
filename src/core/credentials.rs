//! Loads the Claude Code OAuth access token.
//!
//! macOS: stored in the keychain under service "Claude Code-credentials".
//! Linux: stored in `~/.config/claude/.credentials.json`.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    oauth: Option<OauthBlock>,
}

#[derive(Debug, Deserialize)]
struct OauthBlock {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
}

#[allow(dead_code)]
pub fn load_token() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Some(t) = load_token_from_keychain() {
            return Some(t);
        }
    }
    load_token_from_file()
}

#[cfg(target_os = "macos")]
fn load_token_from_keychain() -> Option<String> {
    let output = std::process::Command::new("security")
        .args([
            "find-generic-password",
            "-s",
            "Claude Code-credentials",
            "-w",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8(output.stdout).ok()?;
    extract_token_from_json(raw.trim())
}

fn load_token_from_file() -> Option<String> {
    let home = dirs::home_dir()?;
    let path = home
        .join(".config")
        .join("claude")
        .join(".credentials.json");
    let raw = std::fs::read_to_string(path).ok()?;
    extract_token_from_json(&raw)
}

fn extract_token_from_json(raw: &str) -> Option<String> {
    let parsed: CredentialsFile = serde_json::from_str(raw).ok()?;
    parsed.oauth?.access_token
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_token_valid() {
        let json = r#"{"claudeAiOauth":{"accessToken":"sk-ant-oat01-abc","refreshToken":"x"}}"#;
        assert_eq!(
            extract_token_from_json(json),
            Some("sk-ant-oat01-abc".to_string())
        );
    }

    #[test]
    fn test_extract_token_missing_field() {
        let json = r#"{"claudeAiOauth":{"refreshToken":"x"}}"#;
        assert_eq!(extract_token_from_json(json), None);
    }

    #[test]
    fn test_extract_token_missing_block() {
        let json = r#"{"otherKey":"value"}"#;
        assert_eq!(extract_token_from_json(json), None);
    }

    #[test]
    fn test_extract_token_garbage() {
        assert_eq!(extract_token_from_json("not json"), None);
    }
}
