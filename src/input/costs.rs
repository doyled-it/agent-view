//! Key handler for the Costs tab. Left/Right cycle CostPeriod; everything
//! else falls through to the global keymap.

use crate::app::App;
use crate::core::cost::CostPeriod;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Returns true when the key was fully handled here and should NOT
/// propagate to the global keymap.
pub fn handle_costs_key(app: &mut App, key: KeyEvent) -> bool {
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Left) | (KeyModifiers::NONE, KeyCode::Char('h')) => {
            app.cost_period = app.cost_period.prev();
            true
        }
        (KeyModifiers::NONE, KeyCode::Right) | (KeyModifiers::NONE, KeyCode::Char('l')) => {
            app.cost_period = app.cost_period.next();
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    fn make_app() -> App {
        App::new(false)
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn right_advances_period() {
        let mut app = make_app();
        app.cost_period = CostPeriod::Today;
        assert!(handle_costs_key(&mut app, key(KeyCode::Right)));
        assert_eq!(app.cost_period, CostPeriod::Week);
    }

    #[test]
    fn left_goes_back() {
        let mut app = make_app();
        app.cost_period = CostPeriod::Today;
        assert!(handle_costs_key(&mut app, key(KeyCode::Left)));
        assert_eq!(app.cost_period, CostPeriod::AllTime);
    }

    #[test]
    fn h_and_l_alias_arrow_keys() {
        let mut app = make_app();
        app.cost_period = CostPeriod::Today;
        assert!(handle_costs_key(&mut app, key(KeyCode::Char('l'))));
        assert_eq!(app.cost_period, CostPeriod::Week);
        assert!(handle_costs_key(&mut app, key(KeyCode::Char('h'))));
        assert_eq!(app.cost_period, CostPeriod::Today);
    }

    #[test]
    fn unrelated_key_is_not_consumed() {
        let mut app = make_app();
        assert!(!handle_costs_key(&mut app, key(KeyCode::Char('q'))));
    }
}
