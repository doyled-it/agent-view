use crate::core::status_page::SharedStatusData;
use crate::types::StatusPageData;

#[derive(Default)]
pub struct StatusPageState {
    pub data: Option<StatusPageData>,
    pub shared: Option<SharedStatusData>,
}

impl StatusPageState {
    pub fn new() -> Self {
        Self::default()
    }
}
