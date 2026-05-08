//! Session module — lifecycle ops, status processor, crash detection, title generation.

mod crash;
pub mod hooks;
mod ops;
mod processor;

pub use crash::detect_crashed_statuses;
pub use ops::SessionOps;
pub use processor::StatusProcessor;

// Name generation word lists
const ADJECTIVES: &[&str] = &[
    "swift", "bright", "calm", "deep", "eager", "fair", "gentle", "happy", "keen", "light", "mild",
    "noble", "proud", "quick", "rich", "safe", "true", "vivid", "warm", "wise", "bold", "cool",
    "dark", "fast",
];

const NOUNS: &[&str] = &[
    "fox", "owl", "wolf", "bear", "hawk", "lion", "deer", "crow", "dove", "seal", "swan", "hare",
    "lynx", "moth", "newt", "orca", "pike", "rook", "toad", "vole", "wren", "yak", "bass", "crab",
];

pub(super) fn generate_title() -> String {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as usize;
    let adj = ADJECTIVES[nanos % ADJECTIVES.len()];
    let noun = NOUNS[(nanos / ADJECTIVES.len()) % NOUNS.len()];
    format!("{}-{}", adj, noun)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_title_format() {
        let title = generate_title();
        let parts: Vec<&str> = title.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert!(ADJECTIVES.contains(&parts[0]));
        assert!(NOUNS.contains(&parts[1]));
    }
}
