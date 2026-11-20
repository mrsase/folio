//! Integration tests against real files: create, save, save-as, overwrite and
//! encoding refusal through the public `folio-core` API.

use folio_core::document::{Document, LoadError, SaveError};
use std::fs;
use std::path::Path;

fn doc(path: &Path, bytes: &[u8]) -> Document {
    fs::write(path, bytes).unwrap();
    Document::load(path).unwrap()
}

#[test]
fn create_save_and_save_as_keep_the_bytes_exact() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.md");

    let mut document = Document::new_untitled(path.clone(), "# New\n\nbody\n".to_string());
    assert!(document.is_dirty());
    document.save().unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"# New\n\nbody\n");

    let copy = dir.path().join("copy.md");
    document.save_as(copy.clone()).unwrap();
    assert_eq!(fs::read(&copy).unwrap(), b"# New\n\nbody\n");
    assert_eq!(document.path, copy);
    assert!(!document.is_dirty());
}

#[test]
fn overwriting_an_existing_target_keeps_its_mode() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.md");
    let target = dir.path().join("target.md");
    fs::write(&target, "old").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();

    let mut document = doc(&source, b"# source\n");
    document.save_as(target.clone()).unwrap();
    assert_eq!(fs::read(&target).unwrap(), b"# source\n");
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn create_refuses_to_overwrite_an_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("taken.md");
    fs::write(&path, "keep me").unwrap();

    let mut document = Document::new_untitled(path.clone(), "# new\n".to_string());
    let error = document.save().unwrap_err();
    assert!(matches!(error, SaveError::Conflict(_)), "{error:?}");
    assert_eq!(fs::read(&path).unwrap(), b"keep me");
}

#[test]
fn non_utf8_is_refused_without_creating_a_buffer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("latin1.md");
    fs::write(&path, b"caf\xe9\n").unwrap();
    let error = Document::load(&path).unwrap_err();
    assert!(matches!(error, LoadError::NotUtf8 { .. }), "{error:?}");
}

#[test]
fn utf8_bom_crlf_and_no_trailing_newline_round_trip_together() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("windows.md");
    let original = b"\xEF\xBB\xBF# Title\r\n\r\nline one\r\nline two";
    let mut document = doc(&path, original);
    assert_eq!(document.to_bytes(), original);

    document.apply_edit("# Title\n\nline one\nline two".into());
    document.save().unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        b"\xEF\xBB\xBF# Title\r\n\r\nline one\r\nline two"
    );
}

#[test]
fn new_file_creates_and_refuses_an_existing_target() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fresh.md");
    let mut document = Document::new_untitled(path.clone(), "# Fresh\n".to_string());
    document.save().unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"# Fresh\n");

    let mut second = Document::new_untitled(path.clone(), "# Other\n".to_string());
    let error = second.save().unwrap_err();
    assert!(matches!(error, SaveError::Conflict(_)), "{error:?}");
    assert_eq!(fs::read(&path).unwrap(), b"# Fresh\n");
}

#[test]
fn a_clean_save_still_detects_an_external_change() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("doc.md");
    fs::write(&path, "# hi\n").unwrap();
    let mut document = Document::load(&path).unwrap();
    fs::write(&path, "# changed externally\n").unwrap();
    let error = document.save().unwrap_err();
    assert!(matches!(error, SaveError::Conflict(_)), "{error:?}");
    assert_eq!(fs::read(&path).unwrap(), b"# changed externally\n");
}
