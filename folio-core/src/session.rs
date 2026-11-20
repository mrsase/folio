//! Session persistence: window geometry, root, tabs, per-tab mode and scroll,
//! recents, theme and document font size. Written atomically.

use crate::atomic;
use crate::recents::Recents;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const SESSION_VERSION: u32 = 1;
pub const SIDEBAR_MIN: f32 = 200.0;
pub const SIDEBAR_MAX: f32 = 560.0;
pub const FONT_MIN: f32 = 12.0;
pub const FONT_MAX: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ViewMode {
    #[default]
    Preview,
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TabState {
    pub path: PathBuf,
    pub mode: ViewMode,
    /// Scroll offset in pixels; restored into the tab's outer scroll handle.
    pub scroll_y: f32,
    /// Editor cursor byte offset; restored when switching back to Source.
    pub cursor: usize,
}

impl Default for TabState {
    fn default() -> Self {
        Self {
            path: PathBuf::new(),
            mode: ViewMode::Preview,
            scroll_y: 0.0,
            cursor: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub version: u32,
    pub root: Option<PathBuf>,
    pub tabs: Vec<TabState>,
    pub active: Option<PathBuf>,
    pub window: Option<WindowRect>,
    pub sidebar_width: f32,
    pub sidebar_visible: bool,
    pub outline_open: bool,
    pub theme: ThemePreference,
    pub font_size: f32,
    pub remember_root: bool,
    pub recents: Recents,
    /// Proof-of-concept sessions stored a flat `open_files` list. Read it and
    /// migrate; never write it back.
    #[serde(rename = "open_files", skip_serializing)]
    legacy_open_files: Vec<PathBuf>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            version: SESSION_VERSION,
            root: None,
            tabs: Vec::new(),
            active: None,
            window: None,
            sidebar_width: 272.0,
            sidebar_visible: true,
            outline_open: false,
            theme: ThemePreference::System,
            font_size: 16.0,
            remember_root: true,
            recents: Recents::default(),
            legacy_open_files: Vec::new(),
        }
    }
}

impl Session {
    pub fn store_path() -> Option<PathBuf> {
        if let Some(dir) = std::env::var_os("FOLIO_SESSION_DIR") {
            return Some(PathBuf::from(dir).join("session.json"));
        }
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library/Application Support/Folio")
                .join("session.json"),
        )
    }

    pub fn load() -> Self {
        let Some(path) = Self::store_path() else {
            return Self::default();
        };
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Self {
        let Ok(bytes) = fs::read(path) else {
            return Self::default();
        };
        let mut session: Session = serde_json::from_slice(&bytes).unwrap_or_default();
        session.migrate();
        session.sanitize();
        session
    }

    pub fn save(&self) {
        let Some(path) = Self::store_path() else {
            return;
        };
        self.save_to(&path);
    }

    pub fn save_to(&self, path: &Path) {
        let Some(parent) = path.parent() else {
            return;
        };
        if fs::create_dir_all(parent).is_err() {
            return;
        }
        let Ok(json) = serde_json::to_vec_pretty(self) else {
            return;
        };
        let _ = atomic::write_atomic(path, &json, 0o600);
    }

    fn migrate(&mut self) {
        if self.tabs.is_empty() && !self.legacy_open_files.is_empty() {
            self.tabs = self
                .legacy_open_files
                .iter()
                .cloned()
                .map(|path| TabState {
                    path,
                    ..TabState::default()
                })
                .collect();
            if self.active.is_none() {
                self.active = self.tabs.first().map(|tab| tab.path.clone());
            }
        }
        self.legacy_open_files.clear();
        self.version = SESSION_VERSION;
    }

    fn sanitize(&mut self) {
        self.tabs
            .retain(|tab| !tab.path.as_os_str().is_empty() && tab.path.is_file());
        self.tabs.dedup_by(|a, b| a.path == b.path);
        for tab in &mut self.tabs {
            tab.scroll_y = tab.scroll_y.max(0.0);
        }
        if let Some(active) = &self.active
            && !self.tabs.iter().any(|tab| &tab.path == active)
        {
            self.active = self.tabs.first().map(|tab| tab.path.clone());
        }
        if let Some(root) = &self.root
            && !root.is_dir()
        {
            self.root = None;
        }
        self.recents.prune();
        self.sidebar_width = self.sidebar_width.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
        self.font_size = self.font_size.clamp(FONT_MIN, FONT_MAX);
    }

    pub fn note_root(&mut self, root: &Path) {
        self.root = Some(root.to_path_buf());
        self.recents.note_folder(root);
    }

    pub fn note_file(&mut self, path: &Path) {
        self.recents.note_file(path);
    }

    pub fn tab_mut(&mut self, path: &Path) -> Option<&mut TabState> {
        self.tabs.iter_mut().find(|tab| tab.path == path)
    }

    pub fn upsert_tab(&mut self, tab: TabState) {
        if let Some(existing) = self.tab_mut(&tab.path) {
            *existing = tab;
        } else {
            self.tabs.push(tab);
        }
    }

    pub fn remove_tab(&mut self, path: &Path) {
        self.tabs.retain(|tab| tab.path != path);
        if self.active.as_deref() == Some(path) {
            self.active = self.tabs.first().map(|tab| tab.path.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_session() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.json");
        (dir, path)
    }

    #[test]
    fn round_trip_preserves_every_field() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("doc.md");
        fs::write(&file, "# hi").unwrap();

        let mut session = Session {
            root: Some(dir.path().to_path_buf()),
            theme: ThemePreference::Dark,
            font_size: 18.0,
            remember_root: false,
            sidebar_width: 320.0,
            sidebar_visible: false,
            outline_open: true,
            ..Session::default()
        };
        session.tabs.push(TabState {
            path: file.clone(),
            mode: ViewMode::Source,
            scroll_y: 128.0,
            cursor: 7,
        });
        session.active = Some(file.clone());
        session.window = Some(WindowRect {
            x: 10.0,
            y: 20.0,
            width: 900.0,
            height: 700.0,
        });
        session.note_file(&file);

        let (_session_dir, path) = temp_session();
        session.save_to(&path);
        let loaded = Session::load_from(&path);
        assert_eq!(loaded.root, session.root);
        assert_eq!(loaded.tabs.len(), 1);
        assert_eq!(loaded.tabs[0].mode, ViewMode::Source);
        assert_eq!(loaded.tabs[0].scroll_y, 128.0);
        assert_eq!(loaded.tabs[0].cursor, 7);
        assert_eq!(loaded.theme, ThemePreference::Dark);
        assert_eq!(loaded.font_size, 18.0);
        assert!(!loaded.remember_root);
        assert_eq!(loaded.sidebar_width, 320.0);
        assert!(!loaded.sidebar_visible);
        assert!(loaded.outline_open);
        assert_eq!(loaded.window, session.window);
        assert_eq!(loaded.recents.files, session.recents.files);
        assert_eq!(loaded.version, SESSION_VERSION);
    }

    #[test]
    fn unknown_and_missing_fields_fall_back_to_defaults() {
        let (_dir, path) = temp_session();
        fs::write(&path, br#"{"theme":"light","future_field":42}"#).unwrap();
        let loaded = Session::load_from(&path);
        assert_eq!(loaded.theme, ThemePreference::Light);
        assert_eq!(loaded.sidebar_width, 272.0);
        assert!(loaded.tabs.is_empty());
    }

    #[test]
    fn proof_of_concept_open_files_are_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        fs::write(&a, "# a").unwrap();
        fs::write(&b, "# b").unwrap();
        let (_session_dir, path) = temp_session();
        let json = format!(
            r#"{{"root":"{}","open_files":["{}","{}"],"active":"{}"}}"#,
            dir.path().display(),
            a.display(),
            b.display(),
            b.display()
        );
        fs::write(&path, json).unwrap();
        let loaded = Session::load_from(&path);
        assert_eq!(loaded.tabs.len(), 2);
        assert_eq!(loaded.tabs[1].path, b);
        assert_eq!(loaded.active, Some(b));
    }

    #[test]
    fn missing_active_file_falls_back_to_the_first_tab() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.md");
        fs::write(&a, "# a").unwrap();
        let (_session_dir, path) = temp_session();
        let json = format!(
            r#"{{"tabs":[{{"path":"{}"}}],"active":"{}"}}"#,
            a.display(),
            dir.path().join("gone.md").display()
        );
        fs::write(&path, json).unwrap();
        let loaded = Session::load_from(&path);
        assert_eq!(loaded.tabs.len(), 1);
        assert_eq!(loaded.active, Some(a));
    }

    #[test]
    fn missing_files_and_roots_are_dropped() {
        let (_dir, path) = temp_session();
        fs::write(
            &path,
            r#"{"root":"/definitely/not/here","tabs":[{"path":"/definitely/not/here.md"}]}"#,
        )
        .unwrap();
        let loaded = Session::load_from(&path);
        assert!(loaded.tabs.is_empty());
        assert!(loaded.root.is_none());
    }

    #[test]
    fn widths_and_font_sizes_are_clamped() {
        let (_dir, path) = temp_session();
        fs::write(&path, br#"{"sidebar_width":9000,"font_size":80}"#).unwrap();
        let loaded = Session::load_from(&path);
        assert_eq!(loaded.sidebar_width, SIDEBAR_MAX);
        assert_eq!(loaded.font_size, FONT_MAX);
    }

    #[test]
    fn session_file_is_written_with_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, path) = temp_session();
        Session::default().save_to(&path);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
