#[derive(Debug, Clone, PartialEq)]
pub struct CompletionResult {
    pub completed: String,
    pub candidates: Vec<String>,
}

pub fn complete_path(input: &str) -> CompletionResult {
    let expanded = expand_tilde(input);

    let (parent, prefix) = if expanded.ends_with('/') {
        (expanded.as_str(), "")
    } else {
        match expanded.rfind('/') {
            Some(pos) => (&expanded[..=pos], &expanded[pos + 1..]),
            None => {
                return CompletionResult {
                    completed: input.to_string(),
                    candidates: Vec::new(),
                }
            }
        }
    };

    let entries = match std::fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(_) => {
            return CompletionResult {
                completed: input.to_string(),
                candidates: Vec::new(),
            }
        }
    };

    let mut candidates: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type()
                .map(|ft| ft.is_dir() || ft.is_symlink())
                .unwrap_or(false)
        })
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with(prefix) {
                if e.file_type().map(|ft| ft.is_symlink()).unwrap_or(false) && !e.path().is_dir() {
                    return None;
                }
                Some(name)
            } else {
                None
            }
        })
        .collect();

    candidates.sort();

    if candidates.is_empty() {
        return CompletionResult {
            completed: input.to_string(),
            candidates: Vec::new(),
        };
    }

    let completed = if candidates.len() == 1 {
        format!("{}{}/", parent, candidates[0])
    } else {
        let lcp = longest_common_prefix(&candidates);
        format!("{}{}", parent, lcp)
    };

    CompletionResult {
        completed,
        candidates,
    }
}

fn expand_tilde(input: &str) -> String {
    if let Some(rest) = input.strip_prefix('~') {
        if let Some(home) = dirs::home_dir() {
            return format!("{}{}", home.display(), rest);
        }
    }
    input.to_string()
}

fn longest_common_prefix(strings: &[String]) -> String {
    if strings.is_empty() {
        return String::new();
    }
    let first = &strings[0];
    let mut len = first.len();
    for s in &strings[1..] {
        len = len.min(s.len());
        for (i, (a, b)) in first.chars().zip(s.chars()).enumerate() {
            if a != b {
                len = len.min(i);
                break;
            }
        }
    }
    first[..len].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_expand_tilde_replaces_home() {
        let home = dirs::home_dir().unwrap().to_string_lossy().to_string();
        let result = expand_tilde("~/projects");
        assert_eq!(result, format!("{}/projects", home));
    }

    #[test]
    fn test_expand_tilde_standalone() {
        let home = dirs::home_dir().unwrap().to_string_lossy().to_string();
        let result = expand_tilde("~");
        assert_eq!(result, home);
    }

    #[test]
    fn test_expand_tilde_no_tilde() {
        let result = expand_tilde("/tmp/foo");
        assert_eq!(result, "/tmp/foo");
    }

    #[test]
    fn test_longest_common_prefix_single() {
        let result = longest_common_prefix(&["hello".to_string()]);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_longest_common_prefix_multiple() {
        let result = longest_common_prefix(&[
            "projects".to_string(),
            "productivity".to_string(),
            "promises".to_string(),
        ]);
        assert_eq!(result, "pro");
    }

    #[test]
    fn test_longest_common_prefix_identical() {
        let result = longest_common_prefix(&["abc".to_string(), "abc".to_string()]);
        assert_eq!(result, "abc");
    }

    #[test]
    fn test_longest_common_prefix_empty_slice() {
        let result = longest_common_prefix(&[]);
        assert_eq!(result, "");
    }

    #[test]
    fn test_complete_path_single_match() {
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("projects");
        fs::create_dir(&sub).unwrap();
        fs::create_dir(dir.path().join("other")).unwrap();

        let input = format!("{}/proj", dir.path().display());
        let result = complete_path(&input);

        assert_eq!(
            result.completed,
            format!("{}/projects/", dir.path().display())
        );
        assert_eq!(result.candidates, vec!["projects"]);
    }

    #[test]
    fn test_complete_path_multiple_matches() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("projects")).unwrap();
        fs::create_dir(dir.path().join("productivity")).unwrap();
        fs::create_dir(dir.path().join("other")).unwrap();

        let input = format!("{}/pro", dir.path().display());
        let result = complete_path(&input);

        assert_eq!(result.completed, format!("{}/pro", dir.path().display()));
        assert_eq!(result.candidates.len(), 2);
        assert!(result.candidates.contains(&"projects".to_string()));
        assert!(result.candidates.contains(&"productivity".to_string()));
    }

    #[test]
    fn test_complete_path_no_matches() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("alpha")).unwrap();

        let input = format!("{}/zzz", dir.path().display());
        let result = complete_path(&input);

        assert_eq!(result.completed, input);
        assert!(result.candidates.is_empty());
    }

    #[test]
    fn test_complete_path_trailing_slash_lists_all() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join("alpha")).unwrap();
        fs::create_dir(dir.path().join("beta")).unwrap();
        fs::write(dir.path().join("file.txt"), "hi").unwrap();

        let input = format!("{}/", dir.path().display());
        let result = complete_path(&input);

        assert_eq!(result.candidates.len(), 2);
        assert!(result.candidates.contains(&"alpha".to_string()));
        assert!(result.candidates.contains(&"beta".to_string()));
    }

    #[test]
    fn test_complete_path_nonexistent_parent() {
        let result = complete_path("/tmp/definitely_does_not_exist_xyz123/sub");
        assert!(result.candidates.is_empty());
        assert_eq!(
            result.completed,
            "/tmp/definitely_does_not_exist_xyz123/sub"
        );
    }

    #[test]
    fn test_complete_path_hidden_dirs_included() {
        let dir = TempDir::new().unwrap();
        fs::create_dir(dir.path().join(".hidden")).unwrap();
        fs::create_dir(dir.path().join(".config")).unwrap();

        let input = format!("{}/.", dir.path().display());
        let result = complete_path(&input);

        assert_eq!(result.candidates.len(), 2);
        assert!(result.candidates.contains(&".hidden".to_string()));
        assert!(result.candidates.contains(&".config".to_string()));
    }

    #[test]
    fn test_complete_path_tilde_expansion() {
        let home = dirs::home_dir().unwrap();
        let result = complete_path("~/");
        assert!(result
            .completed
            .starts_with(&home.to_string_lossy().to_string()));
    }
}
