//! Recent files and folders: a small, deduplicated MRU list that is pruned
//! whenever a path disappears.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_RECENTS: usize = 20;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recents {
    #[serde(default)]
    pub files: Vec<PathBuf>,
    #[serde(default)]
    pub folders: Vec<PathBuf>,
}

impl Recents {
    pub fn note_file(&mut self, path: &Path) {
        note(&mut self.files, path);
    }

    pub fn note_folder(&mut self, path: &Path) {
        note(&mut self.folders, path);
    }

    /// Drop entries that no longer exist. Called at load, never on every frame.
    pub fn prune(&mut self) {
        self.files.retain(|path| path.is_file());
        self.folders.retain(|path| path.is_dir());
    }

    pub fn clear(&mut self) {
        self.files.clear();
        self.folders.clear();
    }
}

fn note(list: &mut Vec<PathBuf>, path: &Path) {
    let path = crate::paths::canonical(path);
    list.retain(|existing| existing != &path);
    list.insert(0, path);
    list.truncate(MAX_RECENTS);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_and_deduplicated() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        std::fs::write(&a, "# a").unwrap();
        std::fs::write(&b, "# b").unwrap();

        let mut recents = Recents::default();
        recents.note_file(&a);
        recents.note_file(&b);
        recents.note_file(&a);
        assert_eq!(recents.files.len(), 2);
        assert_eq!(recents.files[0], crate::paths::canonical(&a));
    }

    #[test]
    fn caps_at_twenty() {
        let mut recents = Recents::default();
        for index in 0..25 {
            recents.note_file(Path::new(&format!("/tmp/{index}.md")));
        }
        assert_eq!(recents.files.len(), MAX_RECENTS);
        assert!(recents.files[0].ends_with("24.md"));
    }

    #[test]
    fn prune_drops_missing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("exists.md");
        std::fs::write(&existing, "# hi").unwrap();
        let missing = dir.path().join("gone.md");

        let mut recents = Recents {
            files: vec![missing, existing.clone()],
            folders: vec![dir.path().join("nope")],
        };
        recents.prune();
        assert_eq!(recents.files, vec![existing]);
        assert!(recents.folders.is_empty());
    }
}
