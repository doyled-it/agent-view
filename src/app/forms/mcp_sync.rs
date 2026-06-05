#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSyncForm {
    pub paths: crate::core::mcp::McpSyncConfigPaths,
    pub plan: crate::core::mcp::McpSyncPlan,
    pub selected: usize,
    pub confirming: bool,
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
        }
    }

    pub fn selected_proposal(&self) -> Option<&crate::core::mcp::McpSyncProposal> {
        self.plan.proposals.get(self.selected)
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
        self.confirming = false;
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.plan.proposals.len() {
            self.selected += 1;
        }
        self.confirming = false;
    }

    pub fn replace_plan(&mut self, plan: crate::core::mcp::McpSyncPlan) {
        self.plan = plan;
        if self.plan.proposals.is_empty() {
            self.selected = 0;
        } else {
            self.selected = self.selected.min(self.plan.proposals.len() - 1);
        }
        self.confirming = false;
    }
}
