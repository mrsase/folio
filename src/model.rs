//! Application model types that do not touch GPUI: an open document's content,
//! in-document search, and the quick-open palette's ranking.

use folio::file_tree::SearchHit;
use folio::outline::{self, Heading};
use folio::util::fuzzy_match;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Longest prefix of a document handed to the layout engine.
///
/// `TextView` lays out every block it is given; measured on this machine, 256 KB
/// still paints promptly while 512 KB and up stall the first frame for seconds
/// and retain up to 1.3 GB of layout state. A reader never needs the whole of a
/// 2 MB file at once, so the tail is replaced with a notice; the full text is
/// still what gets written back.
pub const MAX_DISPLAY_BYTES: usize = 256 * 1024;

/// A document loaded from disk.
#[derive(Debug, Clone)]
pub struct OpenDoc {
    pub path: PathBuf,
    /// The whole file, used for outline offsets and every write-back.
    pub source: String,
    /// What the reader renders: `source`, or its head with a truncation notice.
    pub display: String,
    pub truncated: bool,
    pub headings: Vec<Heading>,
    pub modified: Option<SystemTime>,
    pub bytes: u64,
    /// Set when the file could not be read; the tab still exists so the reader
    /// shows the reason instead of silently doing nothing.
    pub error: Option<String>,
}

impl OpenDoc {
    pub fn load(path: PathBuf) -> Self {
        let mut doc = Self {
            path,
            source: String::new(),
            display: String::new(),
            truncated: false,
            headings: Vec::new(),
            modified: None,
            bytes: 0,
            error: None,
        };
        doc.read();
        doc
    }

    /// Recompute the rendered slice after `source` changed.
    pub fn reslice(&mut self) {
        let (display, truncated) = slice_for_display(&self.source);
        self.display = display;
        self.truncated = truncated;
    }

    pub fn dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("/"))
    }

    pub fn title(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    /// Re-read the file. Returns true when the contents changed.
    pub fn read(&mut self) -> bool {
        let metadata = std::fs::metadata(&self.path);
        let modified = metadata.as_ref().ok().and_then(|m| m.modified().ok());
        match std::fs::read(&self.path) {
            Ok(bytes) => {
                self.bytes = bytes.len() as u64;
                let text = match String::from_utf8(bytes) {
                    Ok(text) => text,
                    Err(error) => {
                        self.error = Some(format!("Not valid UTF-8: {error}"));
                        return false;
                    }
                };
                let text = normalize(&text);
                let changed = text != self.source;
                if changed {
                    self.source = text;
                    self.reslice();
                    self.reparse();
                }
                self.modified = modified;
                self.error = None;
                changed
            }
            Err(error) => {
                self.error = Some(error.to_string());
                false
            }
        }
    }

    /// Refresh only when the file changed on disk.
    pub fn reload_if_stale(&mut self) -> bool {
        let modified = std::fs::metadata(&self.path)
            .and_then(|m| m.modified())
            .ok();
        if modified.is_some() && modified == self.modified {
            return false;
        }
        self.read()
    }

    fn reparse(&mut self) {
        // Parse what is displayed: offsets in the slice are source offsets,
        // because the slice is a prefix cut at a paragraph boundary.
        self.headings = outline::outline(&self.display).headings;
    }

}

/// River-cut the source at a block boundary for display.
///
/// The cut is at the last blank line before the limit so a paragraph, table or
/// list is not half-rendered, and a dangling fence is closed so the tail does
/// not turn into one giant code block.
fn slice_for_display(source: &str) -> (String, bool) {
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

/// Strip a UTF-8 BOM and unify line endings so offsets and rendering agree.
fn normalize(text: &str) -> String {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.contains('\r') {
        text.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        text.to_string()
    }
}

/// Case-insensitive substring search over already-rendered text.
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
            // Only accept byte ranges that are valid in the original text.
            if haystack.is_char_boundary(start) && haystack.is_char_boundary(end) {
                self.matches.push(start..end);
            }
            from = end.max(start + 1);
            if self.matches.len() >= 5_000 {
                break;
            }
        }
    }

    /// Move to the next match and return its range.
    pub fn next(&mut self) -> Option<Range<usize>> {
        if self.matches.is_empty() {
            return None;
        }
        self.current = (self.current + 1) % self.matches.len();
        self.matches.get(self.current).cloned()
    }

    pub fn previous(&mut self) -> Option<Range<usize>> {
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

/// Quick-open palette: files scanned under the workspace root, ranked per keystroke.
pub struct Palette {
    pub query: String,
    pub hits: Vec<SearchHit>,
    pub selected: usize,
    pub scanning: bool,
    pub truncated: bool,
    /// Precomputed ranks for the current query, in display order.
    ranked: Vec<(i32, usize)>,
}

impl Palette {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            hits: Vec::new(),
            selected: 0,
            scanning: false,
            truncated: false,
            ranked: Vec::new(),
        }
    }

    pub fn set_hits(&mut self, hits: Vec<SearchHit>, truncated: bool) {
        self.hits = hits;
        self.truncated = truncated;
        self.scanning = false;
        self.rank();
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        let query = query.into();
        if query == self.query {
            return;
        }
        self.query = query;
        self.selected = 0;
        self.rank();
    }

    pub fn rank(&mut self) {
        self.ranked.clear();
        if self.query.trim().is_empty() {
            self.ranked = self
                .hits
                .iter()
                .enumerate()
                .take(200)
                .map(|(index, _)| (0, index))
                .collect();
            return;
        }
        let mut scored: Vec<(i32, usize)> = self
            .hits
            .iter()
            .enumerate()
            .filter_map(|(index, hit)| {
                fuzzy_match(&self.query, &hit.relative).map(|m| (m.score, index))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        scored.truncate(200);
        self.ranked = scored;
    }

    pub fn len(&self) -> usize {
        self.ranked.len()
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.ranked.is_empty() {
            self.selected = 0;
            return;
        }
        let len = self.ranked.len() as isize;
        let next = (self.selected as isize + delta).rem_euclid(len);
        self.selected = next as usize;
    }

    /// A directory typed into the palette's query field, so the browser can be
    /// pointed anywhere without a native file dialog.
    pub fn folder_candidate(&self) -> Option<PathBuf> {
        let query = self.query.trim();
        if query.is_empty() {
            return None;
        }
        let expanded = if let Some(rest) = query.strip_prefix("~/") {
            PathBuf::from(std::env::var_os("HOME")?).join(rest)
        } else if query == "~" {
            PathBuf::from(std::env::var_os("HOME")?)
        } else if query.starts_with('/') {
            PathBuf::from(query)
        } else {
            return None;
        };
        expanded.is_dir().then_some(expanded)
    }

    pub fn result_at(&self, index: usize) -> Option<&SearchHit> {
        let (_, hit) = self.ranked.get(index)?;
        self.hits.get(*hit)
    }

    pub fn selected_hit(&self) -> Option<&SearchHit> {
        let (_, index) = self.ranked.get(self.selected)?;
        self.hits.get(*index)
    }

    pub fn selected_index(&self) -> usize {
        self.selected
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
        // The slice ends at a block boundary, never mid-line.
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
    fn normalization_handles_bom_and_crlf() {
        assert_eq!(normalize("\u{feff}# hi\r\nnext\r\n"), "# hi\nnext\n");
        assert_eq!(normalize("plain\n"), "plain\n");
    }

    #[test]
    fn search_finds_all_occurrences_case_insensitively() {
        let mut find = Find::default();
        find.query = "read".into();
        find.search("Reading is READ, read again");
        assert_eq!(find.matches, vec![0..4, 11..15, 17..21]);
        // The search already reveals the first match, so stepping starts at
        // the second one.
        assert_eq!(find.next(), Some(11..15));
        assert_eq!(find.next(), Some(17..21));
        assert_eq!(find.next(), Some(0..4), "wraps around");
        assert_eq!(find.previous(), Some(17..21));
    }

    #[test]
    fn search_is_empty_without_a_query() {
        let mut find = Find::default();
        find.search("anything");
        assert!(find.matches.is_empty());
        assert_eq!(find.next(), None);
    }

    #[test]
    fn a_typed_directory_becomes_a_folder_candidate() {
        let dir = std::env::temp_dir().join(format!("folio-fold-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("nested")).unwrap();
        let mut palette = Palette::new();
        palette.set_query(dir.to_string_lossy().to_string());
        assert_eq!(palette.folder_candidate(), Some(dir.clone()));
        palette.set_query(dir.join("nested/nope").to_string_lossy().to_string());
        assert!(palette.folder_candidate().is_none());
        palette.set_query("just words");
        assert!(palette.folder_candidate().is_none());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn palette_ranks_and_moves_selection() {
        let root = std::env::temp_dir().join(format!("folio-pal-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let paths = ["reading.md", "src/reading-guide.md", "other.md"];
        for path in paths {
            let full = root.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&full, "# x").unwrap();
        }
        let (hits, _) = folio::file_tree::scan_markdown_files(&root, 100);
        let mut palette = Palette::new();
        palette.set_hits(hits, false);
        assert_eq!(palette.len(), 3);
        palette.set_query("reading");
        let names: Vec<_> = (0..palette.len())
            .filter_map(|index| palette.result_at(index).map(|hit| hit.relative.clone()))
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.iter().all(|name| name.contains("reading")));
        palette.move_selection(1);
        assert_eq!(palette.selected_hits_len(), 1);
        std::fs::remove_dir_all(root).ok();
    }

    impl Palette {
        fn selected_hits_len(&self) -> usize {
            usize::from(self.selected_hit().is_some())
        }
    }
}
