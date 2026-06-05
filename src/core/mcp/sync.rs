use crate::types::Tool;
use serde_json::Value as JsonValue;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use toml_edit::{Array, DocumentMut, Item, Table, Value as TomlValue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSyncPlan {
    pub inventory_rows: Vec<McpSyncInventoryRow>,
    pub proposals: Vec<McpSyncProposal>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSyncInventoryRow {
    pub server_id: String,
    pub claude: McpSyncAvailability,
    pub codex: McpSyncAvailability,
    pub opencode: McpSyncAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpSyncAvailability {
    Configured,
    Missing,
    Unsupported(String),
}

#[derive(Debug, Clone)]
pub struct McpSyncProposal {
    pub server_id: String,
    pub source: Tool,
    pub target: Tool,
    pub preview_lines: Vec<String>,
    target_definition: TargetDefinition,
}

impl PartialEq for McpSyncProposal {
    fn eq(&self, other: &Self) -> bool {
        self.server_id == other.server_id
            && self.source == other.source
            && self.target == other.target
            && self.preview_lines == other.preview_lines
    }
}

impl Eq for McpSyncProposal {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedMcpSyncTexts {
    pub claude_settings: Option<String>,
    pub codex_config: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSyncConfigPaths {
    pub claude_settings: PathBuf,
    pub codex_config: PathBuf,
}

#[derive(Debug, Clone)]
enum TargetDefinition {
    Claude(JsonValue),
    Codex(Item),
}

#[derive(Debug, Clone)]
struct SourceServer {
    id: String,
    runner: Tool,
    definition: SourceDefinition,
}

#[derive(Debug, Clone)]
enum SourceDefinition {
    Claude(JsonValue),
    Codex(Item),
}

pub fn build_sync_plan_from_texts(
    claude_settings: Option<&str>,
    codex_config: Option<&str>,
) -> Result<McpSyncPlan, String> {
    let claude_servers = match claude_settings {
        Some(text) => parse_claude_servers(text)?,
        None => Vec::new(),
    };
    let codex_servers = match codex_config {
        Some(text) => parse_codex_servers(text)?,
        None => Vec::new(),
    };

    let claude_ids = server_id_set(&claude_servers);
    let codex_ids = server_id_set(&codex_servers);
    let inventory_rows = build_inventory_rows(&claude_ids, &codex_ids);
    let mut proposals = Vec::new();

    for server in &claude_servers {
        if !codex_ids.contains(server.id.as_str()) {
            let target_definition =
                TargetDefinition::Codex(claude_json_to_codex_item(source_claude_json(server)?)?);
            let preview_lines = preview_codex_write(&server.id, &target_definition);
            proposals.push(McpSyncProposal {
                server_id: server.id.clone(),
                source: Tool::Claude,
                target: Tool::Codex,
                preview_lines,
                target_definition,
            });
        }
    }

    for server in &codex_servers {
        if !claude_ids.contains(server.id.as_str()) {
            let target_definition =
                TargetDefinition::Claude(codex_item_to_claude_json(source_codex_item(server)?)?);
            proposals.push(McpSyncProposal {
                server_id: server.id.clone(),
                source: Tool::Codex,
                target: Tool::Claude,
                preview_lines: preview_claude_write(&server.id),
                target_definition,
            });
        }
    }

    Ok(McpSyncPlan {
        inventory_rows,
        proposals,
    })
}

pub fn apply_sync_proposal_to_texts(
    proposal: &McpSyncProposal,
    claude_settings: Option<&str>,
    codex_config: Option<&str>,
) -> Result<AppliedMcpSyncTexts, String> {
    match (&proposal.target, &proposal.target_definition) {
        (Tool::Codex, TargetDefinition::Codex(item)) => {
            let updated = apply_codex_definition(
                codex_config.unwrap_or_default(),
                &proposal.server_id,
                item.clone(),
            )?;
            Ok(AppliedMcpSyncTexts {
                claude_settings: claude_settings.map(str::to_string),
                codex_config: Some(updated),
            })
        }
        (Tool::Claude, TargetDefinition::Claude(definition)) => {
            let updated = apply_claude_definition(
                claude_settings.unwrap_or("{}"),
                &proposal.server_id,
                definition.clone(),
            )?;
            Ok(AppliedMcpSyncTexts {
                claude_settings: Some(updated),
                codex_config: codex_config.map(str::to_string),
            })
        }
        _ => Err(format!(
            "proposal target {:?} does not match converted MCP definition",
            proposal.target
        )),
    }
}

pub fn default_sync_config_paths() -> Result<McpSyncConfigPaths, String> {
    let claude_dir = crate::core::runner::claude::hooks::claude_config_dir()
        .ok_or_else(|| "no home directory for Claude settings.json".to_string())?;
    let codex_dir = crate::core::runner::codex::hooks::codex_config_dir()
        .ok_or_else(|| "no home directory for Codex config.toml".to_string())?;
    Ok(McpSyncConfigPaths {
        claude_settings: claude_dir.join("settings.json"),
        codex_config: codex_dir.join("config.toml"),
    })
}

pub fn load_sync_plan_from_paths(paths: &McpSyncConfigPaths) -> Result<McpSyncPlan, String> {
    let claude_settings = std::fs::read_to_string(&paths.claude_settings).ok();
    let codex_config = std::fs::read_to_string(&paths.codex_config).ok();
    build_sync_plan_from_texts(claude_settings.as_deref(), codex_config.as_deref())
}

pub fn apply_sync_proposal_to_paths(
    proposal: &McpSyncProposal,
    paths: &McpSyncConfigPaths,
) -> Result<(), String> {
    let claude_settings = std::fs::read_to_string(&paths.claude_settings).ok();
    let codex_config = std::fs::read_to_string(&paths.codex_config).ok();
    let applied = apply_sync_proposal_to_texts(
        proposal,
        claude_settings.as_deref(),
        codex_config.as_deref(),
    )?;

    match proposal.target {
        Tool::Claude => {
            if let Some(parent) = paths.claude_settings.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("create Claude config dir: {}", e))?;
            }
            let Some(text) = applied.claude_settings else {
                return Err("Claude sync produced no settings.json text".to_string());
            };
            std::fs::write(&paths.claude_settings, text)
                .map_err(|e| format!("write Claude settings.json: {}", e))?;
        }
        Tool::Codex => {
            if let Some(parent) = paths.codex_config.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("create Codex config dir: {}", e))?;
            }
            let Some(text) = applied.codex_config else {
                return Err("Codex sync produced no config.toml text".to_string());
            };
            std::fs::write(&paths.codex_config, text)
                .map_err(|e| format!("write Codex config.toml: {}", e))?;
        }
        _ => {
            return Err(format!(
                "MCP sync writes are not supported for {}",
                proposal.target
            ));
        }
    }

    Ok(())
}

fn parse_claude_servers(settings_json: &str) -> Result<Vec<SourceServer>, String> {
    let value: JsonValue = serde_json::from_str(settings_json)
        .map_err(|e| format!("parse Claude settings.json: {}", e))?;
    let servers = value
        .get("mcpServers")
        .and_then(JsonValue::as_object)
        .map(|servers| {
            servers
                .iter()
                .map(|(id, definition)| SourceServer {
                    id: id.clone(),
                    runner: Tool::Claude,
                    definition: SourceDefinition::Claude(definition.clone()),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(servers)
}

fn parse_codex_servers(config_toml: &str) -> Result<Vec<SourceServer>, String> {
    let doc = config_toml
        .parse::<DocumentMut>()
        .map_err(|e| format!("parse Codex config.toml: {}", e))?;
    let Some(mcp_servers) = doc.get("mcp_servers").and_then(Item::as_table) else {
        return Ok(Vec::new());
    };
    Ok(mcp_servers
        .iter()
        .filter_map(|(id, item)| {
            if item.as_table().is_some() {
                Some(SourceServer {
                    id: id.to_string(),
                    runner: Tool::Codex,
                    definition: SourceDefinition::Codex(item.clone()),
                })
            } else {
                None
            }
        })
        .collect())
}

fn server_id_set(servers: &[SourceServer]) -> BTreeSet<&str> {
    servers.iter().map(|server| server.id.as_str()).collect()
}

fn build_inventory_rows(
    claude_ids: &BTreeSet<&str>,
    codex_ids: &BTreeSet<&str>,
) -> Vec<McpSyncInventoryRow> {
    let mut ids = BTreeSet::new();
    ids.extend(claude_ids.iter().copied());
    ids.extend(codex_ids.iter().copied());
    ids.into_iter()
        .map(|server_id| McpSyncInventoryRow {
            server_id: server_id.to_string(),
            claude: configured_or_missing(claude_ids, server_id),
            codex: configured_or_missing(codex_ids, server_id),
            opencode: McpSyncAvailability::Unsupported(
                "OpenCode MCP sync is not available yet".to_string(),
            ),
        })
        .collect()
}

fn configured_or_missing(ids: &BTreeSet<&str>, server_id: &str) -> McpSyncAvailability {
    if ids.contains(server_id) {
        McpSyncAvailability::Configured
    } else {
        McpSyncAvailability::Missing
    }
}

fn source_claude_json(server: &SourceServer) -> Result<&JsonValue, String> {
    match (&server.runner, &server.definition) {
        (Tool::Claude, SourceDefinition::Claude(value)) => Ok(value),
        _ => Err(format!("server '{}' is not a Claude MCP server", server.id)),
    }
}

fn source_codex_item(server: &SourceServer) -> Result<&Item, String> {
    match (&server.runner, &server.definition) {
        (Tool::Codex, SourceDefinition::Codex(item)) => Ok(item),
        _ => Err(format!("server '{}' is not a Codex MCP server", server.id)),
    }
}

fn claude_json_to_codex_item(definition: &JsonValue) -> Result<Item, String> {
    let object = definition
        .as_object()
        .ok_or_else(|| "Claude MCP server definition must be an object".to_string())?;
    let mut table = Table::new();
    for key in ordered_json_keys(object) {
        let value = object.get(key).expect("ordered key from object");
        if let Some(nested) = json_object_to_toml_table(value)? {
            table.insert(key, Item::Table(nested));
        } else if let Some(value) = json_to_toml_value(value)? {
            table.insert(key, Item::Value(value));
        }
    }
    Ok(Item::Table(table))
}

fn codex_item_to_claude_json(item: &Item) -> Result<JsonValue, String> {
    let table = item
        .as_table()
        .ok_or_else(|| "Codex MCP server definition must be a table".to_string())?;
    let mut object = serde_json::Map::new();
    for (key, item) in table.iter() {
        object.insert(key.to_string(), toml_item_to_json(item)?);
    }
    Ok(JsonValue::Object(object))
}

fn ordered_json_keys(object: &serde_json::Map<String, JsonValue>) -> Vec<&str> {
    const PREFERRED: &[&str] = &["command", "args", "url", "type", "env", "headers"];
    let mut keys = Vec::new();
    for preferred in PREFERRED {
        if object.contains_key(*preferred) {
            keys.push(*preferred);
        }
    }
    for key in object.keys() {
        if !PREFERRED.contains(&key.as_str()) {
            keys.push(key.as_str());
        }
    }
    keys
}

fn json_object_to_toml_table(value: &JsonValue) -> Result<Option<Table>, String> {
    let Some(object) = value.as_object() else {
        return Ok(None);
    };
    let mut table = Table::new();
    for key in ordered_json_keys(object) {
        let value = object.get(key).expect("ordered key from object");
        if let Some(nested) = json_object_to_toml_table(value)? {
            table.insert(key, Item::Table(nested));
        } else if let Some(value) = json_to_toml_value(value)? {
            table.insert(key, Item::Value(value));
        }
    }
    Ok(Some(table))
}

fn json_to_toml_value(value: &JsonValue) -> Result<Option<TomlValue>, String> {
    match value {
        JsonValue::Null => Ok(None),
        JsonValue::Bool(value) => Ok(Some(TomlValue::from(*value))),
        JsonValue::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(Some(TomlValue::from(value)))
            } else if let Some(value) = value.as_f64() {
                Ok(Some(TomlValue::from(value)))
            } else {
                Err("unsupported JSON number in MCP server definition".to_string())
            }
        }
        JsonValue::String(value) => Ok(Some(TomlValue::from(value.as_str()))),
        JsonValue::Array(values) => {
            let mut array = Array::default();
            for value in values {
                let Some(value) = json_to_toml_value(value)? else {
                    continue;
                };
                array.push(value);
            }
            Ok(Some(TomlValue::Array(array)))
        }
        JsonValue::Object(_) => Ok(None),
    }
}

fn toml_item_to_json(item: &Item) -> Result<JsonValue, String> {
    if let Some(value) = item.as_value() {
        return toml_value_to_json(value);
    }
    let table = item
        .as_table()
        .ok_or_else(|| "unsupported TOML item in MCP server definition".to_string())?;
    let mut object = serde_json::Map::new();
    for (key, item) in table.iter() {
        object.insert(key.to_string(), toml_item_to_json(item)?);
    }
    Ok(JsonValue::Object(object))
}

fn toml_value_to_json(value: &TomlValue) -> Result<JsonValue, String> {
    match value {
        TomlValue::String(value) => Ok(JsonValue::String(value.value().to_string())),
        TomlValue::Integer(value) => Ok(JsonValue::Number((*value.value()).into())),
        TomlValue::Float(value) => serde_json::Number::from_f64(*value.value())
            .map(JsonValue::Number)
            .ok_or_else(|| "unsupported TOML float in MCP server definition".to_string()),
        TomlValue::Boolean(value) => Ok(JsonValue::Bool(*value.value())),
        TomlValue::Array(values) => {
            let mut out = Vec::new();
            for value in values.iter() {
                out.push(toml_value_to_json(value)?);
            }
            Ok(JsonValue::Array(out))
        }
        TomlValue::InlineTable(values) => {
            let mut object = serde_json::Map::new();
            for (key, value) in values.iter() {
                object.insert(key.to_string(), toml_value_to_json(value)?);
            }
            Ok(JsonValue::Object(object))
        }
        TomlValue::Datetime(value) => Ok(JsonValue::String(value.to_string())),
    }
}

fn preview_codex_write(server_id: &str, target_definition: &TargetDefinition) -> Vec<String> {
    let TargetDefinition::Codex(item) = target_definition else {
        return Vec::new();
    };
    let mut lines = vec![
        "Will write Codex config.toml".to_string(),
        format!("  + [mcp_servers.{server_id}]"),
    ];
    if let Some(table) = item.as_table() {
        append_toml_table_preview(&mut lines, table, &format!("mcp_servers.{server_id}"), 4);
    }
    lines
}

fn append_toml_table_preview(lines: &mut Vec<String>, table: &Table, path: &str, indent: usize) {
    let nested_tables: HashMap<&str, &Table> = table
        .iter()
        .filter_map(|(key, item)| item.as_table().map(|table| (key, table)))
        .collect();
    for key in ordered_toml_keys(table) {
        let Some(item) = table.get(key) else {
            continue;
        };
        if item.as_table().is_none() {
            lines.push(format!("{}{} = {}", " ".repeat(indent), key, item));
        }
    }
    for key in ordered_toml_keys(table) {
        let Some(nested) = nested_tables.get(key) else {
            continue;
        };
        let nested_path = format!("{path}.{key}");
        lines.push(format!("{}[{nested_path}]", " ".repeat(indent)));
        append_toml_table_preview(lines, nested, &nested_path, indent);
    }
}

fn ordered_toml_keys(table: &Table) -> Vec<&str> {
    const PREFERRED: &[&str] = &["command", "args", "url", "type", "env", "headers"];
    let mut keys = Vec::new();
    for preferred in PREFERRED {
        if table.contains_key(preferred) {
            keys.push(*preferred);
        }
    }
    for (key, _) in table.iter() {
        if !PREFERRED.contains(&key) {
            keys.push(key);
        }
    }
    keys
}

fn preview_claude_write(server_id: &str) -> Vec<String> {
    vec![
        "Will write Claude settings.json".to_string(),
        format!("  + mcpServers.{server_id}"),
    ]
}

fn apply_codex_definition(
    config_toml: &str,
    server_id: &str,
    definition: Item,
) -> Result<String, String> {
    let mut doc = config_toml
        .parse::<DocumentMut>()
        .map_err(|e| format!("parse Codex config.toml: {}", e))?;
    if !doc.as_table().contains_key("mcp_servers") {
        doc["mcp_servers"] = Item::Table(Table::new());
    }
    let servers = doc["mcp_servers"]
        .as_table_mut()
        .ok_or_else(|| "Codex config mcp_servers is not a table".to_string())?;
    servers.insert(server_id, definition);
    Ok(doc.to_string())
}

fn apply_claude_definition(
    settings_json: &str,
    server_id: &str,
    definition: JsonValue,
) -> Result<String, String> {
    let mut value: JsonValue =
        serde_json::from_str(settings_json).map_err(|e| format!("parse Claude settings: {}", e))?;
    if !value.is_object() {
        value = JsonValue::Object(serde_json::Map::new());
    }
    let object = value
        .as_object_mut()
        .expect("Claude settings value normalized to object");
    if !object
        .get("mcpServers")
        .map(JsonValue::is_object)
        .unwrap_or(false)
    {
        object.insert(
            "mcpServers".to_string(),
            JsonValue::Object(serde_json::Map::new()),
        );
    }
    object
        .get_mut("mcpServers")
        .and_then(JsonValue::as_object_mut)
        .expect("mcpServers normalized to object")
        .insert(server_id.to_string(), definition);
    serde_json::to_string_pretty(&value).map_err(|e| format!("encode Claude settings: {}", e))
}

#[cfg(test)]
mod tests {
    use crate::types::Tool;
    use std::fs;

    #[test]
    fn planner_previews_claude_server_missing_from_codex() {
        let claude_settings = r#"
        {
            "theme": "dark",
            "mcpServers": {
                "wavecrest": {
                    "command": "uvx",
                    "args": ["wavecrest-mcp"],
                    "env": { "WAVECREST_TOKEN": "secret" }
                }
            }
        }
        "#;
        let codex_config = r#"
            model = "gpt-5.5"

            [mcp_servers.GitLabMITRE]
            url = "https://gitlab.example.test/api/v4/mcp"
        "#;

        let plan = super::build_sync_plan_from_texts(Some(claude_settings), Some(codex_config))
            .expect("sync plan");
        let proposal = plan
            .proposals
            .iter()
            .find(|proposal| {
                proposal.server_id == "wavecrest"
                    && proposal.source == Tool::Claude
                    && proposal.target == Tool::Codex
            })
            .expect("Claude to Codex proposal");

        assert_eq!(
            proposal.preview_lines,
            vec![
                "Will write Codex config.toml".to_string(),
                "  + [mcp_servers.wavecrest]".to_string(),
                "    command = \"uvx\"".to_string(),
                "    args = [\"wavecrest-mcp\"]".to_string(),
                "    [mcp_servers.wavecrest.env]".to_string(),
                "    WAVECREST_TOKEN = \"secret\"".to_string(),
            ]
        );

        let applied = super::apply_sync_proposal_to_texts(
            proposal,
            Some(claude_settings),
            Some(codex_config),
        )
        .expect("apply proposal");
        let updated_codex = applied.codex_config.expect("codex config updated");

        assert!(updated_codex.contains("model = \"gpt-5.5\""));
        assert!(updated_codex.contains("[mcp_servers.GitLabMITRE]"));
        assert!(updated_codex.contains("[mcp_servers.wavecrest]"));
        assert!(updated_codex.contains("command = \"uvx\""));
        assert!(updated_codex.contains("args = [\"wavecrest-mcp\"]"));
        assert!(updated_codex.contains("[mcp_servers.wavecrest.env]"));
    }

    #[test]
    fn planner_inventory_marks_missing_and_unsupported_runners() {
        let claude_settings = r#"
        {
            "mcpServers": {
                "wavecrest": { "command": "uvx", "args": ["wavecrest-mcp"] }
            }
        }
        "#;
        let codex_config = r#"
            [mcp_servers.GitLabMITRE]
            url = "https://gitlab.example.test/api/v4/mcp"
        "#;

        let plan = super::build_sync_plan_from_texts(Some(claude_settings), Some(codex_config))
            .expect("sync plan");

        let wavecrest = plan
            .inventory_rows
            .iter()
            .find(|row| row.server_id == "wavecrest")
            .expect("wavecrest inventory row");
        assert_eq!(wavecrest.claude, super::McpSyncAvailability::Configured);
        assert_eq!(wavecrest.codex, super::McpSyncAvailability::Missing);
        assert!(matches!(
            &wavecrest.opencode,
            super::McpSyncAvailability::Unsupported(message)
                if message.contains("OpenCode MCP sync is not available")
        ));

        let gitlab = plan
            .inventory_rows
            .iter()
            .find(|row| row.server_id == "GitLabMITRE")
            .expect("GitLabMITRE inventory row");
        assert_eq!(gitlab.claude, super::McpSyncAvailability::Missing);
        assert_eq!(gitlab.codex, super::McpSyncAvailability::Configured);
        assert!(matches!(
            &gitlab.opencode,
            super::McpSyncAvailability::Unsupported(message)
                if message.contains("OpenCode MCP sync is not available")
        ));
    }

    #[test]
    fn planner_applies_codex_server_missing_from_claude() {
        let claude_settings = r#"
        {
            "theme": "dark"
        }
        "#;
        let codex_config = r#"
            [mcp_servers.GitLabMITRE]
            url = "https://gitlab.example.test/api/v4/mcp"

            [mcp_servers.GitLabMITRE.headers]
            Authorization = "Bearer token"
        "#;

        let plan = super::build_sync_plan_from_texts(Some(claude_settings), Some(codex_config))
            .expect("sync plan");
        let proposal = plan
            .proposals
            .iter()
            .find(|proposal| {
                proposal.server_id == "GitLabMITRE"
                    && proposal.source == Tool::Codex
                    && proposal.target == Tool::Claude
            })
            .expect("Codex to Claude proposal");

        assert_eq!(
            proposal.preview_lines,
            vec![
                "Will write Claude settings.json".to_string(),
                "  + mcpServers.GitLabMITRE".to_string(),
            ]
        );

        let applied = super::apply_sync_proposal_to_texts(
            proposal,
            Some(claude_settings),
            Some(codex_config),
        )
        .expect("apply proposal");
        let updated_claude = applied.claude_settings.expect("Claude settings updated");
        let value: serde_json::Value = serde_json::from_str(&updated_claude).unwrap();

        assert_eq!(value["theme"], "dark");
        assert_eq!(
            value["mcpServers"]["GitLabMITRE"]["url"],
            "https://gitlab.example.test/api/v4/mcp"
        );
        assert_eq!(
            value["mcpServers"]["GitLabMITRE"]["headers"]["Authorization"],
            "Bearer token"
        );
    }

    #[test]
    fn file_plan_is_preview_only_until_apply_is_called() {
        let dir = tempfile::tempdir().unwrap();
        let claude_path = dir.path().join("claude").join("settings.json");
        let codex_path = dir.path().join("codex").join("config.toml");
        fs::create_dir_all(claude_path.parent().unwrap()).unwrap();
        fs::create_dir_all(codex_path.parent().unwrap()).unwrap();
        fs::write(
            &claude_path,
            r#"{"mcpServers":{"wavecrest":{"command":"uvx","args":["wavecrest-mcp"]}}}"#,
        )
        .unwrap();
        fs::write(&codex_path, r#"model = "gpt-5.5""#).unwrap();
        let paths = super::McpSyncConfigPaths {
            claude_settings: claude_path.clone(),
            codex_config: codex_path.clone(),
        };

        let plan = super::load_sync_plan_from_paths(&paths).expect("sync plan");

        assert_eq!(
            fs::read_to_string(&codex_path).unwrap(),
            r#"model = "gpt-5.5""#
        );
        let proposal = plan
            .proposals
            .iter()
            .find(|proposal| proposal.server_id == "wavecrest" && proposal.target == Tool::Codex)
            .expect("Codex proposal");

        super::apply_sync_proposal_to_paths(proposal, &paths).expect("apply proposal");

        let codex_config = fs::read_to_string(&codex_path).unwrap();
        assert!(codex_config.contains(r#"model = "gpt-5.5""#));
        assert!(codex_config.contains("[mcp_servers.wavecrest]"));
    }
}
