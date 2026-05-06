use std::collections::HashMap;

use crate::types::{Routine, RoutineRun};

use super::super::RoutineListRow;

#[derive(Default)]
pub struct RoutineState {
    pub routines: Vec<Routine>,
    pub runs_cache: HashMap<String, Vec<RoutineRun>>,
    pub list_rows: Vec<RoutineListRow>,
    pub selected_index: usize,
    pub tab_warning_shown: bool,
}

impl RoutineState {
    pub fn new() -> Self {
        Self::default()
    }
}
