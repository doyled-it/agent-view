use std::time::Instant;

#[derive(Default)]
pub struct PreviewState {
    pub content: String,
    pub last_session: Option<String>,
    pub last_capture: Option<Instant>,
}

impl PreviewState {
    pub fn new() -> Self {
        Self::default()
    }
}
