#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSyncForm {
    pub paths: crate::core::mcp::McpSyncConfigPaths,
    pub plan: crate::core::mcp::McpSyncPlan,
    pub selected: usize,
    pub confirming: bool,
    pub confirming_all: bool,
}

impl McpSyncForm {
    pub fn new(
        paths: crate::core::mcp::McpSyncConfigPaths,
        plan: crate::core::mcp::McpSyncPlan,
    ) -> Self {
        Self {
            paths,
            plan,
            selected: 0,
            confirming: false,
            confirming_all: false,
        }
    }

    pub fn selected_proposal(&self) -> Option<&crate::core::mcp::McpSyncProposal> {
        if self.selected_all_proposals() {
            return None;
        }

        let proposal_idx = if self.has_all_proposals_action() {
            self.selected.checked_sub(1)?
        } else {
            self.selected
        };
        self.plan.proposals.get(proposal_idx)
    }

    pub fn selected_all_proposals(&self) -> bool {
        self.has_all_proposals_action() && self.selected == 0
    }

    pub fn has_all_proposals_action(&self) -> bool {
        self.plan.proposals.len() > 1
    }

    pub fn action_count(&self) -> usize {
        self.plan.proposals.len() + usize::from(self.has_all_proposals_action())
    }

    pub fn all_proposals(&self) -> Vec<crate::core::mcp::McpSyncProposal> {
        self.plan.proposals.clone()
    }

    pub fn begin_confirming_selected(&mut self) {
        if self.selected_all_proposals() {
            self.confirming_all = true;
            self.confirming = false;
        } else if self.selected_proposal().is_some() {
            self.confirming = true;
            self.confirming_all = false;
        }
    }

    pub fn clear_confirmation(&mut self) {
        self.confirming = false;
        self.confirming_all = false;
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
        self.clear_confirmation();
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.action_count() {
            self.selected += 1;
        }
        self.clear_confirmation();
    }

    pub fn replace_plan(&mut self, plan: crate::core::mcp::McpSyncPlan) {
        self.plan = plan;
        let action_count = self.action_count();
        if action_count == 0 {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(action_count - 1);
        }
        self.clear_confirmation();
    }
}
