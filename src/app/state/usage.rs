use crate::core::usage::SharedUsageData;
use crate::types::UsageData;

#[derive(Default)]
pub struct UsageState {
    pub data: Option<UsageData>,
    pub shared: Option<SharedUsageData>,
}

impl UsageState {
    pub fn new() -> Self {
        Self::default()
    }
}
