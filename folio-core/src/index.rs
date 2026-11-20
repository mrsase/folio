//! Bounded, cancellable Markdown file index for quick-open. It reads directory
//! entries only, never file contents.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Directories that are never worth walking.
pub const IGNORED_DIRS: [&str; 15] = [
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "Library",
    ".Trash",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    "DerivedData",
    ".next",
    ".cache",
];

pub const MAX_DEPTH: usize = 12;
pub const MAX_ENTRIES: usize = 50_000;
pub const MAX_RESULTS: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub path: PathBuf,
    pub relative: String,
}

#[derive(Debug, Clone)]
pub struct Limits {
    pub max_depth: usize,
    pub max_entries: usize,
    pub max_results: usize,
    pub deadline: Option<Instant>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_depth: MAX_DEPTH,
            max_entries: MAX_ENTRIES,
            max_results: MAX_RESULTS,
            deadline: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct IndexOutcome {
    pub hits: Vec<SearchHit>,
    /// A cap or the deadline stopped the walk.
    pub truncated: bool,
    /// The caller's generation token was flipped.
    pub cancelled: bool,
    pub entries: usize,
}

pub fn is_ignored_dir(name: &str) -> bool {
    IGNORED_DIRS
        .iter()
        .any(|ignored| ignored.eq_ignore_ascii_case(name))
}

/// Walk `root` depth-first. Hidden entries are skipped, symlinked directories
/// are never followed, and every limit is checked before it can be exceeded.
pub fn scan(root: &Path, cancel: &AtomicBool, limits: Limits) -> IndexOutcome {
    let mut outcome = IndexOutcome::default();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    let mut since_deadline_check = 0usize;

    while let Some((dir, depth)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            outcome.cancelled = true;
            break;
        }
        let Some(entries) = std::fs::read_dir(&dir).ok() else {
            continue;
        };
        for entry in entries.flatten() {
            if cancel.load(Ordering::Relaxed) {
                outcome.cancelled = true;
                break;
            }
            since_deadline_check += 1;
            if since_deadline_check >= 256 {
                since_deadline_check = 0;
                if limits
                    .deadline
                    .is_some_and(|deadline| Instant::now() >= deadline)
                {
                    outcome.truncated = true;
                    return finish(outcome);
                }
            }
            outcome.entries += 1;
            if outcome.entries > limits.max_entries {
                outcome.truncated = true;
                return finish(outcome);
            }

            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_dir() {
                if is_ignored_dir(&name) || depth + 1 > limits.max_depth {
                    continue;
                }
                stack.push((path, depth + 1));
            } else if crate::file_tree::is_markdown_path(&path) {
                if outcome.hits.len() >= limits.max_results {
                    outcome.truncated = true;
                    return finish(outcome);
                }
                let relative = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                outcome.hits.push(SearchHit { path, relative });
            }
        }
        if outcome.cancelled {
            break;
        }
    }
    finish(outcome)
}

fn finish(mut outcome: IndexOutcome) -> IndexOutcome {
    outcome.hits.sort_by(|a, b| a.relative.cmp(&b.relative));
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, "# hi").unwrap();
    }

    #[test]
    fn ignores_build_output_and_hidden_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        touch(&root.join("one.md"));
        touch(&root.join("sub/two.markdown"));
        touch(&root.join("node_modules/dep/readme.md"));
        touch(&root.join("target/readme.md"));
        touch(&root.join(".git/notes.md"));
        touch(&root.join("sub/notes.txt"));

        let outcome = scan(root, &AtomicBool::new(false), Limits::default());
        let names: Vec<_> = outcome.hits.iter().map(|h| h.relative.as_str()).collect();
        assert_eq!(names, vec!["one.md", "sub/two.markdown"]);
        assert!(!outcome.truncated);
    }

    #[test]
    fn caps_results_and_marks_partial() {
        let dir = tempfile::tempdir().unwrap();
        for index in 0..10 {
            touch(&dir.path().join(format!("{index}.md")));
        }
        let limits = Limits {
            max_results: 3,
            ..Limits::default()
        };
        let outcome = scan(dir.path(), &AtomicBool::new(false), limits);
        assert_eq!(outcome.hits.len(), 3);
        assert!(outcome.truncated);
    }

    #[test]
    fn caps_depth() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("a/b/c/deep.md"));
        let limits = Limits {
            max_depth: 1,
            ..Limits::default()
        };
        let outcome = scan(dir.path(), &AtomicBool::new(false), limits);
        assert!(outcome.hits.is_empty());
    }

    #[test]
    fn a_cancelled_generation_returns_nothing_usable() {
        let dir = tempfile::tempdir().unwrap();
        touch(&dir.path().join("a.md"));
        let cancel = AtomicBool::new(true);
        let outcome = scan(dir.path(), &cancel, Limits::default());
        assert!(outcome.cancelled);
        assert!(outcome.hits.is_empty());
    }
}
