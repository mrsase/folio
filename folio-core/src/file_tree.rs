//! Filesystem model for the sidebar browser.
//!
//! The tree is a flat row list rebuilt from the set of expanded directories.
//! Collapsed directories are never scanned, so browsing a huge repository costs
//! one `read_dir` per directory the user actually opened.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Extensions we treat as openable documents.
pub const MARKDOWN_EXTS: [&str; 7] = ["md", "markdown", "mdown", "mkd", "mdx", "mdwn", "mdoc"];

pub fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            MARKDOWN_EXTS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(ext))
        })
}

#[derive(Debug, Clone)]
pub struct Node {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

impl Node {
    pub fn is_markdown(&self) -> bool {
        !self.is_dir && is_markdown_path(&self.path)
    }
}

#[derive(Debug, Default)]
pub struct FileTree {
    root: Option<PathBuf>,
    expanded: HashSet<PathBuf>,
    rows: Vec<Node>,
    show_hidden: bool,
    filter: String,
}

impl FileTree {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn rows(&self) -> &[Node] {
        &self.rows
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn row(&self, index: usize) -> Option<&Node> {
        self.rows.get(index)
    }

    pub fn filter(&self) -> &str {
        &self.filter
    }

    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    pub fn set_show_hidden(&mut self, show: bool) {
        self.show_hidden = show;
        self.rebuild();
    }

    pub fn toggle_hidden(&mut self) {
        self.show_hidden = !self.show_hidden;
        self.rebuild();
    }

    /// Point the tree at a new root. The root itself is always expanded.
    pub fn set_root(&mut self, path: impl Into<PathBuf>) {
        let path = path.into();
        self.root = Some(path.clone());
        self.expanded.clear();
        self.expanded.insert(path);
        self.filter.clear();
        self.rebuild();
    }

    pub fn close_root(&mut self) {
        self.root = None;
        self.expanded.clear();
        self.rows.clear();
        self.filter.clear();
    }

    pub fn refresh(&mut self) {
        self.rebuild();
    }

    pub fn set_filter(&mut self, filter: impl Into<String>) {
        let filter = filter.into();
        if filter == self.filter {
            return;
        }
        self.filter = filter;
        self.rebuild();
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }

    pub fn set_expanded(&mut self, path: &Path, expanded: bool) {
        let changed = if expanded {
            self.expanded.insert(path.to_path_buf())
        } else {
            self.expanded.remove(path)
        };
        if changed {
            self.rebuild();
        }
    }

    /// Toggle a row. Returns true when the row is a directory.
    pub fn toggle_row(&mut self, index: usize) -> bool {
        let Some(node) = self.rows.get(index) else {
            return false;
        };
        if !node.is_dir {
            return false;
        }
        let expanded = node.expanded;
        let path = node.path.clone();
        self.set_expanded(&path, !expanded);
        true
    }

    /// Collapse the row, or jump to its parent when already collapsed.
    pub fn collapse_or_parent(&mut self, index: usize) -> Option<usize> {
        let node = self.rows.get(index)?;
        if node.is_dir && node.expanded {
            let path = node.path.clone();
            self.set_expanded(&path, false);
            return self.index_of(&path);
        }
        let depth = node.depth;
        if depth == 0 {
            return Some(index);
        }
        let parent = self.rows[..index]
            .iter()
            .rposition(|row| row.depth + 1 == depth)?;
        Some(parent)
    }

    /// Expand the row, or move into its first child when already expanded.
    pub fn expand_or_child(&mut self, index: usize) -> Option<usize> {
        let node = self.rows.get(index)?.clone();
        if !node.is_dir {
            return Some(index);
        }
        if !node.expanded {
            self.set_expanded(&node.path, true);
            return self.index_of(&node.path);
        }
        (self.rows.len() > index + 1 && self.rows[index + 1].depth > node.depth)
            .then_some(index + 1)
            .or(Some(index))
    }

    pub fn index_of(&self, path: &Path) -> Option<usize> {
        self.rows.iter().position(|row| row.path == path)
    }

    /// Expand every ancestor of `path` so it becomes visible, and return its row.
    pub fn reveal(&mut self, path: &Path) -> Option<usize> {
        let root = self.root.as_deref()?;
        if !path.starts_with(root) {
            return None;
        }
        let relative = path.strip_prefix(root).ok()?;
        let mut current = root.to_path_buf();
        for component in relative.parent().unwrap_or(Path::new("")).components() {
            current.push(component);
            self.expanded.insert(current.clone());
        }
        self.rebuild();
        self.index_of(path)
    }

    fn rebuild(&mut self) {
        self.rows.clear();
        let Some(root) = self.root.clone() else {
            return;
        };
        if !root.exists() {
            return;
        }
        let filtering = !self.filter.trim().is_empty();
        let needle = self.filter.trim().to_lowercase();
        self.walk(&root, 0, filtering.then_some(needle.as_str()));
    }

    /// Walk `dir` depth-first, appending kept rows. Returns whether anything was
    /// kept, so an active filter can prune directories with no matches.
    fn walk(&mut self, dir: &Path, depth: usize, needle: Option<&str>) -> bool {
        let Some(mut entries) = read_sorted(dir, self.show_hidden) else {
            return false;
        };
        entries.retain(|entry| !should_skip(&entry.path));

        let mut kept_any = false;
        for entry in entries {
            let is_dir = entry.is_dir;
            let name = entry.name.clone();
            let path = entry.path;
            let lower = name.to_lowercase();
            let name_matches = needle.is_some_and(|needle| lower.contains(needle));
            let expanded = is_dir && self.expanded.contains(&path);

            let row = self.rows.len();
            self.rows.push(Node {
                path: path.clone(),
                name,
                depth,
                is_dir,
                expanded,
            });

            let mut kept = name_matches;
            if is_dir && expanded {
                kept |= self.walk(&path, depth + 1, needle);
            }

            if needle.is_some() && !kept {
                self.rows.truncate(row);
                continue;
            }
            kept_any = true;
        }
        kept_any
    }
}

fn should_skip(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == ".DS_Store")
}

struct Entry {
    path: PathBuf,
    name: String,
    is_dir: bool,
}

fn read_sorted(dir: &Path, show_hidden: bool) -> Option<Vec<Entry>> {
    let read = fs::read_dir(dir).ok()?;
    let mut entries = Vec::new();
    for entry in read.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') && !show_hidden {
            continue;
        }
        let is_dir = if file_type.is_symlink() {
            // Never follow symlinked directories: cheap cycle protection.
            false
        } else {
            file_type.is_dir()
        };
        entries.push(Entry {
            path: entry.path(),
            name,
            is_dir,
        });
    }
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Some(entries)
}

/// Standard places offered in the sidebar so the app never needs a file dialog
/// to reach the obvious folders.
pub fn places() -> Vec<(&'static str, PathBuf)> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let Some(home) = home else {
        return Vec::new();
    };
    let mut places = vec![("Home", home.clone())];
    for (label, name) in [
        ("Desktop", "Desktop"),
        ("Documents", "Documents"),
        ("Downloads", "Downloads"),
        ("Developer", "Developer"),
    ] {
        let path = home.join(name);
        if path.is_dir() {
            places.push((label, path));
        }
    }
    places
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "folio-test-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, "# hi").unwrap();
    }

    #[test]
    fn collapsed_directories_are_not_scanned() {
        let root = tempdir("collapse");
        touch(&root.join("zeta.md"));
        touch(&root.join("docs/guide.md"));
        fs::create_dir_all(root.join("docs/deep")).unwrap();
        touch(&root.join("docs/deep/nested.md"));

        let mut tree = FileTree::new();
        tree.set_root(&root);
        let names: Vec<_> = tree.rows().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["docs", "zeta.md"], "hidden dirs stay collapsed");

        let docs = tree.index_of(&root.join("docs")).unwrap();
        tree.toggle_row(docs);
        let names: Vec<_> = tree.rows().iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["docs", "deep", "guide.md", "zeta.md"]);
        assert!(!names.contains(&"nested.md"));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn filter_keeps_matching_files_and_their_ancestors() {
        let root = tempdir("filter");
        touch(&root.join("notes/reading.md"));
        touch(&root.join("notes/shopping.md"));
        touch(&root.join("other/reading-list.md"));
        touch(&root.join("other/unrelated.md"));

        let mut tree = FileTree::new();
        tree.set_root(&root);
        tree.set_expanded(&root.join("notes"), true);
        tree.set_expanded(&root.join("other"), true);
        tree.set_filter("reading");

        let rows: Vec<_> = tree.rows().iter().map(|r| r.name.as_str()).collect();
        assert!(rows.contains(&"reading.md"));
        assert!(rows.contains(&"reading-list.md"));
        assert!(!rows.contains(&"shopping.md"));
        assert!(rows.contains(&"notes") && rows.contains(&"other"));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn reveal_expands_ancestors() {
        let root = tempdir("reveal");
        touch(&root.join("a/b/c/target.md"));
        let mut tree = FileTree::new();
        tree.set_root(&root);
        let row = tree.reveal(&root.join("a/b/c/target.md")).unwrap();
        assert_eq!(tree.row(row).unwrap().name, "target.md");
        assert_eq!(tree.row(row).unwrap().depth, 3);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn extension_matching_is_case_insensitive() {
        assert!(is_markdown_path(Path::new("/x/README.MD")));
        assert!(is_markdown_path(Path::new("/x/a.mdx")));
        assert!(!is_markdown_path(Path::new("/x/a.txt")));
    }
}
