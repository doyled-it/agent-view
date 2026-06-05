use crate::core::mcp::{McpProfile, McpSelection};

#[allow(dead_code)]
pub fn apply_profile(profiles: &[McpProfile], id: &str) -> Option<McpSelection> {
    profiles
        .iter()
        .find(|profile| profile.id == id)
        .map(|profile| {
            let mut selection = profile.selection.clone();
            selection.profile_id = Some(profile.id.clone());
            selection
        })
}

#[allow(dead_code)]
pub fn upsert_profile(profiles: &mut Vec<McpProfile>, profile: McpProfile) {
    if let Some(existing) = profiles
        .iter_mut()
        .find(|existing| existing.id == profile.id)
    {
        *existing = profile;
    } else {
        profiles.push(profile);
    }
}

#[allow(dead_code)]
pub fn delete_profile(profiles: &mut Vec<McpProfile>, id: &str) -> bool {
    let before = profiles.len();
    profiles.retain(|profile| profile.id != id);
    profiles.len() != before
}

#[cfg(test)]
mod tests {
    use crate::core::mcp::profiles::{apply_profile, delete_profile, upsert_profile};
    use crate::core::mcp::{McpProfile, McpSelection, McpServerSelection};

    #[test]
    fn apply_profile_returns_resolved_selection() {
        let profile = McpProfile {
            id: "minimal".into(),
            name: "Minimal".into(),
            selection: McpSelection {
                profile_id: Some("minimal".into()),
                servers: vec![McpServerSelection {
                    id: "browser".into(),
                    enabled: false,
                    selected_tools: None,
                }],
            },
        };
        let resolved = apply_profile(&[profile], "minimal").unwrap();
        assert_eq!(resolved.profile_id.as_deref(), Some("minimal"));
        assert_eq!(resolved.servers[0].id, "browser");
    }

    #[test]
    fn apply_profile_stamps_outer_id_as_active_profile() {
        let profiles = vec![
            McpProfile {
                id: "rust".into(),
                name: "Rust".into(),
                selection: McpSelection {
                    profile_id: None,
                    servers: Vec::new(),
                },
            },
            McpProfile {
                id: "docs".into(),
                name: "Docs".into(),
                selection: McpSelection {
                    profile_id: Some("other".into()),
                    servers: Vec::new(),
                },
            },
        ];

        let rust = apply_profile(&profiles, "rust").unwrap();
        let docs = apply_profile(&profiles, "docs").unwrap();

        assert_eq!(rust.profile_id.as_deref(), Some("rust"));
        assert_eq!(docs.profile_id.as_deref(), Some("docs"));
    }

    #[test]
    fn upsert_profile_replaces_same_id() {
        let mut profiles = vec![McpProfile {
            id: "rust".into(),
            name: "Rust old".into(),
            selection: McpSelection::default(),
        }];
        upsert_profile(
            &mut profiles,
            McpProfile {
                id: "rust".into(),
                name: "Rust".into(),
                selection: McpSelection::default(),
            },
        );
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].name, "Rust");
    }

    #[test]
    fn delete_profile_removes_by_id() {
        let mut profiles = vec![McpProfile {
            id: "docs".into(),
            name: "Docs".into(),
            selection: McpSelection::default(),
        }];
        assert!(delete_profile(&mut profiles, "docs"));
        assert!(profiles.is_empty());
    }
}
