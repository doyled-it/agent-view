#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleFrequency {
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Yearly,
    Advanced,
}

impl ScheduleFrequency {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Hourly => "Hourly",
            Self::Daily => "Daily",
            Self::Weekly => "Weekly",
            Self::Monthly => "Monthly",
            Self::Yearly => "Yearly",
            Self::Advanced => "Advanced",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            Self::Hourly => Self::Daily,
            Self::Daily => Self::Weekly,
            Self::Weekly => Self::Monthly,
            Self::Monthly => Self::Yearly,
            Self::Yearly => Self::Advanced,
            Self::Advanced => Self::Hourly,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            Self::Hourly => Self::Advanced,
            Self::Daily => Self::Hourly,
            Self::Weekly => Self::Daily,
            Self::Monthly => Self::Weekly,
            Self::Yearly => Self::Monthly,
            Self::Advanced => Self::Yearly,
        }
    }
}
