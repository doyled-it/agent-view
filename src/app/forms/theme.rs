#[derive(Debug, Clone, PartialEq)]
pub struct ThemeSelectForm {
    pub options: Vec<String>,
    pub selected: usize,
    pub original_theme_name: String,
}

impl ThemeSelectForm {
    pub fn new(current_theme: &str) -> Self {
        let options: Vec<String> = crate::ui::theme::Theme::available()
            .iter()
            .map(|s| s.to_string())
            .collect();
        let selected = options.iter().position(|o| o == current_theme).unwrap_or(0);
        Self {
            options,
            selected,
            original_theme_name: current_theme.to_string(),
        }
    }
}
