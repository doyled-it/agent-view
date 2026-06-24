pub fn build_startup_prompt(conductor_session_id: &str) -> String {
    format!(
        r#"You are an Agent View conductor session.

Do not use runner-native background agents for durable child work; use Agent View child sessions so the user can see, enter, answer, and clean them up.

Queue conductor actions from the terminal with:
agent-view conductor-action {conductor_session_id} '<request-json>'

Spawn child example:
agent-view conductor-action {conductor_session_id} '{{"action_type":"spawn_child","payload":{{"title":"Short task name","prompt":"Task instructions"}}}}'

Other action_type values: mark_child_needs_user, send_child_response, record_child_summary.

Use runner-native subagents only for throwaway reasoning inside this conversation. Durable child work must be an Agent View child session.
"#
    )
}

pub fn send_startup_prompt(tmux_session: &str, conductor_session_id: &str) -> Result<(), String> {
    let prompt = build_startup_prompt(conductor_session_id);

    #[cfg(test)]
    if crate::core::session::test_support::should_skip_tmux_create() {
        crate::core::session::test_support::record_sent_keys(tmux_session, &prompt);
        return Ok(());
    }

    crate::core::tmux::send_keys(tmux_session, &prompt).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn startup_prompt_names_agent_view_child_action_path() {
        let prompt = super::build_startup_prompt("conductor-1");

        assert!(prompt.contains("Do not use runner-native background agents"));
        assert!(prompt.contains("agent-view conductor-action conductor-1"));
        assert!(prompt.contains(r#""action_type":"spawn_child""#));
        assert!(prompt.contains("mark_child_needs_user"));
        assert!(prompt.contains("record_child_summary"));
    }
}
