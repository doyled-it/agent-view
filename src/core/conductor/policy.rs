use crate::types::{ConductorActionRequest, ConductorActionType, ConductorConfig};

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum PolicyDecision {
    Allowed,
    Blocked(String),
}

#[allow(dead_code)]
pub fn validate_action(
    config: &ConductorConfig,
    action: &ConductorActionRequest,
    current_child_count: usize,
) -> PolicyDecision {
    if !config.enabled {
        return PolicyDecision::Blocked("conductor is disabled".to_string());
    }

    let max_children = usize::try_from(config.max_children).unwrap_or(0);

    match action.action_type {
        ConductorActionType::SpawnChild if !config.allow_spawn_child => {
            PolicyDecision::Blocked("spawn_child is disabled".to_string())
        }
        ConductorActionType::SpawnChild if current_child_count >= max_children => {
            PolicyDecision::Blocked("max child sessions reached".to_string())
        }
        ConductorActionType::SendChildResponse if !config.allow_send_child_response => {
            PolicyDecision::Blocked("send_child_response is disabled".to_string())
        }
        _ => PolicyDecision::Allowed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn request(action_type: ConductorActionType) -> ConductorActionRequest {
        ConductorActionRequest {
            action_type,
            child_session_id: None,
            payload: json!({}),
        }
    }

    #[test]
    fn blocks_send_child_response_when_policy_disallows_it() {
        let mut config = ConductorConfig::default_for_session("conductor-1".to_string());
        config.allow_send_child_response = false;
        let action = request(ConductorActionType::SendChildResponse);

        let decision = validate_action(&config, &action, 0_usize);

        assert_eq!(
            decision,
            PolicyDecision::Blocked("send_child_response is disabled".to_string())
        );
    }

    #[test]
    fn allows_record_child_summary() {
        let config = ConductorConfig::default_for_session("conductor-1".to_string());
        let action = request(ConductorActionType::RecordChildSummary);

        let decision = validate_action(&config, &action, 0_usize);

        assert_eq!(decision, PolicyDecision::Allowed);
    }

    #[test]
    fn blocks_spawn_child_when_policy_disallows_it() {
        let mut config = ConductorConfig::default_for_session("conductor-1".to_string());
        config.allow_spawn_child = false;
        let action = request(ConductorActionType::SpawnChild);

        let decision = validate_action(&config, &action, 0_usize);

        assert_eq!(
            decision,
            PolicyDecision::Blocked("spawn_child is disabled".to_string())
        );
    }

    #[test]
    fn allows_spawn_child_below_max_children() {
        let mut config = ConductorConfig::default_for_session("conductor-1".to_string());
        config.max_children = 2;
        let action = request(ConductorActionType::SpawnChild);

        let decision = validate_action(&config, &action, 1_usize);

        assert_eq!(decision, PolicyDecision::Allowed);
    }

    #[test]
    fn blocks_spawn_child_when_max_children_reached() {
        let mut config = ConductorConfig::default_for_session("conductor-1".to_string());
        config.max_children = 2;
        let action = request(ConductorActionType::SpawnChild);

        let decision = validate_action(&config, &action, 2_usize);

        assert_eq!(
            decision,
            PolicyDecision::Blocked("max child sessions reached".to_string())
        );
    }

    #[test]
    fn blocks_spawn_child_when_current_children_exceed_max() {
        let mut config = ConductorConfig::default_for_session("conductor-1".to_string());
        config.max_children = 2;
        let action = request(ConductorActionType::SpawnChild);

        let decision = validate_action(&config, &action, 3_usize);

        assert_eq!(
            decision,
            PolicyDecision::Blocked("max child sessions reached".to_string())
        );
    }

    #[test]
    fn blocks_spawn_child_when_max_children_is_negative() {
        let mut config = ConductorConfig::default_for_session("conductor-1".to_string());
        config.max_children = -1;
        let action = request(ConductorActionType::SpawnChild);

        let decision = validate_action(&config, &action, 0_usize);

        assert_eq!(
            decision,
            PolicyDecision::Blocked("max child sessions reached".to_string())
        );
    }

    #[test]
    fn blocks_actions_when_conductor_is_disabled() {
        let mut config = ConductorConfig::default_for_session("conductor-1".to_string());
        config.enabled = false;
        let action = request(ConductorActionType::RecordChildSummary);

        let decision = validate_action(&config, &action, 0_usize);

        assert_eq!(
            decision,
            PolicyDecision::Blocked("conductor is disabled".to_string())
        );
    }
}
