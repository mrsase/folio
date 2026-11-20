//! File identity: the stamp taken when a document is read, and the conflict
//! classification performed before a write.

use std::fmt;
use std::fs::Metadata;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskStamp {
    pub len: u64,
    pub modified: Option<SystemTime>,
    pub dev: u64,
    pub ino: u64,
}

impl DiskStamp {
    pub fn of(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            Self {
                len: metadata.len(),
                modified: metadata.modified().ok(),
                dev: metadata.dev(),
                ino: metadata.ino(),
            }
        }
        #[cfg(not(unix))]
        {
            Self {
                len: metadata.len(),
                modified: metadata.modified().ok(),
                dev: 0,
                ino: 0,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conflict {
    /// Same inode, but size or mtime differs.
    Modified,
    /// The path no longer exists.
    Deleted,
    /// A different inode now occupies the path.
    Replaced,
    /// A new file was expected, but something already exists there.
    Exists,
}

impl fmt::Display for Conflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Conflict::Modified => write!(f, "the file changed on disk"),
            Conflict::Deleted => write!(f, "the file was deleted on disk"),
            Conflict::Replaced => write!(f, "the file was replaced by another file"),
            Conflict::Exists => write!(f, "a file already exists at that path"),
        }
    }
}

/// Compare the stamp captured at load with the path's current metadata.
pub fn compare(recorded: Option<DiskStamp>, current: Option<&Metadata>) -> Option<Conflict> {
    match (recorded, current) {
        (None, None) => None,
        (None, Some(_)) => Some(Conflict::Exists),
        (Some(_), None) => Some(Conflict::Deleted),
        (Some(recorded), Some(metadata)) => {
            let current = DiskStamp::of(metadata);
            if (recorded.ino != 0 && current.ino != 0 && recorded.ino != current.ino)
                || (recorded.dev != 0 && current.dev != 0 && recorded.dev != current.dev)
            {
                Some(Conflict::Replaced)
            } else if recorded.len != current.len || recorded.modified != current.modified {
                Some(Conflict::Modified)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn stamp(path: &std::path::Path) -> Option<DiskStamp> {
        fs::metadata(path).ok().map(|meta| DiskStamp::of(&meta))
    }

    #[test]
    fn unchanged_metadata_has_no_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "# hi").unwrap();
        let recorded = stamp(&path);
        let meta = fs::metadata(&path).unwrap();
        assert_eq!(compare(recorded, Some(&meta)), None);
    }

    #[test]
    fn a_size_change_is_modified() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "# hi").unwrap();
        let recorded = stamp(&path);
        fs::write(&path, "# hi there").unwrap();
        let meta = fs::metadata(&path).unwrap();
        assert_eq!(compare(recorded, Some(&meta)), Some(Conflict::Modified));
    }

    #[test]
    fn same_size_with_a_new_mtime_is_modified() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "# hi").unwrap();
        let recorded = stamp(&path);
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(&path, "# hi").unwrap();
        let meta = fs::metadata(&path).unwrap();
        assert_eq!(compare(recorded, Some(&meta)), Some(Conflict::Modified));
    }

    #[test]
    fn deletion_and_creation_are_distinct() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "# hi").unwrap();
        let recorded = stamp(&path);
        fs::remove_file(&path).unwrap();
        assert_eq!(compare(recorded, None), Some(Conflict::Deleted));
        fs::write(&path, "x").unwrap();
        let meta = fs::metadata(&path).unwrap();
        assert_eq!(compare(None, Some(&meta)), Some(Conflict::Exists));
    }

    #[test]
    fn a_replaced_inode_wins_over_size_equality() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        let other = dir.path().join("b.md");
        fs::write(&path, "# hi").unwrap();
        let recorded = stamp(&path);
        fs::write(&other, "# hi").unwrap();
        fs::rename(&other, &path).unwrap();
        let meta = fs::metadata(&path).unwrap();
        let conflict = compare(recorded, Some(&meta));
        assert!(
            matches!(conflict, Some(Conflict::Replaced)),
            "expected replacement, got {conflict:?}"
        );
    }
}
