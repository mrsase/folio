//! Small shared helpers: fuzzy matching for the quick-open palette and text
//! formatting used in the chrome.

/// A fuzzy subsequence match with a ranking score and matched byte positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FuzzyMatch {
    pub score: i32,
    /// Indices (in `char` units) of the candidate that matched, for emphasis.
    pub positions: Vec<usize>,
}

/// Case-insensitive subsequence match that rewards prefix, segment starts and
/// consecutive runs. Returns `None` when the query is not a subsequence.
///
/// Deliberately allocation-free per candidate apart from the matched positions:
/// the palette ranks thousands of paths per keystroke.
pub fn fuzzy_match(query: &str, candidate: &str) -> Option<FuzzyMatch> {
    if query.is_empty() {
        return Some(FuzzyMatch {
            score: 0,
            positions: Vec::new(),
        });
    }
    let query: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).collect();
    let candidate_chars: Vec<char> = candidate.chars().collect();
    let mut positions = Vec::with_capacity(query.len());
    let mut score = 0;
    let mut query_index = 0;
    let mut last_match: Option<usize> = None;

    for (index, raw) in candidate_chars.iter().enumerate() {
        if query_index >= query.len() {
            break;
        }
        let mut lower = raw.to_lowercase();
        let Some(ch) = lower.next() else {
            continue;
        };
        if ch != query[query_index] {
            continue;
        }
        // Only pay for the segment-boundary check when it can matter.
        let boundary = index == 0
            || matches!(
                candidate_chars[index - 1],
                '/' | '\\' | '_' | '-' | '.' | ' ' | ':' | '(' | '['
            );
        score += 16;
        if boundary {
            score += 24;
        }
        if last_match == Some(index.wrapping_sub(1)) {
            score += 12;
        }
        if index == 0 {
            score += 40;
        }
        // Matching the case exactly is a mild hint that this is the intended file.
        if *raw == query[query_index] {
            score += 4;
        }
        positions.push(index);
        last_match = Some(index);
        query_index += 1;
    }

    if query_index == query.len() {
        // Shorter candidates make for tighter matches.
        score -= (candidate_chars.len() as i32).min(200) / 4;
        Some(FuzzyMatch { score, positions })
    } else {
        None
    }
}

/// Compact a byte count for display (`1.4 MB`).
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{:.1} {}", value, UNITS[unit])
    }
}

/// Shorten a path for a tab label: the file name, with a parent hint when two
/// open tabs share the name.
pub fn tab_label(path: &std::path::Path, ambiguous: bool) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string());
    if !ambiguous {
        return name;
    }
    let parent = path
        .parent()
        .and_then(|parent| parent.file_name())
        .map(|name| name.to_string_lossy().to_string());
    match parent {
        Some(parent) => format!("{parent}/{name}"),
        None => name,
    }
}

/// Strip CLI/GUI noise from a path for display: collapse the home directory.
pub fn display_path(path: &std::path::Path) -> String {
    // macOS reports canonical paths under `/private` for the well-known
    // symlinked roots; people type and read the short form.
    let mut short = path.to_path_buf();
    if let Ok(rest) = path.strip_prefix("/private") {
        let first = rest
            .components()
            .next()
            .and_then(|component| component.as_os_str().to_str())
            .unwrap_or_default();
        if matches!(first, "tmp" | "var" | "etc") {
            short = std::path::Path::new("/").join(rest);
        }
    }
    if let Some(home) = std::env::var_os("HOME")
        && let Ok(rest) = short.strip_prefix(&home)
    {
        return format!("~/{rest}", rest = rest.display());
    }
    short.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subsequence_matching_scores_prefixes_higher() {
        let prefix = fuzzy_match("read", "reading.md").unwrap();
        let scattered = fuzzy_match("read", "re-arranged-doc.md").unwrap();
        assert!(prefix.score > scattered.score);

        assert!(fuzzy_match("rdme", "README.md").is_some());
        assert!(fuzzy_match("xyz", "reading.md").is_none());
        assert!(fuzzy_match("", "anything").is_some());
    }

    #[test]
    fn matching_is_case_insensitive_and_reports_positions() {
        let matched = fuzzy_match("gu", "Guide.md").unwrap();
        assert_eq!(matched.positions, vec![0, 1]);
    }

    #[test]
    fn segment_boundaries_beat_mid_word_hits() {
        let boundary = fuzzy_match("gc", "src/git/config.md").unwrap();
        let mid_word = fuzzy_match("gc", "logging.config.md").unwrap();
        assert!(boundary.score > mid_word.score);
    }

    #[test]
    fn duplicate_names_get_parent_hints() {
        let label = tab_label(std::path::Path::new("/a/notes/README.md"), true);
        assert_eq!(label, "notes/README.md");
        let label = tab_label(std::path::Path::new("/a/notes/README.md"), false);
        assert_eq!(label, "README.md");
    }

    #[test]
    fn canonical_private_paths_display_short() {
        assert_eq!(
            display_path(std::path::Path::new("/private/tmp/notes.md")),
            "/tmp/notes.md"
        );
        assert_eq!(
            display_path(std::path::Path::new("/private/var/folders/x")),
            "/var/folders/x"
        );
    }

    #[test]
    fn byte_sizes_are_readable() {
        assert_eq!(human_bytes(900), "900 B");
        assert_eq!(human_bytes(2048), "2.0 KB");
    }
}
