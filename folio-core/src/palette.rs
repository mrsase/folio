//! Quick-open palette ranking. Pure: the app feeds it an index result and reads
//! the ordered rows.

use crate::index::SearchHit;
use crate::util::fuzzy_match;
use std::path::{Path, PathBuf};

pub struct Palette {
    pub query: String,
    pub hits: Vec<SearchHit>,
    pub selected: usize,
    pub scanning: bool,
    pub truncated: bool,
    /// Precomputed ranks for the current query, in display order.
    ranked: Vec<(i32, usize)>,
}

impl Default for Palette {
    fn default() -> Self {
        Self::new()
    }
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

    pub fn is_empty(&self) -> bool {
        self.ranked.is_empty()
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

    /// A directory typed into the palette's query field, resolved against the
    /// current root. Used to re-root without a native dialog.
    pub fn folder_candidate_in(&self, root: Option<&Path>) -> Option<PathBuf> {
        let query = self.query.trim();
        if query.is_empty() || !crate::paths::looks_like_path(query) {
            return None;
        }
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let resolved = crate::paths::resolve(query, root, home.as_deref()).ok()?;
        resolved.is_dir().then_some(resolved)
    }

    /// Compatibility helper for callers with no root.
    pub fn folder_candidate(&self) -> Option<PathBuf> {
        self.folder_candidate_in(None)
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
    use crate::index::{self, Limits};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn ranks_and_moves_selection() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let paths = ["reading.md", "src/reading-guide.md", "other.md"];
        for path in paths {
            let full = root.join(path);
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(&full, "# x").unwrap();
        }
        let outcome = index::scan(root, &AtomicBool::new(false), Limits::default());
        let mut palette = Palette::new();
        palette.set_hits(outcome.hits, false);
        assert_eq!(palette.len(), 3);
        palette.set_query("reading");
        let names: Vec<_> = (0..palette.len())
            .filter_map(|index| palette.result_at(index).map(|hit| hit.relative.clone()))
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.iter().all(|name| name.contains("reading")));
        palette.move_selection(1);
        assert!(palette.selected_hit().is_some());
    }

    #[test]
    fn a_typed_directory_becomes_a_folder_candidate() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("nested")).unwrap();
        let mut palette = Palette::new();
        palette.set_query(dir.path().to_string_lossy().to_string());
        assert_eq!(
            palette.folder_candidate(),
            Some(std::fs::canonicalize(dir.path()).unwrap())
        );
        palette.set_query(dir.path().join("nested/nope").to_string_lossy().to_string());
        assert!(palette.folder_candidate().is_none());
        palette.set_query("just words");
        assert!(palette.folder_candidate().is_none());
    }

    #[test]
    fn a_relative_directory_resolves_against_the_root() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("docs")).unwrap();
        let mut palette = Palette::new();
        palette.set_query("docs");
        assert!(palette.folder_candidate_in(Some(dir.path())).is_none());
        palette.set_query("docs/");
        assert_eq!(
            palette.folder_candidate_in(Some(dir.path())),
            Some(std::fs::canonicalize(dir.path().join("docs")).unwrap())
        );
    }
}
