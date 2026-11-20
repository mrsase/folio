//! Display slicing and in-document search, independent of GPUI.

use std::ops::Range;

/// Longest prefix of a document handed to the layout engine.
///
/// This is the proof of concept's measured cap: 256 KB still paints promptly,
/// while larger documents stall layout and retain hundreds of megabytes. The
/// full source is kept separately for writes.
pub const MAX_DISPLAY_BYTES: usize = 256 * 1024;

/// Cut the source at a block boundary for display. A dangling fence is closed
/// so the tail does not turn into one giant code block.
pub fn slice_for_display(source: &str) -> (String, bool) {
    if source.len() <= MAX_DISPLAY_BYTES {
        return (source.to_string(), false);
    }
    let mut limit = MAX_DISPLAY_BYTES;
    while limit > 0 && !source.is_char_boundary(limit) {
        limit -= 1;
    }
    let head = &source[..limit];
    let cut = head.rfind("\n\n").unwrap_or(limit);
    let mut display = head[..cut].trim_end().to_string();
    if display.matches("```").count() % 2 == 1 {
        display.push_str("\n```");
    }
    let total_kb = source.len() / 1024;
    let shown_kb = display.len() / 1024;
    display.push_str(&format!(
        "\n\n---\n\n*Showing the first {shown_kb} KB of {total_kb} KB — the rest is not rendered.*\n"
    ));
    (display, true)
}

/// Case-insensitive substring search over rendered text.
#[derive(Debug, Default, Clone)]
pub struct Find {
    pub query: String,
    pub matches: Vec<Range<usize>>,
    pub current: usize,
}

impl Find {
    pub fn search(&mut self, haystack: &str) {
        self.matches.clear();
        self.current = 0;
        if self.query.is_empty() {
            return;
        }
        let needle = self.query.to_lowercase();
        let haystack_lower = haystack.to_lowercase();
        if needle.is_empty() {
            return;
        }
        let mut from = 0;
        while let Some(found) = haystack_lower[from..].find(&needle) {
            let start = from + found;
            let end = start + needle.len();
            if haystack.is_char_boundary(start) && haystack.is_char_boundary(end) {
                self.matches.push(start..end);
            }
            from = end.max(start + 1);
            if self.matches.len() >= 5_000 {
                break;
            }
        }
    }

    pub fn next_match(&mut self) -> Option<Range<usize>> {
        if self.matches.is_empty() {
            return None;
        }
        self.current = (self.current + 1) % self.matches.len();
        self.matches.get(self.current).cloned()
    }

    pub fn previous_match(&mut self) -> Option<Range<usize>> {
        if self.matches.is_empty() {
            return None;
        }
        self.current = (self.current + self.matches.len() - 1) % self.matches.len();
        self.matches.get(self.current).cloned()
    }

    pub fn current_range(&self) -> Option<Range<usize>> {
        self.matches.get(self.current).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huge_documents_are_sliced_at_a_block_boundary() {
        let body = "## Section\n\nSome prose that goes on.\n\n".repeat(200_000);
        let (display, truncated) = slice_for_display(&body);
        assert!(truncated);
        assert!(display.len() <= MAX_DISPLAY_BYTES + 512);
        assert!(display.ends_with("not rendered.*\n"));
        assert!(display.contains("Showing the first"));
        let head = display.split("\n\n---\n\n").next().unwrap();
        let last_line = head.lines().last().unwrap_or_default();
        assert!(
            last_line == "Some prose that goes on." || last_line == "## Section",
            "cut at a block boundary, got {last_line:?}"
        );
    }

    #[test]
    fn small_documents_are_untouched() {
        let (display, truncated) = slice_for_display("# hi\n");
        assert!(!truncated);
        assert_eq!(display, "# hi\n");
    }

    #[test]
    fn search_finds_all_occurrences_case_insensitively() {
        let mut find = Find {
            query: "read".into(),
            ..Find::default()
        };
        find.search("Reading is READ, read again");
        assert_eq!(find.matches, vec![0..4, 11..15, 17..21]);
        assert_eq!(find.next_match(), Some(11..15));
        assert_eq!(find.next_match(), Some(17..21));
        assert_eq!(find.next_match(), Some(0..4), "wraps around");
        assert_eq!(find.previous_match(), Some(17..21));
    }

    #[test]
    fn search_is_empty_without_a_query() {
        let mut find = Find::default();
        find.search("anything");
        assert!(find.matches.is_empty());
        assert_eq!(find.next_match(), None);
    }
}
