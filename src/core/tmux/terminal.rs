use std::process::Command;

const DIM_OVERRIDE_INDEX: &str = "terminal-overrides[999]";

fn boxed_text_style_override() -> &'static str {
    "*:dim=\\E[38;5;245m:sitm@"
}

pub(super) struct TextStyleGuard {
    previous_override: Option<String>,
}

pub(super) fn normalize_text_styles_for_attached_client() -> TextStyleGuard {
    let previous_override = show_tmux_option(DIM_OVERRIDE_INDEX);
    let _ = Command::new("tmux")
        .args([
            "set-option",
            "-g",
            DIM_OVERRIDE_INDEX,
            boxed_text_style_override(),
        ])
        .status();

    TextStyleGuard { previous_override }
}

impl Drop for TextStyleGuard {
    fn drop(&mut self) {
        match self.previous_override.as_deref() {
            Some(previous) => {
                let _ = Command::new("tmux")
                    .args(["set-option", "-g", DIM_OVERRIDE_INDEX, previous])
                    .status();
            }
            None => {
                let _ = Command::new("tmux")
                    .args(["set-option", "-gu", DIM_OVERRIDE_INDEX])
                    .status();
            }
        }
    }
}

fn show_tmux_option(option: &str) -> Option<String> {
    let output = Command::new("tmux")
        .args(["show-option", "-gqv", option])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    Some(
        String::from_utf8_lossy(&output.stdout)
            .trim_end_matches(['\r', '\n'])
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boxed_text_style_override_maps_dim_to_muted_foreground() {
        let override_value = boxed_text_style_override();

        assert!(override_value.contains("dim=\\E[38;5;245m"));
        assert!(!override_value.contains("dim@"));
    }

    #[test]
    fn boxed_text_style_override_removes_italic_capability() {
        assert!(boxed_text_style_override().contains("sitm@"));
    }

    #[test]
    fn boxed_text_style_override_uses_reserved_override_slot() {
        assert_eq!(DIM_OVERRIDE_INDEX, "terminal-overrides[999]");
    }
}
