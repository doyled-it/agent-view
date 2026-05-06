use std::collections::HashSet;

#[derive(Default)]
pub struct BulkSelection {
    pub selected: HashSet<String>,
}

impl BulkSelection {
    pub fn new() -> Self {
        Self::default()
    }
}
