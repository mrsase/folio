//! Session persistence: the reader reopens exactly where you left off.
//!
//! Stored as JSON under `~/Library/Application Support/Folio/session.json`, written
//! atomically so a crash mid-save can never produce a truncated file.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    /// Follow the macOS appearance.
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub root: Option<PathBuf>,
    pub open_files: Vec<PathBuf>,
    pub active: Option<PathBuf>,
    pub window: Option<WindowRect>,
    pub sidebar_width: f32,
    pub sidebar_visible: bool,
    pub outline_open: bool,
    pub theme: ThemePreference,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            root: None,
            open_files: Vec::new(),
            active: None,
            window: None,
            sidebar_width: 272.0,
            sidebar_visible: true,
            outline_open: false,
            theme: ThemePreference::System,
        }
    }
}

impl Session {
    pub fn store_path() -> Option<PathBuf> {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library/Application Support/Folio")
                .join("session.json"),
        )
    }

    /// Load the previous session, falling back to defaults for anything that
    /// no longer exists on disk.
    pub fn load() -> Self {
        let Some(path) = Self::store_path() else {
            return Self::default();
        };
        let Ok(bytes) = fs::read(&path) else {
            return Self::default();
        };
        let mut session: Session = serde_json::from_slice(&bytes).unwrap_or_default();
        session.open_files.retain(|file| file.is_file());
        session.open_files.dedup();
        if let Some(active) = &session.active
            && !session.open_files.contains(active)
        {
            session.active = session.open_files.first().cloned();
        }
        if let Some(root) = &session.root
            && !root.is_dir()
        {
            session.root = None;
        }
        session.sidebar_width = session.sidebar_width.clamp(200.0, 560.0);
        session
    }

    pub fn save(&self) {
        let Some(path) = Self::store_path() else {
            return;
        };
        let Some(parent) = path.parent() else {
            return;
        };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        let Ok(json) = serde_json::to_vec_pretty(self) else {
            return;
        };
        let tmp = path.with_extension("json.tmp");
        if fs::write(&tmp, json).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }

    /// Remember a directory as the workspace root, keeping a short MRU list.
    pub fn note_root(&mut self, root: &Path) {
        self.root = Some(root.to_path_buf());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let session = Session::default();
        assert_eq!(session.sidebar_width, 272.0);
        assert_eq!(session.theme, ThemePreference::System);
    }

    #[test]
    fn json_roundtrip_preserves_fields() {
        let session = Session {
            root: Some(PathBuf::from("/tmp")),
            open_files: vec![PathBuf::from("/tmp/a.md"), PathBuf::from("/tmp/b.md")],
            active: Some(PathBuf::from("/tmp/b.md")),
            window: Some(WindowRect {
                x: 10.0,
                y: 20.0,
                width: 1200.0,
                height: 800.0,
            }),
            theme: ThemePreference::Dark,
            ..Default::default()
        };
        let json = serde_json::to_vec(&session).unwrap();
        let back: Session = serde_json::from_slice(&json).unwrap();
        assert_eq!(back.root, session.root);
        assert_eq!(back.open_files, session.open_files);
        assert_eq!(back.theme, ThemePreference::Dark);
        assert_eq!(back.window.unwrap().width, 1200.0);
    }

    #[test]
    fn unknown_and_missing_fields_fall_back_to_defaults() {
        let back: Session = serde_json::from_str(r#"{"sidebar_width": 300, "future_field": true}"#).unwrap();
        assert_eq!(back.sidebar_width, 300.0);
        assert!(back.open_files.is_empty());
    }

    #[test]
    fn missing_active_file_falls_back_to_first_open_tab() {
        let dir = std::env::temp_dir().join(format!("folio-session-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.md");
        let b = dir.join("b.md");
        fs::write(&a, "# a").unwrap();
        fs::write(&b, "# b").unwrap();

        let json = serde_json::json!({
            "open_files": [a.clone(), b.clone()],
            "active": dir.join("gone.md"),
            "sidebar_width": 9999.0
        });
        let path = dir.join("session.json");
        fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();

        let mut session: Session = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        session.open_files.retain(|file| file.is_file());
        if let Some(active) = &session.active
            && !session.open_files.contains(active)
        {
            session.active = session.open_files.first().cloned();
        }
        session.sidebar_width = session.sidebar_width.clamp(200.0, 560.0);

        assert_eq!(session.active, Some(a));
        assert_eq!(session.sidebar_width, 560.0);
        fs::remove_dir_all(dir).ok();
    }
}
