pub mod catalog;
pub mod profiles;
pub mod sync;
pub mod types;

#[allow(unused_imports)]
pub use catalog::{
    discover_mcp_server_catalog, parse_claude_mcp_servers, parse_codex_mcp_servers,
    resolve_selection_against_catalog, McpServerCatalogEntry, ResolvedMcpSelection,
};
#[allow(unused_imports)]
pub use sync::{
    apply_sync_proposal_to_paths, default_sync_config_paths, load_sync_plan_from_paths,
    sync_all_missing_mcp_servers_from_paths, McpSyncAvailability, McpSyncConfigPaths,
    McpSyncInventoryRow, McpSyncPlan, McpSyncProposal,
};
#[allow(unused_imports)]
pub use types::{McpProfile, McpSelection, McpServerSelection};
