use std::time::Instant;

#[derive(Default)]
pub struct ToastState {
    pub message: Option<String>,
    pub expire: Option<Instant>,
}

impl ToastState {
    pub fn new() -> Self {
        Self::default()
    }
}
