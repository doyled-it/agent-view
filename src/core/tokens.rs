//! Token counting from Claude Code output

use lazy_static::lazy_static;
use regex::Regex;

lazy_static! {
    static ref TOKEN_PATTERN: Regex = Regex::new(r"(\d+(?:\.\d+)?)\s*([kKmM])?\s*tokens?").unwrap();
}

pub fn parse_token_count(text: &str) -> Option<i64> {
    let caps = TOKEN_PATTERN.captures(text)?;
    let num: f64 = caps.get(1)?.as_str().parse().ok()?;
    let multiplier = match caps.get(2).map(|m| m.as_str()) {
        Some("k") | Some("K") => 1_000.0,
        Some("m") | Some("M") => 1_000_000.0,
        _ => 1.0,
    };
    Some((num * multiplier) as i64)
}

/// Parse the last occurrence of a token count from pane output.
pub fn extract_latest_tokens(output: &str) -> Option<i64> {
    output
        .lines()
        .rev()
        .take(50)
        .filter_map(parse_token_count)
        .next()
}

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
    fn test_parse_token_count_k() {
        assert_eq!(parse_token_count("↓ 20.4k tokens"), Some(20400));
    }

    #[test]
    fn test_parse_token_count_m() {
        assert_eq!(parse_token_count("... 1.2M tokens"), Some(1200000));
    }

    #[test]
    fn test_parse_token_count_plain() {
        assert_eq!(parse_token_count("500 tokens"), Some(500));
    }

    #[test]
    fn test_parse_no_tokens() {
        assert_eq!(parse_token_count("hello world"), None);
    }

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

    #[test]
    fn test_extract_latest_tokens() {
        let output = "Some output\n↓ 10k tokens\nMore stuff\n↓ 20.4k tokens\nfinal line";
        assert_eq!(extract_latest_tokens(output), Some(20400));
    }

    #[test]
    fn test_extract_latest_tokens_none() {
        let output = "no token info here\njust normal output";
        assert_eq!(extract_latest_tokens(output), None);
    }
}
