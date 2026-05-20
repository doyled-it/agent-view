//! Token display helpers.

/// Format a token count for display: "45.2k", "1.2M", etc.
pub fn format_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}k", tokens as f64 / 1_000.0)
    } else {
        format!("{}", tokens)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_tokens_millions() {
        assert_eq!(format_tokens(1_200_000), "1.2M");
    }

    #[test]
    fn test_format_tokens_thousands() {
        assert_eq!(format_tokens(45_200), "45.2k");
    }

    #[test]
    fn test_format_tokens_small() {
        assert_eq!(format_tokens(500), "500");
    }
}
