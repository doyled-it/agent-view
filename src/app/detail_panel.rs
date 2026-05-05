#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DetailPanelMode {
    None,
    Preview,
    Metadata,
    Both,
}

impl DetailPanelMode {
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Preview,
            Self::Preview => Self::Metadata,
            Self::Metadata => Self::Both,
            Self::Both => Self::None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "Off",
            Self::Preview => "Preview",
            Self::Metadata => "Details",
            Self::Both => "Both",
        }
    }

    pub fn shows_preview(self) -> bool {
        matches!(self, Self::Preview | Self::Both)
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "none" => Self::None,
            "preview" => Self::Preview,
            "both" => Self::Both,
            _ => Self::Metadata,
        }
    }

    pub fn as_config_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Preview => "preview",
            Self::Metadata => "metadata",
            Self::Both => "both",
        }
    }
}
