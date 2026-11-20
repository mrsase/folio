//! The editable file buffer: decode UTF-8 with BOM and any line-ending style,
//! keep an LF-only editing representation, and re-emit the original bytes on
//! save. This is the only module that decides how a document becomes bytes.

use crate::atomic;
use crate::checkbox;
use crate::conflict::{self, Conflict, DiskStamp};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Files above this are not read at all; the app shows a size notice.
pub const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;
/// Fully editable source.
pub const MAX_EDIT_BYTES: usize = 2 * 1024 * 1024;
/// Read-only source, still writable for task checkboxes.
pub const MAX_READ_ONLY_BYTES: usize = 8 * 1024 * 1024;

const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    Lf,
    Crlf,
    Cr,
}

impl Ending {
    pub fn bytes(self) -> &'static [u8] {
        match self {
            Ending::Lf => b"\n",
            Ending::Crlf => b"\r\n",
            Ending::Cr => b"\r",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditPolicy {
    Editable,
    ReadOnlySource,
    PreviewOnly,
    Refused,
}

impl EditPolicy {
    pub fn for_len(len: usize) -> Self {
        if len <= MAX_EDIT_BYTES {
            EditPolicy::Editable
        } else if len <= MAX_READ_ONLY_BYTES {
            EditPolicy::ReadOnlySource
        } else if len <= MAX_FILE_BYTES {
            EditPolicy::PreviewOnly
        } else {
            EditPolicy::Refused
        }
    }
}

#[derive(Debug)]
pub enum LoadError {
    Io(io::Error),
    NotUtf8 { offset: usize, detail: String },
    Utf16 { big_endian: bool },
    Binary,
    TooLarge { bytes: u64, limit: usize },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io(error) => write!(f, "{error}"),
            LoadError::NotUtf8 { offset, detail } => write!(
                f,
                "not valid UTF-8 (byte {offset}: {detail}); convert the file to UTF-8 to edit it"
            ),
            LoadError::Utf16 { big_endian } => write!(
                f,
                "UTF-16{} is not supported; convert the file to UTF-8",
                if *big_endian { "BE" } else { "LE" }
            ),
            LoadError::Binary => {
                write!(
                    f,
                    "the file contains NUL bytes; it does not look like Markdown"
                )
            }
            LoadError::TooLarge { bytes, limit } => write!(
                f,
                "the file is {bytes} bytes; Folio opens at most {limit} bytes"
            ),
        }
    }
}

impl std::error::Error for LoadError {}

#[derive(Debug)]
pub enum SaveError {
    Conflict(Conflict),
    NotWritable,
    Io(io::Error),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Conflict(conflict) => write!(f, "{conflict}"),
            SaveError::NotWritable => write!(f, "this document is not writable"),
            SaveError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<io::Error> for SaveError {
    fn from(error: io::Error) -> Self {
        SaveError::Io(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved { bytes: usize },
    Unchanged,
}

#[derive(Debug, Clone)]
struct Decoded {
    text: String,
    endings: Vec<Ending>,
    dominant: Ending,
    trailing_newline: bool,
}

fn decode(bytes: &[u8]) -> Result<(Decoded, bool), LoadError> {
    let (body, bom) = if bytes.starts_with(&BOM) {
        (&bytes[BOM.len()..], true)
    } else {
        (bytes, false)
    };
    if body.starts_with(&[0xFF, 0xFE]) {
        return Err(LoadError::Utf16 { big_endian: false });
    }
    if body.starts_with(&[0xFE, 0xFF]) {
        return Err(LoadError::Utf16 { big_endian: true });
    }
    let source = std::str::from_utf8(body).map_err(|error| LoadError::NotUtf8 {
        offset: error.valid_up_to() + if bom { BOM.len() } else { 0 },
        detail: error.to_string(),
    })?;
    if source.as_bytes().contains(&0) {
        return Err(LoadError::Binary);
    }

    let mut text = String::with_capacity(source.len());
    let mut endings = Vec::new();
    let bytes = source.as_bytes();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\r' => {
                text.push_str(&source[start..index]);
                if bytes.get(index + 1) == Some(&b'\n') {
                    endings.push(Ending::Crlf);
                    index += 2;
                } else {
                    endings.push(Ending::Cr);
                    index += 1;
                }
                text.push('\n');
                start = index;
            }
            b'\n' => {
                text.push_str(&source[start..index]);
                endings.push(Ending::Lf);
                index += 1;
                text.push('\n');
                start = index;
            }
            _ => index += 1,
        }
    }
    text.push_str(&source[start..]);

    let mut lf = 0;
    let mut crlf = 0;
    let mut cr = 0;
    for ending in &endings {
        match ending {
            Ending::Lf => lf += 1,
            Ending::Crlf => crlf += 1,
            Ending::Cr => cr += 1,
        }
    }
    let dominant = if crlf >= lf && crlf >= cr && crlf > 0 {
        Ending::Crlf
    } else if cr > lf && cr > 0 {
        Ending::Cr
    } else {
        Ending::Lf
    };
    let trailing_newline = text.ends_with('\n');
    Ok((
        Decoded {
            text,
            endings,
            dominant,
            trailing_newline,
        },
        bom,
    ))
}

#[derive(Debug, Clone)]
pub struct Document {
    pub path: PathBuf,
    /// LF-only editing buffer.
    pub text: String,
    pub bom: bool,
    /// One ending per terminator in the file as loaded.
    pub endings: Vec<Ending>,
    pub dominant: Ending,
    pub trailing_newline: bool,
    pub mode: u32,
    pub disk: Option<DiskStamp>,
    pub policy: EditPolicy,
    original_text: Option<String>,
    revision: u64,
    saved_revision: u64,
}

impl Document {
    pub fn load(path: &Path) -> Result<Self, LoadError> {
        let metadata = fs::metadata(path).map_err(LoadError::Io)?;
        if metadata.len() > MAX_FILE_BYTES as u64 {
            return Err(LoadError::TooLarge {
                bytes: metadata.len(),
                limit: MAX_FILE_BYTES,
            });
        }
        let bytes = fs::read(path).map_err(LoadError::Io)?;
        Self::from_parts(
            path.to_path_buf(),
            &bytes,
            Some(DiskStamp::of(&metadata)),
            mode_of(&metadata),
        )
    }

    pub fn from_parts(
        path: PathBuf,
        bytes: &[u8],
        disk: Option<DiskStamp>,
        mode: u32,
    ) -> Result<Self, LoadError> {
        let policy = EditPolicy::for_len(bytes.len());
        if policy == EditPolicy::Refused {
            return Err(LoadError::TooLarge {
                bytes: bytes.len() as u64,
                limit: MAX_FILE_BYTES,
            });
        }
        let (decoded, bom) = decode(bytes)?;
        let original_text =
            (!matches!(policy, EditPolicy::PreviewOnly)).then(|| decoded.text.clone());
        Ok(Self {
            path,
            text: decoded.text,
            bom,
            endings: decoded.endings,
            dominant: decoded.dominant,
            trailing_newline: decoded.trailing_newline,
            mode,
            disk,
            policy,
            original_text,
            revision: 0,
            saved_revision: 0,
        })
    }

    /// A new, never-saved document. It starts dirty.
    pub fn new_untitled(path: PathBuf, text: String) -> Self {
        Self {
            path,
            text,
            bom: false,
            endings: Vec::new(),
            dominant: Ending::Lf,
            trailing_newline: false,
            mode: 0o644,
            disk: None,
            policy: EditPolicy::Editable,
            original_text: Some(String::new()),
            revision: 1,
            saved_revision: 0,
        }
    }

    pub fn can_edit(&self) -> bool {
        self.policy == EditPolicy::Editable
    }

    pub fn can_write(&self) -> bool {
        matches!(
            self.policy,
            EditPolicy::Editable | EditPolicy::ReadOnlySource
        )
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn saved_revision(&self) -> u64 {
        self.saved_revision
    }

    pub fn is_dirty(&self) -> bool {
        self.disk.is_none() || self.revision != self.saved_revision
    }

    /// Replace the editing buffer. Refused for read-only documents; task
    /// checkbox edits use [`Self::toggle_checkbox_at`] instead.
    pub fn apply_edit(&mut self, text: String) -> bool {
        if !self.can_edit() || text == self.text {
            return false;
        }
        self.text = text;
        self.revision += 1;
        true
    }

    pub fn toggle_checkbox_at(&mut self, offset: usize) -> Option<bool> {
        if !self.can_write() {
            return None;
        }
        let result = checkbox::toggle_checkbox_at(&mut self.text, offset);
        if result.is_some() {
            self.revision += 1;
        }
        result
    }

    pub fn task_counts(&self) -> (usize, usize) {
        checkbox::count_tasks(&self.text)
    }

    /// The exact bytes a save would write.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.text.len() + self.endings.len() + 3);
        if self.bom {
            out.extend_from_slice(&BOM);
        }
        let new_lines: Vec<&str> = self.text.split('\n').collect();
        let original = self.original_text.as_deref().unwrap_or("");
        let original_lines: Vec<&str> = original.split('\n').collect();
        let prefix = common_prefix(&new_lines, &original_lines);
        let suffix = common_suffix(&new_lines, &original_lines, prefix);

        for (index, line) in new_lines.iter().enumerate() {
            out.extend_from_slice(line.as_bytes());
            if index + 1 == new_lines.len() {
                break;
            }
            let ending =
                self.ending_for(index, new_lines.len(), original_lines.len(), prefix, suffix);
            out.extend_from_slice(ending.bytes());
        }
        out
    }

    fn ending_for(
        &self,
        index: usize,
        new_len: usize,
        original_len: usize,
        prefix: usize,
        suffix: usize,
    ) -> Ending {
        // Editing a line's text does not change its terminator. When the line
        // count is unchanged, map by position; inserted/removed lines fall back
        // to the prefix/suffix mapping below.
        if new_len == original_len
            && let Some(ending) = self.endings.get(index)
        {
            return *ending;
        }
        if index < prefix
            && let Some(ending) = self.endings.get(index)
        {
            return *ending;
        }
        if suffix > 0 && index >= new_len.saturating_sub(suffix) {
            let original_index = original_len + index - new_len;
            if let Some(ending) = self.endings.get(original_index) {
                return *ending;
            }
        }
        self.dominant
    }

    pub fn conflict(&self) -> Option<Conflict> {
        let metadata = fs::metadata(&self.path).ok();
        conflict::compare(self.disk, metadata.as_ref())
    }

    pub fn save(&mut self) -> Result<SaveOutcome, SaveError> {
        if !self.can_write() {
            return Err(SaveError::NotWritable);
        }
        if self.disk.is_some() && self.revision == self.saved_revision {
            return Ok(SaveOutcome::Unchanged);
        }
        if let Some(conflict) = self.conflict() {
            return Err(SaveError::Conflict(conflict));
        }
        self.write_to(self.path.clone(), false)
    }

    /// User-confirmed overwrite after a conflict dialog.
    pub fn save_overwriting_conflict(&mut self) -> Result<SaveOutcome, SaveError> {
        if !self.can_write() {
            return Err(SaveError::NotWritable);
        }
        self.write_to(self.path.clone(), true)
    }

    /// Save to another path. The caller has already confirmed overwrite (the
    /// native save panel does this).
    pub fn save_as(&mut self, path: PathBuf) -> Result<SaveOutcome, SaveError> {
        if !self.can_write() {
            return Err(SaveError::NotWritable);
        }
        self.write_to(path, true)
    }

    fn write_to(&mut self, target: PathBuf, overwrite: bool) -> Result<SaveOutcome, SaveError> {
        let existing = fs::metadata(&target).ok();
        // `Exists` is only a conflict for a brand-new document. An existing
        // document writing its own path has already passed the stamp check.
        if existing.is_some() && !overwrite && self.disk.is_none() {
            return Err(SaveError::Conflict(Conflict::Exists));
        }
        let mode = existing
            .as_ref()
            .map(mode_of)
            .unwrap_or(if self.disk.is_some() {
                self.mode
            } else {
                0o644
            });
        let bytes = self.to_bytes();
        if existing.is_some() {
            atomic::write_atomic(&target, &bytes, mode)?;
        } else {
            atomic::create_new_atomic(&target, &bytes, mode)?;
        }
        let metadata = fs::metadata(&target)?;
        self.path = target;
        self.mode = mode_of(&metadata);
        self.disk = Some(DiskStamp::of(&metadata));
        self.saved_revision = self.revision;
        Ok(SaveOutcome::Saved { bytes: bytes.len() })
    }

    /// Re-read from disk, replacing buffer and stamps. Returns whether the
    /// bytes changed.
    pub fn reload(&mut self) -> Result<bool, LoadError> {
        let fresh = Self::load(&self.path)?;
        let changed = fresh.text != self.text;
        *self = fresh;
        Ok(changed)
    }
}

fn common_prefix(a: &[&str], b: &[&str]) -> usize {
    a.iter().zip(b).take_while(|(a, b)| a == b).count()
}

fn common_suffix(a: &[&str], b: &[&str], prefix: usize) -> usize {
    let max = a.len().min(b.len()).saturating_sub(prefix);
    let mut count = 0;
    while count < max && a[a.len() - 1 - count] == b[b.len() - 1 - count] {
        count += 1;
    }
    count
}

#[cfg(unix)]
fn mode_of(metadata: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn mode_of(_metadata: &fs::Metadata) -> u32 {
    0o644
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, bytes).unwrap();
    }

    fn load(bytes: &[u8]) -> (tempfile::TempDir, PathBuf, Document) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.md");
        write(&path, bytes);
        let document = Document::load(&path).unwrap();
        (dir, path, document)
    }

    #[test]
    fn lf_without_bom_round_trips_an_edit() {
        let (_dir, path, mut doc) = load(b"# hi\n\nbody\n");
        assert_eq!(doc.policy, EditPolicy::Editable);
        assert!(doc.apply_edit("# hello\n\nbody\n".into()));
        doc.save().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"# hello\n\nbody\n");
    }

    #[test]
    fn crlf_is_preserved_through_an_edit() {
        let (_dir, path, mut doc) = load(b"# hi\r\n\r\nbody\r\n");
        doc.apply_edit("# hello\n\nbody\n".into());
        doc.save().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"# hello\r\n\r\nbody\r\n");
    }

    #[test]
    fn bom_and_crlf_are_preserved_together() {
        let (_dir, path, mut doc) = load(b"\xEF\xBB\xBF# hi\r\nbody\r\n");
        assert!(doc.bom);
        doc.apply_edit("# hello\nbody\n".into());
        doc.save().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"\xEF\xBB\xBF# hello\r\nbody\r\n");
    }

    #[test]
    fn a_missing_trailing_newline_stays_missing() {
        let (_dir, path, mut doc) = load(b"# hi");
        doc.apply_edit("# hey".into());
        doc.save().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"# hey");
    }

    #[test]
    fn a_present_trailing_newline_stays_present() {
        let (_dir, path, mut doc) = load(b"# hi\n");
        doc.apply_edit("# hey\n".into());
        doc.save().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"# hey\n");
    }

    #[test]
    fn lone_cr_endings_survive() {
        let (_dir, path, mut doc) = load(b"a\rb\r");
        doc.apply_edit("a\nB\n".into());
        doc.save().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"a\rB\r");
    }

    #[test]
    fn mixed_endings_keep_untouched_head_and_tail() {
        let original = b"a\r\nb\nc\r\n";
        let (_dir, path, mut doc) = load(original);
        doc.apply_edit("a\nB\nc\n".into());
        doc.save().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"a\r\nB\nc\r\n");
    }

    #[test]
    fn a_clean_save_does_not_touch_the_file() {
        let (_dir, path, mut doc) = load(b"# hi\r\n");
        let before = fs::metadata(&path).unwrap().modified().unwrap();
        assert_eq!(doc.save().unwrap(), SaveOutcome::Unchanged);
        assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
    }

    #[test]
    fn invalid_utf8_is_refused_with_an_offset() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("latin1.md");
        write(&path, b"caf\xe9\n");
        let error = Document::load(&path).unwrap_err();
        assert!(
            matches!(error, LoadError::NotUtf8 { offset: 3, .. }),
            "{error:?}"
        );
    }

    #[test]
    fn utf16_is_refused_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("utf16.md");
        write(&path, &[0xFF, 0xFE, b'a', 0, b'b', 0]);
        assert!(matches!(
            Document::load(&path).unwrap_err(),
            LoadError::Utf16 { big_endian: false }
        ));
        write(&path, &[0xFE, 0xFF, 0, b'a']);
        assert!(matches!(
            Document::load(&path).unwrap_err(),
            LoadError::Utf16 { big_endian: true }
        ));
    }

    #[test]
    fn nul_bytes_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bin.md");
        write(&path, b"# hi\0\n");
        assert!(matches!(
            Document::load(&path).unwrap_err(),
            LoadError::Binary
        ));
    }

    #[test]
    fn external_change_is_a_conflict_and_is_not_overwritten() {
        let (_dir, path, mut doc) = load(b"# hi\n");
        doc.apply_edit("# mine\n".into());
        write(&path, b"# theirs, longer\n");
        let error = doc.save().unwrap_err();
        assert!(
            matches!(error, SaveError::Conflict(Conflict::Modified)),
            "{error:?}"
        );
        assert_eq!(fs::read(&path).unwrap(), b"# theirs, longer\n");

        doc.save_overwriting_conflict().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"# mine\n");
    }

    #[test]
    fn deleted_file_is_reported() {
        let (_dir, path, mut doc) = load(b"# hi\n");
        doc.apply_edit("# mine\n".into());
        fs::remove_file(&path).unwrap();
        assert!(matches!(
            doc.save().unwrap_err(),
            SaveError::Conflict(Conflict::Deleted)
        ));
    }

    #[test]
    fn checkbox_writes_preserve_bom_and_crlf() {
        let (_dir, path, mut doc) = load(b"\xEF\xBB\xBF# x\r\n- [ ] task\r\n");
        let offset = doc.text.find("task").unwrap();
        assert_eq!(doc.toggle_checkbox_at(offset), Some(true));
        doc.save().unwrap();
        assert_eq!(
            fs::read(&path).unwrap(),
            b"\xEF\xBB\xBF# x\r\n- [x] task\r\n"
        );
    }

    #[test]
    fn save_as_writes_a_new_path_and_retargets() {
        let (_dir, _path, mut doc) = load(b"# hi\n");
        doc.apply_edit("# hello\n".into());
        let target_dir = tempfile::tempdir().unwrap();
        let target = target_dir.path().join("copy.md");
        doc.save_as(target.clone()).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"# hello\n");
        assert_eq!(doc.path, target);
        assert!(!doc.is_dirty());
    }

    #[test]
    fn policy_thresholds_are_explicit() {
        assert_eq!(EditPolicy::for_len(MAX_EDIT_BYTES), EditPolicy::Editable);
        assert_eq!(
            EditPolicy::for_len(MAX_EDIT_BYTES + 1),
            EditPolicy::ReadOnlySource
        );
        assert_eq!(
            EditPolicy::for_len(MAX_READ_ONLY_BYTES + 1),
            EditPolicy::PreviewOnly
        );
        assert_eq!(EditPolicy::for_len(MAX_FILE_BYTES + 1), EditPolicy::Refused);
    }

    #[test]
    fn oversized_files_are_refused_before_reading() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.md");
        let file = fs::File::create(&path).unwrap();
        file.set_len(MAX_FILE_BYTES as u64 + 1).unwrap();
        assert!(matches!(
            Document::load(&path).unwrap_err(),
            LoadError::TooLarge { .. }
        ));
    }

    #[test]
    fn read_only_documents_refuse_text_edits_but_allow_checkboxes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.md");
        let mut bytes = b"- [ ] task\n".to_vec();
        bytes.resize(MAX_EDIT_BYTES + 10, b'x');
        write(&path, &bytes);
        let mut doc = Document::load(&path).unwrap();
        assert_eq!(doc.policy, EditPolicy::ReadOnlySource);
        assert!(!doc.apply_edit("# changed\n".into()));
        let offset = doc.text.find("task").unwrap();
        assert_eq!(doc.toggle_checkbox_at(offset), Some(true));
    }
}
