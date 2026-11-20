//! State for the single Folio view. All mutation goes through methods here;
//! the view modules only read state and build elements.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use folio_core::document::Document;
use folio_core::file_tree::FileTree;
use folio_core::index::{self, Limits};
use folio_core::outline::{self, Heading};
use folio_core::palette::Palette;
use folio_core::paths;
use folio_core::session::{Session, TabState, ViewMode};
use folio_core::text::{self, Find};
use gpui_kit::component::ResizableState;
use gpui_kit::component::input::{EditorState, InputEvent, InputState};
use gpui_kit::component::text::{RangeHighlight, TextViewState};
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::*;

pub use folio_core::session::ThemePreference;

static START: OnceLock<Instant> = OnceLock::new();
static TRACED_FIRST_FRAME: AtomicBool = AtomicBool::new(false);

pub fn mark_process_start() {
    let _ = START.set(Instant::now());
}

/// Report process start to first frame once. Called from the first render.
pub fn trace_first_frame() {
    if !TRACED_FIRST_FRAME.swap(true, Ordering::Relaxed)
        && let Some(start) = START.get()
    {
        trace(|| format!("first frame at {} ms", start.elapsed().as_millis()));
    }
}

pub fn trace(message: impl FnOnce() -> String) {
    if std::env::var_os("FOLIO_TRACE").is_some() {
        eprintln!("[folio] {}", message());
    }
}

pub struct Tab {
    pub path: PathBuf,
    /// `None` when the file could not be decoded; `error` explains why.
    pub doc: Option<Document>,
    pub view: Entity<TextViewState>,
    pub editor: Option<Entity<EditorState>>,
    pub mode: ViewMode,
    pub scroll: ScrollHandle,
    pub cursor: usize,
    pub headings: Vec<Heading>,
    pub truncated: bool,
    pub error: Option<String>,
    /// The file changed on disk while the buffer was dirty.
    pub disk_changed: bool,
    /// A clean file changed on disk; reload on the next frame with a window.
    pub needs_reload: bool,
    /// The buffer changed (e.g. a checkbox) and the editor needs it back.
    pub needs_sync: bool,
}

/// What happens after a conflict dialog is resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum After {
    Nothing,
    Close(usize),
    Quit,
}

#[derive(Debug, Clone)]
pub enum Dialog {
    Conflict {
        index: usize,
        conflict: folio_core::conflict::Conflict,
        after: After,
    },
    Unsaved {
        index: usize,
    },
    Quit {
        dirty: Vec<usize>,
    },
}

impl Tab {
    pub fn load(path: PathBuf, mode: ViewMode, scroll_y: f32, cursor: usize, cx: &mut App) -> Self {
        let scroll = ScrollHandle::new();
        scroll.set_offset(point(px(0.), px(-scroll_y.max(0.0))));
        match Document::load(&path) {
            Ok(doc) => {
                let (display, truncated) = text::slice_for_display(&doc.text);
                let view = cx.new(|cx| TextViewState::markdown(&display, cx));
                let headings = if doc.text.len() <= folio_core::text::MAX_DISPLAY_BYTES {
                    outline::outline(&doc.text).headings
                } else {
                    Vec::new()
                };
                let editor = None;
                Self {
                    path,
                    doc: Some(doc),
                    view,
                    editor,
                    mode,
                    scroll,
                    cursor,
                    headings,
                    truncated,
                    error: None,
                    disk_changed: false,
                    needs_reload: false,
                    needs_sync: false,
                }
            }
            Err(error) => {
                let message = error.to_string();
                let body = format!(
                    "# This file could not be opened\n\n{}\n\n`{}`\n",
                    message,
                    paths::display(&path, None)
                );
                let view = cx.new(|cx| TextViewState::markdown(&body, cx));
                Self {
                    path,
                    doc: None,
                    view,
                    editor: None,
                    mode: ViewMode::Preview,
                    scroll,
                    cursor: 0,
                    headings: Vec::new(),
                    truncated: false,
                    error: Some(message),
                    disk_changed: false,
                    needs_reload: false,
                    needs_sync: false,
                }
            }
        }
    }

    /// Create the source editor on demand. Large documents use plain text
    /// instead of Markdown highlighting so syntect cannot stall the window.
    pub fn ensure_editor(&mut self, window: &mut Window, cx: &mut App) -> bool {
        if self.editor.is_some() {
            return true;
        }
        let Some(doc) = self.doc.as_ref() else {
            return false;
        };
        if !matches!(
            doc.policy,
            folio_core::document::EditPolicy::Editable
                | folio_core::document::EditPolicy::ReadOnlySource
        ) {
            return false;
        }
        let text = doc.text.clone();
        let language = if text.len() <= 512 * 1024 {
            "markdown"
        } else {
            "text"
        };
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .default_value(text)
                .language(language)
                .folding(false)
        });
        if self.cursor > 0 {
            let end = editor.read(cx).value().len().min(self.cursor);
            editor.update(cx, |state, cx| state.set_selected_range(end..end, cx));
        }
        if self.scroll_y(cx) > 0.0 {
            let y = -self.scroll_y(cx);
            editor.update(cx, |state, cx| {
                state.set_scroll_offset(point(px(0.), px(y)), cx);
            });
        }
        self.editor = Some(editor);
        true
    }

    pub fn title(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    pub fn is_dirty(&self) -> bool {
        self.doc.as_ref().is_some_and(Document::is_dirty)
    }

    pub fn scroll_y(&self, cx: &App) -> f32 {
        if self.mode == ViewMode::Source
            && let Some(editor) = &self.editor
        {
            return (-f32::from(editor.read(cx).scroll_offset().y)).max(0.0);
        }
        (-f32::from(self.scroll.offset().y)).max(0.0)
    }

    pub fn cursor_offset(&self, cx: &App) -> usize {
        self.editor
            .as_ref()
            .map(|editor| editor.read(cx).cursor())
            .unwrap_or(self.cursor)
    }

    pub fn refresh_view(&mut self, cx: &mut App) {
        let Some(doc) = self.doc.as_ref() else {
            return;
        };
        let (display, truncated) = text::slice_for_display(&doc.text);
        self.truncated = truncated;
        self.headings = if doc.text.len() <= folio_core::text::MAX_DISPLAY_BYTES {
            outline::outline(&doc.text).headings
        } else {
            Vec::new()
        };
        self.view
            .update(cx, |state, cx| state.set_text(&display, cx));
    }

    /// Push the buffer into the editor after a reload. Requires a window
    /// because `EditorState::set_value` participates in input/IME state.
    pub fn sync_editor(&self, window: &mut Window, cx: &mut App) {
        let (Some(doc), Some(editor)) = (self.doc.as_ref(), self.editor.as_ref()) else {
            return;
        };
        let text = doc.text.clone();
        if editor.read(cx).value().as_str() == text {
            return;
        }
        editor.update(cx, |state, cx| state.set_value(text, window, cx));
    }

    pub fn show_error(&mut self, message: String, cx: &mut App) {
        let body = format!(
            "# This file could not be opened\n\n{}\n\n`{}`\n",
            message,
            paths::display(&self.path, None)
        );
        self.error = Some(message);
        self.view.update(cx, |state, cx| state.set_text(&body, cx));
    }
}

pub struct NewFileState {
    pub dir: PathBuf,
    pub name: Entity<InputState>,
    pub template: bool,
    pub overwrite: bool,
    pub error: Option<String>,
}

pub struct Folio {
    pub session: Session,
    pub tree: FileTree,
    pub tree_selected: Option<usize>,
    pub tree_focus: FocusHandle,
    pub tabs: Vec<Tab>,
    pub active: Option<usize>,
    pub split: Entity<ResizableState>,
    pub sidebar_filter: Entity<InputState>,
    pub palette: Option<Palette>,
    pub palette_input: Entity<InputState>,
    pub find: Option<Find>,
    pub find_input: Entity<InputState>,
    pub notice: Option<String>,
    pub settings_open: bool,
    pub dialog: Option<Dialog>,
    pub new_file: Option<NewFileState>,
    /// Where a Save As should leave the tab after the panel returns.
    pub save_as_intent: Option<(PathBuf, After)>,
    /// Guards background palette scans against out-of-order completion.
    pub scan_generation: u64,
    pub scan_cancel: Option<Arc<AtomicBool>>,
    pub last_bounds_save: Instant,
    pub watcher: Option<notify::RecommendedWatcher>,
    pub watched_dirs: HashSet<PathBuf>,
    pub pending_changes: Arc<Mutex<Vec<PathBuf>>>,
    pub palette_prefill: Option<String>,
    pub refocus_tree: bool,
    pub pending_opens: Vec<(PathBuf, ViewMode, f32, usize)>,
    pub pending_active: Option<PathBuf>,
    pub tree_scroll: UniformListScrollHandle,
    pub palette_scroll: UniformListScrollHandle,
}

impl Folio {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        session: Session,
        initial: Option<PathBuf>,
    ) -> Self {
        let tree_focus = cx.focus_handle();
        let sidebar_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter files"));
        let palette_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Open a markdown file…"));
        let find_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in document"));

        let mut tree = FileTree::new();
        tree.set_show_hidden(false);

        let initial_file = initial.as_ref().filter(|path| path.is_file()).cloned();
        let root = initial
            .as_ref()
            .filter(|path| path.is_dir())
            .cloned()
            .or_else(|| session.root.clone())
            .or_else(|| {
                initial_file
                    .as_ref()
                    .and_then(|path| path.parent().map(Path::to_path_buf))
            });
        if let Some(root) = &root {
            tree.set_root(root.clone());
        }

        cx.subscribe(
            &palette_input,
            |this, _, event: &InputEvent, cx| match event {
                InputEvent::Change => {
                    let query = this.palette_input.read(cx).value().to_string();
                    this.palette_query(&query, cx);
                }
                InputEvent::PressEnter { .. } => this.palette_accept(cx),
                _ => {}
            },
        )
        .detach();

        cx.subscribe(&sidebar_filter, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let query = this.sidebar_filter.read(cx).value().to_string();
                this.tree.set_filter(query);
                this.tree_selected = (!this.tree.is_empty()).then_some(0);
                cx.notify();
            }
        })
        .detach();

        cx.subscribe(&find_input, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                let query = this.find_input.read(cx).value().to_string();
                this.update_find_query(&query, cx);
            }
        })
        .detach();

        let pending_changes = Arc::new(Mutex::new(Vec::new()));
        let watcher = {
            let pending = pending_changes.clone();
            notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                let Ok(event) = result else { return };
                if event.kind.is_access() {
                    return;
                }
                pending.lock().unwrap().extend(event.paths);
            })
            .ok()
        };

        let pending_opens = if let Some(file) = initial_file {
            vec![(file, ViewMode::Preview, 0.0, 0)]
        } else {
            session
                .tabs
                .iter()
                .map(|tab| (tab.path.clone(), tab.mode, tab.scroll_y, tab.cursor))
                .collect()
        };
        let pending_active = session.active.clone();

        let mut this = Self {
            split: cx.new(|_| ResizableState::default()),
            session,
            tree,
            tree_selected: None,
            tree_focus,
            tabs: Vec::new(),
            active: None,
            sidebar_filter,
            palette: None,
            palette_input,
            find: None,
            find_input,
            notice: None,
            settings_open: false,
            dialog: None,
            new_file: None,
            save_as_intent: None,
            scan_generation: 0,
            scan_cancel: None,
            last_bounds_save: Instant::now() - Duration::from_secs(5),
            watcher,
            watched_dirs: HashSet::new(),
            pending_changes,
            refocus_tree: false,
            palette_prefill: None,
            pending_opens,
            pending_active,
            tree_scroll: UniformListScrollHandle::new(),
            palette_scroll: UniformListScrollHandle::new(),
        };

        if let Some(first) = this.pending_opens.first().map(|open| open.0.clone()) {
            this.tree_selected = this.tree.reveal(&first);
        }
        if let Some(root) = this.tree.root().map(Path::to_path_buf) {
            this.watch_dir(&root);
        }
        this.start_file_watcher(cx);

        let weak = cx.weak_entity();
        window.on_window_should_close(cx, move |_window, cx| {
            let quit = weak
                .update(cx, |this, cx| {
                    if this.request_quit(cx) {
                        cx.quit();
                    }
                    false
                })
                .unwrap_or(true);
            // Never let the platform close the only window behind the guard:
            // a clean close quits through `cx.quit()` above.
            let _ = quit;
            false
        });

        let handle = this.tree_focus.clone();
        window.focus(&handle, cx);
        this
    }

    // ---------------------------------------------------------------- watcher

    fn start_file_watcher(&self, cx: &mut Context<Self>) {
        let pending = self.pending_changes.clone();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(300))
                    .await;
                let batch: Vec<PathBuf> = {
                    let mut guard = pending.lock().unwrap();
                    std::mem::take(&mut *guard)
                };
                if batch.is_empty() {
                    continue;
                }
                if this
                    .update(cx, |this, cx| this.files_changed(batch, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn watch_dir(&mut self, dir: &Path) {
        if !self.watched_dirs.insert(dir.to_path_buf()) {
            return;
        }
        if let Some(watcher) = self.watcher.as_mut() {
            use notify::Watcher as _;
            let _ = watcher.watch(dir, notify::RecursiveMode::NonRecursive);
        }
    }

    fn files_changed(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let mut changed = false;
        for tab in &mut self.tabs {
            if !paths.iter().any(|path| *path == tab.path) {
                continue;
            }
            if tab.is_dirty() {
                tab.disk_changed = true;
                changed = true;
                continue;
            }
            // Our own atomic write produces a watcher event whose stamp is
            // already recorded; do not reload the buffer for it.
            if tab.doc.as_ref().is_some_and(|doc| doc.conflict().is_none()) {
                continue;
            }
            // Reloading the editor needs a window; do it at the next render
            // instead of touching input state from the watcher.
            tab.needs_reload = true;
            changed = true;
        }
        self.tree.refresh();
        if changed {
            cx.notify();
        }
    }

    // --------------------------------------------------------------- documents

    pub fn active_tab(&self) -> Option<&Tab> {
        self.active.and_then(|index| self.tabs.get(index))
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.active.and_then(|index| self.tabs.get_mut(index))
    }

    pub fn open_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        let path = paths::canonical(path);
        if self.tree.root().is_none()
            && let Some(parent) = path.parent()
        {
            self.tree.set_root(parent.to_path_buf());
            self.session.note_root(parent);
            self.watch_dir(parent);
        }
        if let Some(index) = self.tabs.iter().position(|tab| tab.path == path) {
            self.active = Some(index);
        } else {
            self.pending_opens
                .push((path.clone(), ViewMode::Preview, 0.0, 0));
        }
        if let Some(row) = self.tree.reveal(&path) {
            self.tree_selected = Some(row);
            self.tree_scroll.scroll_to_item(row, ScrollStrategy::Center);
        }
        if let Some(parent) = path.parent() {
            self.watch_dir(parent);
        }
        self.session.note_file(&path);
        self.find = None;
        self.persist_session(cx);
        cx.notify();
    }

    pub fn open_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        let root = paths::canonical(&root);
        if !root.is_dir() {
            self.notice = Some(format!("Not a folder: {}", paths::display(&root, None)));
            cx.notify();
            return;
        }
        self.tree.set_root(root.clone());
        self.watch_dir(&root);
        self.tree_selected = None;
        self.session.note_root(&root);
        self.persist_session(cx);
        cx.notify();
    }

    pub fn close_tab_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        if self.tabs[index].is_dirty() {
            self.dialog = Some(Dialog::Unsaved { index });
            cx.notify();
            return;
        }
        self.force_close_tab(index, cx);
    }

    pub fn force_close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.tabs.remove(index);
        self.active = match self.active {
            None => None,
            Some(_) if self.tabs.is_empty() => None,
            Some(active) if active > index => Some(active - 1),
            Some(active) if active == index => Some(active.min(self.tabs.len() - 1)),
            Some(active) => Some(active),
        };
        self.find = None;
        self.persist_session(cx);
        cx.notify();
    }

    pub fn close_active(&mut self, cx: &mut Context<Self>) {
        if let Some(active) = self.active {
            self.close_tab_at(active, cx);
        }
    }

    pub fn cycle_tab(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.tabs.is_empty() {
            return;
        }
        let len = self.tabs.len() as isize;
        let active = self.active.unwrap_or(0) as isize;
        self.active = Some((active + delta).rem_euclid(len) as usize);
        self.find = None;
        self.persist_session(cx);
        cx.notify();
    }

    pub fn select_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.tabs.len() || self.active == Some(index) {
            return;
        }
        self.active = Some(index);
        self.find = None;
        self.persist_session(cx);
        cx.notify();
    }

    pub fn reload_active(&mut self, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab_mut() else {
            return;
        };
        tab.needs_reload = true;
        cx.notify();
    }

    pub fn toggle_task(&mut self, offset: usize, checked: bool, cx: &mut Context<Self>) {
        trace(|| format!("task toggle at {offset}, checked={checked}"));
        let index = match self.active {
            Some(index) => index,
            None => return,
        };
        let result = {
            let Some(tab) = self.tabs.get_mut(index) else {
                return;
            };
            let Some(doc) = tab.doc.as_mut() else {
                return;
            };
            // The renderer reports the desired state; set the marker explicitly.
            let Some(marker) = folio_core::checkbox::checkbox_marker_at(&doc.text, offset) else {
                trace(|| format!("no checkbox marker at {offset}"));
                return;
            };
            trace(|| format!("checkbox marker at {marker}"));
            folio_core::checkbox::set_checkbox(&mut doc.text, marker, checked);
            doc.bump_revision();
            doc.save()
        };
        match result {
            Ok(_) => {
                if let Some(tab) = self.tabs.get_mut(index) {
                    tab.refresh_view(cx);
                    tab.needs_sync = true;
                }
                self.persist_session(cx);
            }
            Err(folio_core::document::SaveError::Conflict(conflict)) => {
                self.dialog = Some(Dialog::Conflict {
                    index,
                    conflict,
                    after: After::Nothing,
                });
            }
            Err(error) => {
                self.notice = Some(format!("Could not save checkbox: {error}"));
            }
        }
        cx.notify();
    }

    pub fn persist_session(&mut self, cx: &App) {
        self.session.tabs = self
            .tabs
            .iter()
            .map(|tab| TabState {
                path: tab.path.clone(),
                mode: tab.mode,
                scroll_y: tab.scroll_y(cx),
                cursor: tab.cursor_offset(cx),
            })
            .collect();
        self.session.active = self.active_tab().map(|tab| tab.path.clone());
        if self.session.remember_root {
            self.session.root = self.tree.root().map(Path::to_path_buf);
        }
        self.session.save();
    }

    pub fn editor_changed(&mut self, editor: Entity<EditorState>, cx: &mut Context<Self>) {
        let Some(index) = self
            .tabs
            .iter()
            .position(|tab| tab.editor.as_ref() == Some(&editor))
        else {
            return;
        };
        let text = editor.read(cx).value().to_string();
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        let Some(doc) = tab.doc.as_mut() else {
            return;
        };
        if doc.apply_edit(text) {
            // Keep the outline honest for normal-sized documents; a 2 MB
            // parse on every keystroke is not worth it.
            if doc.text.len() <= folio_core::text::MAX_DISPLAY_BYTES {
                tab.headings = outline::outline(&doc.text).headings;
            }
            cx.notify();
        }
    }

    pub fn toggle_preview(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.active else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        if tab.doc.is_none() {
            return;
        }
        if tab.mode == ViewMode::Preview {
            let was_empty = tab.editor.is_none();
            if !tab.ensure_editor(window, cx) {
                self.notice = Some("This file is read-only because it exceeds 8 MiB.".to_string());
                cx.notify();
                return;
            }
            let editor = tab.editor.clone();
            tab.mode = ViewMode::Source;
            if was_empty && let Some(editor) = editor {
                cx.subscribe(&editor, |this, editor, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.editor_changed(editor, cx);
                    }
                })
                .detach();
            }
        } else {
            tab.mode = ViewMode::Preview;
            tab.refresh_view(cx);
        }
        self.persist_session(cx);
        cx.notify();
    }

    pub fn save_active(&mut self, cx: &mut Context<Self>) {
        let index = match self.active {
            Some(index) => index,
            None => return,
        };
        self.save_tab(index, After::Nothing, cx);
    }

    fn save_tab(&mut self, index: usize, after: After, cx: &mut Context<Self>) {
        let result = self
            .tabs
            .get_mut(index)
            .and_then(|tab| tab.doc.as_mut())
            .map(Document::save);
        match result {
            Some(Ok(_)) => {
                if let Some(tab) = self.tabs.get_mut(index) {
                    tab.disk_changed = false;
                    tab.error = None;
                }
                self.persist_session(cx);
                self.after_saved(after, cx);
            }
            Some(Err(folio_core::document::SaveError::Conflict(conflict))) => {
                self.dialog = Some(Dialog::Conflict {
                    index,
                    conflict,
                    after,
                });
            }
            Some(Err(error)) => {
                self.notice = Some(format!("Could not save: {error}"));
            }
            None => {}
        }
        cx.notify();
    }

    fn after_saved(&mut self, after: After, cx: &mut Context<Self>) {
        match after {
            After::Nothing => {}
            After::Close(index) => self.force_close_tab(index, cx),
            After::Quit => cx.quit(),
        }
    }

    pub fn save_as_active(&mut self, cx: &mut Context<Self>) {
        let Some(index) = self.active else {
            return;
        };
        self.save_as_tab(index, After::Nothing, cx);
    }

    fn save_as_tab(&mut self, index: usize, after: After, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let Some(doc) = tab.doc.as_ref() else {
            return;
        };
        if !doc.can_write() {
            self.notice = Some("This document is not writable.".to_string());
            cx.notify();
            return;
        }
        let dir = tab
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("/"));
        let name = tab
            .path
            .file_name()
            .map(|name| name.to_string_lossy().to_string());
        self.save_as_intent = Some((tab.path.clone(), after));
        let receiver = cx.prompt_for_new_path(&dir, name.as_deref());
        cx.spawn(async move |this, cx| {
            let result = receiver.await;
            let Ok(Ok(Some(path))) = result else {
                this.update(cx, |this, cx| {
                    this.save_as_intent = None;
                    cx.notify();
                })
                .ok();
                return;
            };
            this.update(cx, |this, cx| this.finish_save_as(path, cx))
                .ok();
        })
        .detach();
    }

    fn finish_save_as(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some((old_path, after)) = self.save_as_intent.take() else {
            return;
        };
        let Some(index) = self.tabs.iter().position(|tab| tab.path == old_path) else {
            return;
        };
        let result = self
            .tabs
            .get_mut(index)
            .and_then(|tab| tab.doc.as_mut())
            .map(|doc| doc.save_as(path.clone()));
        match result {
            Some(Ok(_)) => {
                if let Some(tab) = self.tabs.get_mut(index) {
                    tab.path = path.clone();
                    tab.error = None;
                    tab.disk_changed = false;
                    tab.refresh_view(cx);
                    tab.needs_sync = true;
                }
                self.session.note_file(&path);
                self.persist_session(cx);
                self.after_saved(after, cx);
            }
            Some(Err(error)) => {
                self.notice = Some(format!("Could not save as: {error}"));
            }
            None => {}
        }
        cx.notify();
    }

    pub fn request_quit(&mut self, cx: &mut Context<Self>) -> bool {
        let dirty: Vec<usize> = self
            .tabs
            .iter()
            .enumerate()
            .filter(|(_, tab)| tab.is_dirty())
            .map(|(index, _)| index)
            .collect();
        if dirty.is_empty() {
            self.persist_session(cx);
            true
        } else {
            self.dialog = Some(Dialog::Quit { dirty });
            cx.notify();
            false
        }
    }

    pub fn dismiss_dialog(&mut self, cx: &mut Context<Self>) {
        self.dialog = None;
        cx.notify();
    }

    pub fn cancel_dialog(&mut self, cx: &mut Context<Self>) {
        self.dismiss_dialog(cx);
    }

    pub fn conflict_reload(&mut self, index: usize, after: After, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.disk_changed = false;
            tab.needs_reload = true;
        }
        self.dialog = None;
        match after {
            After::Nothing => {}
            After::Close(index) => self.force_close_tab(index, cx),
            After::Quit => cx.quit(),
        }
        cx.notify();
    }

    pub fn conflict_overwrite(&mut self, index: usize, after: After, cx: &mut Context<Self>) {
        let result = self
            .tabs
            .get_mut(index)
            .and_then(|tab| tab.doc.as_mut())
            .map(Document::save_overwriting_conflict);
        match result {
            Some(Ok(_)) => {
                if let Some(tab) = self.tabs.get_mut(index) {
                    tab.disk_changed = false;
                    tab.error = None;
                    tab.refresh_view(cx);
                    tab.needs_sync = true;
                }
                self.dialog = None;
                self.persist_session(cx);
                self.after_saved(after, cx);
            }
            Some(Err(error)) => {
                self.notice = Some(format!("Could not overwrite: {error}"));
                self.dialog = None;
            }
            None => self.dialog = None,
        }
        cx.notify();
    }

    pub fn conflict_save_as(&mut self, index: usize, after: After, cx: &mut Context<Self>) {
        self.dialog = None;
        self.save_as_tab(index, after, cx);
    }

    pub fn unsaved_save(&mut self, index: usize, cx: &mut Context<Self>) {
        self.dialog = None;
        self.save_tab(index, After::Close(index), cx);
    }

    pub fn unsaved_discard(&mut self, index: usize, cx: &mut Context<Self>) {
        self.dialog = None;
        self.force_close_tab(index, cx);
    }

    pub fn quit_save_all(&mut self, dirty: Vec<usize>, cx: &mut Context<Self>) {
        for index in dirty {
            let result = self
                .tabs
                .get_mut(index)
                .and_then(|tab| tab.doc.as_mut())
                .map(Document::save);
            match result {
                Some(Ok(_)) => {}
                Some(Err(folio_core::document::SaveError::Conflict(conflict))) => {
                    self.dialog = Some(Dialog::Conflict {
                        index,
                        conflict,
                        after: After::Quit,
                    });
                    cx.notify();
                    return;
                }
                Some(Err(error)) => {
                    self.notice = Some(format!("Could not save: {error}"));
                    cx.notify();
                    return;
                }
                None => {}
            }
        }
        self.persist_session(cx);
        cx.quit();
    }

    pub fn quit_discard_all(&mut self, cx: &mut Context<Self>) {
        cx.quit();
    }

    // -------------------------------------------------------------- open / new

    pub fn open_file_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Open Markdown".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            this.update_in(cx, |this, _window, cx| {
                for path in paths {
                    if crate::state::is_markdown(&path) {
                        this.open_path(&path, cx);
                    } else {
                        this.notice = Some(format!(
                            "Not a markdown file: {}",
                            paths::display(&path, None)
                        ));
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn open_folder_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update_in(cx, |this, _window, cx| this.open_root(path, cx))
                .ok();
        })
        .detach();
    }

    pub fn open_new_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self
            .active_tab()
            .and_then(|tab| tab.path.parent().map(Path::to_path_buf))
            .or_else(|| self.tree.root().map(Path::to_path_buf))
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"));
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("notes.md"));
        cx.subscribe(&name, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                this.create_new_file(cx);
            }
        })
        .detach();
        let handle = name.read(cx).focus_handle(cx).clone();
        self.new_file = Some(NewFileState {
            dir,
            name,
            template: false,
            overwrite: false,
            error: None,
        });
        window.focus(&handle, cx);
        cx.notify();
    }

    pub fn choose_new_file_dir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose Folder".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else {
                return;
            };
            let Some(dir) = paths.into_iter().next() else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                if let Some(state) = this.new_file.as_mut() {
                    state.dir = dir;
                    state.error = None;
                    let handle = state.name.read(cx).focus_handle(cx).clone();
                    window.focus(&handle, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn create_new_file(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.new_file.as_ref() else {
            return;
        };
        let raw = state.name.read(cx).value().to_string();
        let dir = state.dir.clone();
        let template = state.template;
        let overwrite = state.overwrite;
        let name = match validate_new_name(&raw) {
            Ok(name) => name,
            Err(error) => {
                if let Some(state) = self.new_file.as_mut() {
                    state.error = Some(error);
                }
                cx.notify();
                return;
            }
        };
        let path = dir.join(&name);
        let exists = path.exists();
        if exists && !overwrite {
            if let Some(state) = self.new_file.as_mut() {
                state.overwrite = true;
                state.error = Some(format!(
                    "{name} already exists. Press Create again to replace it."
                ));
            }
            cx.notify();
            return;
        }

        let body = if template {
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_string())
                .unwrap_or_else(|| "Untitled".to_string());
            let today = today_string();
            format!("---\ntitle: {stem}\ncreated: {today}\n---\n\n# {stem}\n\n")
        } else {
            String::new()
        };
        let mut doc = Document::new_untitled(path.clone(), body);
        let result = if exists {
            doc.save_as(path.clone())
        } else {
            doc.save()
        };
        match result {
            Ok(_) => {
                self.new_file = None;
                self.session.note_file(&path);
                self.pending_opens
                    .push((path.clone(), ViewMode::Source, 0.0, 0));
                self.refocus_tree = true;
                self.persist_session(cx);
                cx.notify();
            }
            Err(error) => {
                if let Some(state) = self.new_file.as_mut() {
                    state.error = Some(format!("Could not create the file: {error}"));
                }
                cx.notify();
            }
        }
    }

    pub fn close_new_file(&mut self, cx: &mut Context<Self>) {
        self.new_file = None;
        self.refocus_tree = true;
        cx.notify();
    }

    // ---------------------------------------------------------------- settings

    pub fn set_theme(
        &mut self,
        preference: ThemePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mode = match preference {
            ThemePreference::System => {
                gpui_kit::component::theme::ThemeMode::from(window.appearance())
            }
            ThemePreference::Light => gpui_kit::component::theme::ThemeMode::Light,
            ThemePreference::Dark => gpui_kit::component::theme::ThemeMode::Dark,
        };
        gpui_kit::component::theme::Theme::change(mode, Some(window), cx);
        self.session.theme = preference;
        self.persist_session(cx);
        cx.notify();
    }

    pub fn set_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        self.session.font_size =
            size.clamp(folio_core::session::FONT_MIN, folio_core::session::FONT_MAX);
        self.persist_session(cx);
        cx.notify();
    }

    pub fn toggle_remember_root(&mut self, cx: &mut Context<Self>) {
        self.session.remember_root = !self.session.remember_root;
        if !self.session.remember_root {
            self.session.root = None;
        } else {
            self.session.root = self.tree.root().map(Path::to_path_buf);
        }
        self.persist_session(cx);
        cx.notify();
    }

    // -------------------------------------------------------------------- find

    pub fn begin_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() {
            self.find = Some(Find::default());
        }
        let handle = self.find_input.read(cx).focus_handle(cx).clone();
        window.focus(&handle, cx);
        cx.notify();
    }

    pub fn close_find(&mut self, cx: &mut Context<Self>) {
        if self.find.take().is_some() {
            if let Some(tab) = self.active_tab() {
                let state = tab.view.clone();
                state.update(cx, |state, cx| state.clear_range_highlights(cx));
            }
            self.refocus_tree = true;
            cx.notify();
        }
    }

    fn update_find_query(&mut self, query: &str, cx: &mut Context<Self>) {
        let Some(index) = self.active else { return };
        let Some(tab) = self.tabs.get(index) else {
            return;
        };
        let source_mode = tab.mode == ViewMode::Source;
        let editor = tab.editor.clone();
        let state = tab.view.clone();
        let mut find = self.find.take().unwrap_or_default();
        find.query = query.to_string();
        if source_mode {
            let Some(doc) = self.tabs[index].doc.as_ref() else {
                return;
            };
            find.search(&doc.text);
            if let (Some(editor), Some(range)) = (editor, find.current_range()) {
                editor.update(cx, |state, cx| state.set_selected_range(range, cx));
            }
            self.find = Some(find);
            cx.notify();
            return;
        }
        let color = cx.theme().accent.opacity(0.45);
        let rendered = state.read(cx).rendered_text();
        find.search(rendered.as_str());
        let ranges = find.matches.clone();
        let current = find.current_range();
        self.find = Some(find);
        state.update(cx, |state, cx| {
            let highlights: Vec<_> = ranges
                .iter()
                .cloned()
                .map(|range| RangeHighlight::new(range, color))
                .collect();
            state.set_range_highlights(highlights, cx).ok();
            if let Some(range) = current {
                state.reveal_range(range, cx).ok();
            }
        });
        cx.notify();
    }

    pub fn find_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else { return };
        let source_mode = tab.mode == ViewMode::Source;
        let editor = tab.editor.clone();
        let state = tab.view.clone();
        let Some(find) = self.find.as_mut() else {
            return;
        };
        if find.matches.is_empty() {
            return;
        }
        let range = if delta > 0 {
            find.next_match()
        } else {
            find.previous_match()
        };
        if let Some(range) = range {
            if source_mode {
                if let Some(editor) = editor {
                    editor.update(cx, |state, cx| state.set_selected_range(range, cx));
                }
            } else {
                state.update(cx, |state, cx| {
                    state.reveal_range(range, cx).ok();
                });
            }
            cx.notify();
        }
    }

    // ----------------------------------------------------------------- outline

    pub fn go_to_heading(&mut self, offset: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else { return };
        if tab.mode == ViewMode::Source {
            if let Some(editor) = tab.editor.clone() {
                let end = editor.read(cx).value().len().min(offset);
                editor.update(cx, |state, cx| state.set_selected_range(end..end, cx));
            }
            cx.notify();
            return;
        }
        let state = tab.view.clone();
        state.update(cx, |state, cx| {
            let rendered = state.rendered_text();
            let source_len = rendered.source().len();
            if offset >= source_len || !rendered.source().is_char_boundary(offset) {
                return;
            }
            let end = (offset + 1).min(source_len);
            if let Some(range) = rendered.range_for_source(offset..end) {
                state.reveal_range(range, cx).ok();
            }
        });
        cx.notify();
    }

    pub fn go_to_anchor(&mut self, anchor: &str, cx: &mut Context<Self>) {
        let Some(tab) = self.active_tab() else { return };
        let target = outline::slugify(anchor);
        let offset = tab
            .headings
            .iter()
            .find(|heading| outline::slugify(&heading.text) == target)
            .map(|heading| heading.offset);
        if let Some(offset) = offset {
            self.go_to_heading(offset, cx);
        }
    }

    // ----------------------------------------------------------------- palette

    pub fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        let root = self.tree.root().map(Path::to_path_buf);
        let prefill = self.palette_prefill.take().unwrap_or_default();
        {
            let palette = self.palette.get_or_insert_with(Palette::new);
            palette.scanning = true;
            palette.set_query(prefill.clone());
        }
        self.palette_input
            .update(cx, |state, cx| state.set_value(prefill, window, cx));

        if let Some(previous) = self.scan_cancel.take() {
            previous.store(true, Ordering::Relaxed);
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.scan_cancel = Some(cancel.clone());
        self.scan_generation += 1;
        let generation = self.scan_generation;

        cx.spawn(async move |this, cx| {
            let Some(root) = root else {
                this.update(cx, |this, cx| {
                    if let Some(palette) = this.palette.as_mut() {
                        palette.set_hits(Vec::new(), false);
                    }
                    cx.notify();
                })
                .ok();
                return;
            };
            let outcome = cx
                .background_spawn(async move {
                    let limits = Limits {
                        deadline: Some(Instant::now() + Duration::from_secs(3)),
                        ..Limits::default()
                    };
                    index::scan(&root, &cancel, limits)
                })
                .await;
            this.update(cx, |this, cx| {
                if this.scan_generation != generation {
                    return;
                }
                if let Some(palette) = this.palette.as_mut() {
                    palette.set_hits(outcome.hits, outcome.truncated);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();

        let handle = self.palette_input.read(cx).focus_handle(cx).clone();
        window.focus(&handle, cx);
        cx.notify();
    }

    pub fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            let handle = self.tree_focus.clone();
            window.focus(&handle, cx);
            cx.notify();
        }
    }

    fn palette_query(&mut self, query: &str, cx: &mut Context<Self>) {
        if let Some(palette) = self.palette.as_mut() {
            palette.set_query(query.to_string());
            cx.notify();
        }
    }

    pub fn palette_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(palette) = self.palette.as_mut() {
            palette.move_selection(delta);
            let selected = palette.selected_index();
            self.palette_scroll
                .scroll_to_item(selected, ScrollStrategy::Center);
            cx.notify();
        }
    }

    pub fn palette_accept(&mut self, cx: &mut Context<Self>) {
        let Some(palette) = self.palette.as_ref() else {
            return;
        };
        let root = self.tree.root().map(Path::to_path_buf);
        let query = palette.query.clone();
        let folder = palette.folder_candidate_in(root.as_deref());
        let hit = palette.selected_hit().map(|hit| hit.path.clone());
        self.palette = None;
        self.refocus_tree = true;

        if let Some(dir) = folder {
            self.open_root(dir, cx);
            return;
        }
        if paths::looks_like_path(&query) {
            let home = std::env::var_os("HOME").map(PathBuf::from);
            if let Ok(path) = paths::resolve(&query, root.as_deref(), home.as_deref()) {
                if path.is_file() {
                    self.open_path(&path, cx);
                    return;
                }
            }
        }
        if let Some(path) = hit {
            self.open_path(&path, cx);
        }
        cx.notify();
    }

    // -------------------------------------------------------------------- tree

    pub fn tree_click(&mut self, row: usize, cx: &mut Context<Self>) {
        let Some(node) = self.tree.row(row).cloned() else {
            return;
        };
        self.tree_selected = Some(row);
        if node.is_dir {
            self.tree.toggle_row(row);
            self.watch_dir(&node.path);
            cx.notify();
            return;
        }
        if node.is_markdown() {
            self.open_path(&node.path, cx);
        } else {
            cx.open_url(&format!("file://{}", node.path.display()));
        }
    }

    pub fn tree_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.tree.is_empty() {
            return;
        }
        let next = match self.tree_selected {
            Some(current) => {
                (current as isize + delta).clamp(0, self.tree.len() as isize - 1) as usize
            }
            None => 0,
        };
        self.tree_selected = Some(next);
        self.tree_scroll
            .scroll_to_item(next, ScrollStrategy::Center);
        cx.notify();
    }

    pub fn tree_collapse(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.tree_selected
            && let Some(parent) = self.tree.collapse_or_parent(row)
        {
            self.tree_selected = Some(parent);
            self.tree_scroll
                .scroll_to_item(parent, ScrollStrategy::Center);
            cx.notify();
        }
    }

    pub fn tree_expand(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.tree_selected
            && let Some(next) = self.tree.expand_or_child(row)
        {
            self.tree_selected = Some(next);
            self.tree_scroll
                .scroll_to_item(next, ScrollStrategy::Center);
            cx.notify();
        }
    }

    pub fn tree_activate(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.tree_selected {
            self.tree_click(row, cx);
        }
    }

    pub fn reveal_active(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.active_tab().map(|tab| tab.path.clone()) else {
            return;
        };
        cx.reveal_path(&path);
    }

    pub fn open_containing_folder(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.active_tab().map(|tab| tab.path.clone()) else {
            return;
        };
        if let Some(root) = self.tree.root()
            && paths::is_within(root, &path)
            && let Some(row) = self.tree.reveal(&path)
        {
            self.tree_selected = Some(row);
            cx.notify();
            return;
        }
        if let Some(parent) = path.parent() {
            self.open_root(parent.to_path_buf(), cx);
            if let Some(row) = self.tree.reveal(&path) {
                self.tree_selected = Some(row);
            }
        }
    }

    pub fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings_open {
            self.settings_open = false;
            cx.notify();
            return;
        }
        if self.notice.take().is_some() {
            cx.notify();
            return;
        }
        if self.palette.is_some() {
            self.close_palette(window, cx);
        } else {
            self.close_find(cx);
        }
    }
}

pub fn is_markdown(path: &Path) -> bool {
    folio_core::file_tree::is_markdown_path(path)
}

fn validate_new_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("Enter a file name.".to_string());
    }
    if name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err("The name cannot contain a path separator.".to_string());
    }
    if name.starts_with('.') {
        return Err("The name cannot start with a dot.".to_string());
    }
    if name.len() > 200 {
        return Err("The name is too long.".to_string());
    }
    if Path::new(name).file_name().and_then(|part| part.to_str()) != Some(name) {
        return Err("The name is not a plain file name.".to_string());
    }
    let path = Path::new(name);
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) => {
            if !folio_core::file_tree::MARKDOWN_EXTS
                .iter()
                .any(|known| known.eq_ignore_ascii_case(ext))
            {
                Err("Use a Markdown extension such as .md.".to_string())
            } else {
                Ok(name.to_string())
            }
        }
        None => Ok(format!("{name}.md")),
    }
}

fn today_string() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    // Howard Hinnant's civil-from-days algorithm, in UTC.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
