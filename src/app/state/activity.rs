use std::collections::VecDeque;

use crate::types::ActivityEvent;

pub struct ActivityState {
    pub feed: VecDeque<ActivityEvent>,
    pub show_feed: bool,
}

impl ActivityState {
    pub fn new() -> Self {
        Self {
            feed: VecDeque::new(),
            show_feed: true,
        }
    }
}

impl Default for ActivityState {
    fn default() -> Self {
        Self::new()
    }
}
