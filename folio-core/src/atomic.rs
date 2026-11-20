//! Atomic writes: sibling temp file, fsync, mode preservation, rename, parent
//! directory fsync. A failure before the rename leaves the target untouched.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Test hook: inject a failure at a named point. Production callers use
/// [`write_atomic`], which never injects anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteFault {
    None,
    /// Write half the bytes, sync, then fail.
    DuringWrite,
    /// Fully write and sync the temp file, then fail before the rename.
    BeforeRename,
}

pub fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    write_atomic_with_fault(path, bytes, mode, WriteFault::None)
}

/// Replace `path` atomically. The original is never truncated.
pub fn write_atomic_with_fault(
    path: &Path,
    bytes: &[u8],
    mode: u32,
    fault: WriteFault,
) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target path has no parent"))?;
    let temp = temp_path(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        if fault == WriteFault::DuringWrite {
            let half = bytes.len() / 2;
            file.write_all(&bytes[..half])?;
            file.sync_all()?;
            return Err(injected("write"));
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        set_mode(&temp, mode)?;
        if fault == WriteFault::BeforeRename {
            return Err(injected("before rename"));
        }
        fs::rename(&temp, path)?;
        sync_dir(parent)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

/// Create a file that must not already exist, with the bytes fully written and
/// synced before the name appears. `hard_link` gives no-replace semantics; when
/// the filesystem does not support it, a checked rename is the fallback.
pub fn create_new_atomic(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "target already exists",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target path has no parent"))?;
    let temp = temp_path(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        set_mode(&temp, mode)?;
        match fs::hard_link(&temp, path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                return Err(error);
            }
            Err(_) => {
                // Filesystems without hard links: check, then rename. The
                // window is small and the caller has already confirmed intent.
                if path.exists() {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "target already exists",
                    ));
                }
                fs::rename(&temp, path)?;
            }
        }
        sync_dir(parent)?;
        Ok(())
    })();

    // Success leaves the target link; failure leaves a partial temp. Either
    // way the temp name is not needed after this point.
    let _ = fs::remove_file(&temp);
    result
}

/// Remove stale temp siblings older than `age`. Called before a save; it never
/// touches the target.
pub fn remove_stale_temps(dir: &Path, age: std::time::Duration) -> io::Result<usize> {
    let mut removed = 0;
    let now = std::time::SystemTime::now();
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(0);
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !(name.starts_with('.') && name.contains(".folio-") && name.ends_with(".tmp")) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let old = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|elapsed| elapsed >= age);
        if old && fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

fn temp_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string());
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".{name}.folio-{}-{counter}.tmp",
        std::process::id()
    ))
}

fn injected(point: &str) -> io::Error {
    io::Error::other(format!("injected failure {point}"))
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> io::Result<()> {
    Ok(())
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temps(dir: &Path) -> Vec<PathBuf> {
        fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().contains(".folio-"))
                    .unwrap_or(false)
            })
            .collect()
    }

    #[test]
    fn replaces_and_preserves_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        write_atomic(&path, b"new", 0o600).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(temps(dir.path()).is_empty());
    }

    #[test]
    fn a_failure_before_rename_keeps_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "original").unwrap();

        let error = write_atomic_with_fault(&path, b"replacement", 0o644, WriteFault::BeforeRename)
            .unwrap_err();
        assert!(error.to_string().contains("before rename"));
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert!(temps(dir.path()).is_empty());
    }

    #[test]
    fn a_failure_during_write_keeps_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "original").unwrap();

        write_atomic_with_fault(&path, b"replacement bytes", 0o644, WriteFault::DuringWrite)
            .unwrap_err();
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert!(temps(dir.path()).is_empty());
    }

    #[test]
    fn create_new_refuses_an_existing_target() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        fs::write(&path, "existing").unwrap();
        let error = create_new_atomic(&path, b"new", 0o644).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&path).unwrap(), b"existing");
    }

    #[test]
    fn create_new_writes_the_file_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        create_new_atomic(&path, b"# new\n", 0o644).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"# new\n");
        assert!(temps(dir.path()).is_empty());
    }

    #[test]
    fn stale_temps_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.md");
        let temp = dir.path().join(".a.md.folio-1-1.tmp");
        fs::write(&path, "x").unwrap();
        fs::write(&temp, "junk").unwrap();
        let removed = remove_stale_temps(dir.path(), std::time::Duration::ZERO).unwrap();
        assert_eq!(removed, 1);
        assert!(!temp.exists());
        assert!(path.exists());
    }
}
