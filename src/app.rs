//! The reader window: file browser, tab strip, document pane, outline, find bar
//! and quick open.
//!
//! There is exactly one stateful view ([`Folio`]); everything else is a plain
//! element rebuilt from it. That keeps invalidation trivial (one `cx.notify()`),
//! and means a keystroke never walks a tree of entities.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::text::{
    FrontmatterPlugin, MarkdownExtensions, RangeHighlight, TaskToggleEvent, TextView, TextViewState,
};
use gpui_kit::component::theme::{ActiveTheme, Theme, ThemeMode};
use gpui_kit::base::StyledExt as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::component::{Icon, ResizableState, Sizable, TitleBar, h_resizable, resizable_panel};
use gpui_kit::*;

use folio::file_tree::{self, FileTree};
use folio::session::Session;
use folio::util;

use crate::model::{Find, OpenDoc, Palette};

/// Reading measure. Prose past this width is measurably harder to read, so a
/// full-screen window keeps the page centered instead of stretching it.
const READING_WIDTH: f32 = 780.0;
const SIDEBAR_MIN: f32 = 200.0;
const SIDEBAR_MAX: f32 = 520.0;
const OUTLINE_WIDTH: f32 = 236.0;
const OUTLINE_MIN: f32 = 160.0;
const OUTLINE_MAX: f32 = 420.0;
const TREE_ROW_HEIGHT: f32 = 26.0;

/// `FOLIO_TRACE=1` reports process start to first frame on stderr. Off by
/// default; used to keep startup honest.
static START: OnceLock<Instant> = OnceLock::new();
static TRACED_FIRST_FRAME: AtomicBool = AtomicBool::new(false);

pub fn mark_process_start() {
    let _ = START.set(Instant::now());
}

fn trace(message: impl FnOnce() -> String) {
    if std::env::var_os("FOLIO_TRACE").is_some() {
        eprintln!("[folio] {}", message());
    }
}
const TAB_STRIP_HEIGHT: f32 = 38.0;

actions!(
    folio,
    [
        Quit,
        OpenPalette,
        CloseOverlay,
        ToggleSidebar,
        ToggleOutline,
        ToggleTheme,
        ToggleHidden,
        Reload,
        NextTab,
        PreviousTab,
        CloseTab,
        BeginFind,
        FindNext,
        FindPrevious,
        RefreshTree,
        MoveSelectionDown,
        MoveSelectionUp,
        CollapseSelection,
        ExpandSelection,
        ActivateSelection,
        FirstTab,
        LastTab,
        PaletteDown,
        PaletteUp,
    ]
);

pub fn bind_keys(cx: &mut App) {
    use gpui_kit::KeyBinding;
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-p", OpenPalette, None),
        KeyBinding::new("cmd-shift-o", OpenPalette, None),
        KeyBinding::new("escape", CloseOverlay, None),
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-shift-b", ToggleOutline, None),
        KeyBinding::new("cmd-shift-l", ToggleTheme, None),
        KeyBinding::new("cmd-shift-.", ToggleHidden, None),
        KeyBinding::new("cmd-r", Reload, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("cmd-shift-]", NextTab, None),
        KeyBinding::new("cmd-shift-[", PreviousTab, None),
        KeyBinding::new("cmd-f", BeginFind, None),
        KeyBinding::new("cmd-g", FindNext, None),
        KeyBinding::new("cmd-shift-g", FindPrevious, None),
        KeyBinding::new("cmd-1", FirstTab, None),
        KeyBinding::new("cmd-9", LastTab, None),
        KeyBinding::new("down", MoveSelectionDown, Some("FileTree")),
        KeyBinding::new("up", MoveSelectionUp, Some("FileTree")),
        KeyBinding::new("left", CollapseSelection, Some("FileTree")),
        KeyBinding::new("right", ExpandSelection, Some("FileTree")),
        KeyBinding::new("enter", ActivateSelection, Some("FileTree")),
        KeyBinding::new("down", PaletteDown, Some("Palette")),
        KeyBinding::new("up", PaletteUp, Some("Palette")),
    ]);
}

pub fn install_menus(cx: &mut App) {
    use gpui_kit::{Menu, MenuItem};
    cx.set_menus([
        Menu::new("File").items([
            MenuItem::action("Quick Open…", OpenPalette),
            MenuItem::action("Reload Document", Reload),
            MenuItem::separator(),
            MenuItem::action("Close Tab", CloseTab),
            MenuItem::action("Quit Folio", Quit),
        ]),
        Menu::new("View").items([
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
            MenuItem::action("Toggle Outline", ToggleOutline),
            MenuItem::action("Toggle Theme", ToggleTheme),
            MenuItem::action("Show Hidden Files", ToggleHidden),
            MenuItem::separator(),
            MenuItem::action("Find in Document", BeginFind),
            MenuItem::action("Find Next", FindNext),
            MenuItem::action("Find Previous", FindPrevious),
        ]),
    ]);
}

struct Doc {
    model: OpenDoc,
    state: Entity<TextViewState>,
}

pub struct Folio {
    session: Session,
    tree: FileTree,
    tree_selected: Option<usize>,
    tree_focus: FocusHandle,
    docs: Vec<Doc>,
    active: Option<usize>,
    split: Entity<ResizableState>,
    sidebar_filter: Entity<InputState>,
    palette: Option<Palette>,
    palette_input: Entity<InputState>,
    find: Option<Find>,
    find_input: Entity<InputState>,
    /// Guards background palette scans against out-of-order completion.
    scan_generation: u64,
    /// Throttles window-geometry writes so dragging a window is one write per
    /// second, not one per frame.
    last_bounds_save: Instant,
    /// Markdown files edited elsewhere reload without a keystroke: a filesystem
    /// watcher appends changed paths, and a timer folds them into one update.
    watcher: Option<notify::RecommendedWatcher>,
    watched_dirs: HashSet<PathBuf>,
    pending_changes: Arc<Mutex<Vec<PathBuf>>>,
    /// Text to place in quick open when it next opens (the sidebar's folder
    /// button pre-fills the current root so it can be edited).
    palette_prefill: Option<String>,
    /// Set when a modal path took focus away and the tree should get it back
    /// on the next frame (there is no `&mut Window` inside input events).
    refocus_tree: bool,
    /// Documents queued before the first frame. Opening them inside `new`
    /// would paint nothing until a large file finished parsing, so the window
    /// appears first and the reader fills in on the next frame.
    pending_opens: Vec<PathBuf>,
    pending_active: Option<PathBuf>,
    pending_retarget: bool,
    /// Keeps the keyboard-selected row and palette entry on screen.
    tree_scroll: UniformListScrollHandle,
    palette_scroll: UniformListScrollHandle,
}

impl Folio {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        mut session: Session,
        initial: Option<PathBuf>,
    ) -> Self {
        let tree_focus = cx.focus_handle();
        let sidebar_filter =
            cx.new(|cx| InputState::new(window, cx).placeholder("Filter files"));
        let palette_input =
            cx.new(|cx| InputState::new(window, cx).placeholder("Open a markdown file…"));
        let find_input = cx.new(|cx| InputState::new(window, cx).placeholder("Find in document"));

        let mut tree = FileTree::new();
        tree.set_show_hidden(false);

        let initial_file = initial.as_ref().filter(|path| path.is_file()).cloned();
        let root = match initial.as_ref().filter(|path| path.is_dir()) {
            Some(dir) => Some(dir.clone()),
            None => session
                .root
                .clone()
                .or_else(|| {
                    initial_file
                        .as_ref()
                        .and_then(|path| path.parent().map(Path::to_path_buf))
                })
                .or_else(|| std::env::var_os("HOME").map(PathBuf::from)),
        };
        if let Some(root) = &root {
            tree.set_root(root.clone());
            session.note_root(root);
        }

        cx.subscribe(&palette_input, |this, _, event: &InputEvent, cx| match event {
            InputEvent::Change => {
                let query = this.palette_input.read(cx).value().to_string();
                this.palette_query(&query, cx);
            }
            InputEvent::PressEnter { .. } => this.palette_accept(cx),
            _ => {}
        })
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

        let mut this = Self {
            split: cx.new(|_| ResizableState::default()),
            session,
            tree,
            tree_selected: None,
            tree_focus,
            docs: Vec::new(),
            active: None,
            sidebar_filter,
            palette: None,
            palette_input,
            find: None,
            find_input,
            scan_generation: 0,
            last_bounds_save: Instant::now() - Duration::from_secs(5),
            watcher,
            watched_dirs: HashSet::new(),
            pending_changes,
            refocus_tree: false,
            palette_prefill: None,
            pending_opens: Vec::new(),
            pending_active: None,
            pending_retarget: false,
            tree_scroll: UniformListScrollHandle::new(),
            palette_scroll: UniformListScrollHandle::new(),
        };

        if let Some(path) = initial_file {
            this.pending_opens = vec![path];
            this.pending_retarget = true;
        } else {
            this.pending_opens = this.session.open_files.clone();
            this.pending_retarget = false;
        }
        this.pending_active = this.session.active.clone();
        if let Some(first) = this.pending_opens.first().cloned() {
            this.tree_selected = this.tree.reveal(&first);
        }
        if let Some(root) = this.tree.root().map(Path::to_path_buf) {
            this.watch_dir(&root);
        }
        this.start_file_watcher(cx);
        if this.pending_opens.is_empty() {
            this.persist_session();
        }
        let handle = this.tree_focus.clone();
        window.focus(&handle, cx);
        this
    }

    /// Poll the watcher's buffer on a slow timer and fold the batch into one
    /// UI update. Cheaper and far safer than touching GPUI state from the
    /// watcher thread.
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

    fn watch_dir(&mut self, dir: &Path) {
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
        for index in 0..self.docs.len() {
            if !paths.iter().any(|path| *path == self.docs[index].model.path) {
                continue;
            }
            if self.docs[index].model.reload_if_stale() {
                let display = self.docs[index].model.display.clone();
                self.docs[index]
                    .state
                    .update(cx, |state, cx| state.set_text(&display, cx));
                changed = true;
            }
        }
        // Directory listings change too (new files, renames). A refresh only
        // re-reads the directories the user expanded, so it stays cheap.
        self.tree.refresh();
        if changed {
            cx.notify();
        }
    }

    // ---------------------------------------------------------------- documents

    fn active_doc(&self) -> Option<&Doc> {
        self.active.and_then(|index| self.docs.get(index))
    }

    fn active_doc_mut(&mut self) -> Option<&mut Doc> {
        self.active.and_then(|index| self.docs.get_mut(index))
    }

    /// Open a document, focusing its tab when it is already open.
    ///
    /// `retarget` moves the sidebar to the file's folder when the file lives
    /// outside the current root, so the tree always shows what you are reading.
    fn open_path(&mut self, path: &Path, retarget: bool, cx: &mut Context<Self>) {
        if retarget
            && !self
                .tree
                .root()
                .is_some_and(|root| path.starts_with(root))
            && let Some(parent) = path.parent()
        {
            self.tree.set_root(parent.to_path_buf());
            self.session.note_root(parent);
        }
        if let Some(index) = self.docs.iter().position(|doc| doc.model.path == path) {
            self.active = Some(index);
        } else {
            let model = {
                let started = Instant::now();
                let model = OpenDoc::load(path.to_path_buf());
                trace(|| {
                    format!(
                        "load {} ({} KB) in {} ms",
                        path.display(),
                        model.source.len() / 1024,
                        started.elapsed().as_millis()
                    )
                });
                model
            };
            let display = model.display.clone();
            let started = Instant::now();
            let state = cx.new(|cx| TextViewState::markdown(&display, cx));
            trace(|| format!("parse markdown in {} ms", started.elapsed().as_millis()));
            self.docs.push(Doc { model, state });
            self.active = Some(self.docs.len() - 1);
        }
        self.tree_selected = self.tree.reveal(path);
        if let Some(row) = self.tree_selected {
            self.tree_scroll.scroll_to_item(row, ScrollStrategy::Center);
        }
        if let Some(parent) = path.parent() {
            self.watch_dir(parent);
        }
        self.find = None;
        self.persist_session();
        cx.notify();
    }

    fn close_tab_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.docs.len() {
            return;
        }
        self.docs.remove(index);
        self.active = match self.active {
            None => None,
            Some(_) if self.docs.is_empty() => None,
            Some(active) if active > index => Some(active - 1),
            Some(active) if active == index => Some(active.min(self.docs.len() - 1)),
            Some(active) => Some(active),
        };
        self.find = None;
        self.persist_session();
        cx.notify();
    }

    fn close_active(&mut self, cx: &mut Context<Self>) {
        if let Some(active) = self.active {
            self.close_tab_at(active, cx);
        }
    }

    fn cycle_tab(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.docs.is_empty() {
            return;
        }
        let len = self.docs.len() as isize;
        let active = self.active.unwrap_or(0) as isize;
        self.active = Some((active + delta).rem_euclid(len) as usize);
        self.find = None;
        self.persist_session();
        cx.notify();
    }

    fn select_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.docs.len() {
            return;
        }
        if self.active == Some(index) {
            if let Some(doc) = self.docs.get_mut(index) {
                doc.model.reload_if_stale();
            }
            return;
        }
        if let Some(doc) = self.docs.get_mut(index) {
            doc.model.reload_if_stale();
        }
        self.active = Some(index);
        self.find = None;
        self.persist_session();
        cx.notify();
    }

    fn reload_active(&mut self, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc_mut() else {
            return;
        };
        doc.model.read();
        let display = doc.model.display.clone();
        doc.state.update(cx, |state, cx| state.set_text(&display, cx));
        cx.notify();
    }

    fn toggle_task(&mut self, event: &TaskToggleEvent, cx: &mut Context<Self>) {
        let Some(index) = self.active else { return };
        let Some(doc) = self.docs.get_mut(index) else {
            return;
        };
        let marker = event.marker_range().start;
        let checked = event.checked();
        if !folio::outline::set_checkbox(&mut doc.model.source, marker, checked) {
            return;
        }
        doc.model.reslice();
        let display = doc.model.display.clone();
        let source = doc.model.source.clone();
        doc.state.update(cx, |state, cx| state.set_text(&display, cx));
        let path = doc.model.path.clone();
        // The reader never blocks on the disk; a failed write leaves the tab
        // showing the intended state until the next reload.
        std::thread::spawn(move || {
            let _ = std::fs::write(&path, source);
        });
        cx.notify();
    }

    fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = if cx.theme().mode.is_dark() {
            ThemeMode::Light
        } else {
            ThemeMode::Dark
        };
        Theme::change(next, Some(window), cx);
        self.session.theme = if next.is_dark() {
            folio::session::ThemePreference::Dark
        } else {
            folio::session::ThemePreference::Light
        };
        self.persist_session();
        cx.notify();
    }

    fn toggle_sidebar(&mut self, cx: &mut Context<Self>) {
        self.session.sidebar_visible = !self.session.sidebar_visible;
        self.persist_session();
        cx.notify();
    }

    fn toggle_outline(&mut self, cx: &mut Context<Self>) {
        self.session.outline_open = !self.session.outline_open;
        self.persist_session();
        cx.notify();
    }

    fn persist_session(&mut self) {
        self.session.open_files = self.docs.iter().map(|doc| doc.model.path.clone()).collect();
        self.session.active = self.active_doc().map(|doc| doc.model.path.clone());
        self.session.save();
    }

    // -------------------------------------------------------------------- find

    fn begin_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.is_none() {
            self.find = Some(Find::default());
        }
        let handle = self.find_input.read(cx).focus_handle(cx).clone();
        window.focus(&handle, cx);
        cx.notify();
    }

    fn close_find(&mut self, cx: &mut Context<Self>) {
        if self.find.take().is_some() {
            if let Some(doc) = self.active_doc() {
                let state = doc.state.clone();
                state.update(cx, |state, cx| state.clear_range_highlights(cx));
            }
            // The find input is about to be unmounted; leaving focus on it
            // would strand every subsequent key binding.
            self.refocus_tree = true;
            cx.notify();
        }
    }

    fn update_find_query(&mut self, query: &str, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc() else { return };
        let state = doc.state.clone();
        let color = cx.theme().accent.opacity(0.45);
        let mut find = self.find.take().unwrap_or_default();
        find.query = query.to_string();
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

    fn find_step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc() else { return };
        let state = doc.state.clone();
        let Some(find) = self.find.as_mut() else { return };
        if find.matches.is_empty() {
            return;
        }
        let range = if delta > 0 {
            find.next()
        } else {
            find.previous()
        };
        if let Some(range) = range {
            state.update(cx, |state, cx| {
                state.reveal_range(range, cx).ok();
            });
            cx.notify();
        }
    }

    // ----------------------------------------------------------------- outline

    fn go_to_heading(&mut self, offset: usize, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc() else { return };
        let state = doc.state.clone();
        state.update(cx, |state, cx| {
            let rendered = state.rendered_text();
            let source_len = rendered.source().len();
            if offset >= source_len {
                return;
            }
            let end = (offset + 1).min(source_len);
            if !rendered.source().is_char_boundary(offset) {
                return;
            }
            if let Some(range) = rendered.range_for_source(offset..end) {
                state.reveal_range(range, cx).ok();
            }
        });
        cx.notify();
    }

    fn go_to_anchor(&mut self, anchor: &str, cx: &mut Context<Self>) {
        let Some(doc) = self.active_doc() else { return };
        let target = folio::outline::slugify(anchor);
        let offset = doc
            .model
            .headings
            .iter()
            .find(|heading| folio::outline::slugify(&heading.text) == target)
            .map(|heading| heading.offset);
        if let Some(offset) = offset {
            self.go_to_heading(offset, cx);
        }
    }

    // ----------------------------------------------------------------- palette

    fn open_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.find = None;
        let Some(root) = self.tree.root().map(Path::to_path_buf) else {
            return;
        };
        let prefill = self.palette_prefill.take().unwrap_or_default();
        {
            let palette = self.palette.get_or_insert_with(Palette::new);
            palette.scanning = true;
            palette.set_query(prefill.clone());
        }
        self.palette_input
            .update(cx, |state, cx| state.set_value(prefill, window, cx));
        self.scan_generation += 1;
        let generation = self.scan_generation;
        cx.spawn(async move |this, cx| {
            let (hits, truncated) = cx
                .background_spawn(async move { file_tree::scan_markdown_files(&root, 8_000) })
                .await;
            this.update(cx, |this, cx| {
                if this.scan_generation != generation {
                    return;
                }
                if let Some(palette) = this.palette.as_mut() {
                    palette.set_hits(hits, truncated);
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

    fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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

    fn palette_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        if let Some(palette) = self.palette.as_mut() {
            palette.move_selection(delta);
            let selected = palette.selected_index();
            self.palette_scroll
                .scroll_to_item(selected, ScrollStrategy::Center);
            cx.notify();
        }
    }

    fn palette_accept(&mut self, cx: &mut Context<Self>) {
        if let Some(dir) = self.palette.as_ref().and_then(|palette| palette.folder_candidate()) {
            self.palette = None;
            self.refocus_tree = true;
            self.open_root(dir, cx);
            return;
        }
        let Some(path) = self
            .palette
            .as_ref()
            .and_then(|palette| palette.selected_hit())
            .map(|hit| hit.path.clone())
        else {
            return;
        };
        self.palette = None;
        self.refocus_tree = true;
        self.open_path(&path, true, cx);
    }

    // -------------------------------------------------------------------- tree

    fn tree_click(&mut self, row: usize, cx: &mut Context<Self>) {
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
            self.open_path(&node.path, true, cx);
        } else {
            cx.open_url(&format!("file://{}", node.path.display()));
        }
    }

    fn tree_move(&mut self, delta: isize, cx: &mut Context<Self>) {
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
        self.tree_scroll.scroll_to_item(next, ScrollStrategy::Center);
        cx.notify();
    }

    fn tree_collapse(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.tree_selected
            && let Some(parent) = self.tree.collapse_or_parent(row)
        {
            self.tree_selected = Some(parent);
            self.tree_scroll.scroll_to_item(parent, ScrollStrategy::Center);
            cx.notify();
        }
    }

    fn tree_expand(&mut self, cx: &mut Context<Self>) {
        if let Some(row) = self.tree_selected
            && let Some(next) = self.tree.expand_or_child(row)
        {
            self.tree_selected = Some(next);
            self.tree_scroll.scroll_to_item(next, ScrollStrategy::Center);
            cx.notify();
        }
    }

    fn open_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.tree.set_root(root.clone());
        self.watch_dir(&root);
        self.tree_selected = None;
        self.session.note_root(&root);
        self.persist_session();
        cx.notify();
    }

    // ------------------------------------------------------------------ render

    fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let sidebar_bg = theme.sidebar;
        let border = theme.sidebar_border;
        let muted = theme.muted_foreground;
        let foreground = theme.foreground;
        let hover = theme.list_hover;
        let selected_bg = theme.list_active;
        let accent = theme.accent;

        let weak = cx.weak_entity();
        let entity = cx.entity();
        let rows = self.tree.len();
        let selected = self.tree_selected;
        let open_paths: Vec<PathBuf> = self.docs.iter().map(|doc| doc.model.path.clone()).collect();

        let list = uniform_list("file-tree", rows, move |range, _window, cx| {
            let this = entity.read(cx);
            range
                .filter_map(|index| {
                    let node = this.tree.row(index)?;
                    let is_selected = selected == Some(index);
                    let is_open = open_paths.contains(&node.path);
                    let (icon, icon_color) = if node.is_dir {
                        let icon = if node.expanded {
                            gpui_kit::assets::IconName::FolderOpen
                        } else {
                            gpui_kit::assets::IconName::Folder
                        };
                        (icon, if is_open { accent } else { muted })
                    } else if node.is_markdown() {
                        (
                            gpui_kit::assets::IconName::FileText,
                            if is_open { accent } else { muted },
                        )
                    } else {
                        (gpui_kit::assets::IconName::File, muted)
                    };
                    let label_color = if node.is_dir || node.is_markdown() {
                        foreground
                    } else {
                        muted
                    };
                    let weak = weak.clone();
                    let name = node.name.clone();
                    let depth = node.depth;
                    Some(
                        div()
                            .id(("tree-row", index))
                            .h(px(TREE_ROW_HEIGHT))
                            .w_full()
                            .flex()
                            .items_center()
                            .gap_2()
                            .pl(px(8.0 + depth as f32 * 13.0))
                            .pr_2()
                            .rounded_sm()
                            .cursor_pointer()
                            .when(is_selected, |row| row.bg(selected_bg))
                            .when(!is_selected, |row| row.hover(|row| row.bg(hover)))
                            .child(
                                if node.is_dir {
                                    div()
                                        .w(px(12.0))
                                        .flex()
                                        .justify_center()
                                        .child(
                                            Icon::new(if node.expanded {
                                                gpui_kit::assets::IconName::ChevronDown
                                            } else {
                                                gpui_kit::assets::IconName::ChevronRight
                                            })
                                            .text_color(muted)
                                            .with_size(px(12.)),
                                        )
                                        .into_any_element()
                                } else {
                                    div().w(px(12.0)).into_any_element()
                                },
                            )
                            .child(Icon::new(icon).text_color(icon_color).with_size(px(14.)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_sm()
                                    .text_color(label_color)
                                    .child(name),
                            )
                            .on_click(move |_, _window, cx| {
                                weak.update(cx, |this, cx| this.tree_click(index, cx)).ok();
                            }),
                    )
                })
                .collect()
        })
        .flex_1()
        .min_h_0()
        .track_scroll(&self.tree_scroll);

        div()
            .v_flex()
            .id("sidebar")
            .h_full()
            .w_full()
            .bg(sidebar_bg)
            .border_r_1()
            .border_color(border)
            .key_context("FileTree")
            .track_focus(&self.tree_focus)
            .child(
                div()
                    .v_flex()
                    .w_full()
                    .px_3()
                    .pt_3()
                    .pb_2()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_xs()
                                    .font_semibold()
                                    .text_color(muted)
                                    .child("FILES"),
                            )
                            .child(div().text_xs().text_color(muted).child(format!("{rows}"))),
                    )
                    .child(Input::new(&self.sidebar_filter)),
            )
            .child(list)
            .child({
                let weak = cx.weak_entity();
                let root = self.tree.root().map(Path::to_path_buf);
                let label = root
                    .as_ref()
                    .map(|root| {
                        root.file_name()
                            .map(|name| name.to_string_lossy().to_string())
                            .unwrap_or_else(|| root.display().to_string())
                    })
                    .unwrap_or_else(|| "Open a folder".to_string());
                let prefill = root
                    .as_ref()
                    .map(|root| util::display_path(root))
                    .unwrap_or_default();
                div()
                    .id("sidebar-root")
                    .w_full()
                    .px_2()
                    .h(px(30.0))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |row| row.bg(hover))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::FolderOpen)
                            .text_color(muted)
                            .with_size(px(13.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .truncate()
                            .text_xs()
                            .text_color(muted)
                            .child(label),
                    )
                    .child(div().text_xs().text_color(muted).child("⌘P"))
                    .on_click(move |_, window, cx| {
                        weak.update(cx, |this, cx| {
                            this.palette_prefill = Some(prefill.clone());
                            this.open_palette(window, cx);
                        })
                        .ok();
                    })
            })
            .into_any_element()
    }

    fn render_tabs(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let bar = theme.tab_bar;
        let border = theme.border;
        let active_bg = theme.tab_active;
        let active_fg = theme.tab_active_foreground;
        let fg = theme.tab_foreground;
        let muted = theme.muted_foreground;
        let hover = theme.list_hover;
        let weak = cx.weak_entity();

        // The strip scrolls horizontally so a dozen open tabs never squeeze
        // labels into ellipses; stats stay pinned on the right.
        let mut strip = div()
            .id("tab-strip")
            .flex()
            .flex_1()
            .min_w_0()
            .overflow_x_scroll()
            .px_2()
            .gap_1()
            .items_center();

        // Only duplicate file names need a parent hint; unique names stay short.
        let names: Vec<String> = self.docs.iter().map(|doc| doc.model.title()).collect();
        for (index, doc) in self.docs.iter().enumerate() {
            let is_active = self.active == Some(index);
            let duplicate = names.iter().filter(|name| **name == names[index]).count() > 1;
            let label = util::tab_label(&doc.model.path, duplicate);
            let weak_row = weak.clone();
            let weak_close = weak.clone();
            strip = strip.child(
                div()
                    .id(("tab", index))
                    .flex()
                    .items_center()
                    .max_w(px(220.0))
                    .h(px(28.0))
                    .px_2()
                    .gap_2()
                    .rounded_md()
                    .cursor_pointer()
                    .when(is_active, |tab| tab.bg(active_bg))
                    .when(!is_active, |tab| tab.hover(|tab| tab.bg(hover)))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::FileText)
                            .text_color(if is_active { active_fg } else { muted })
                            .with_size(px(13.)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .overflow_hidden()
                            .text_sm()
                            .text_color(if is_active { active_fg } else { fg })
                            .child(label),
                    )
                    .child(
                        div()
                            .id(("tab-close", index))
                            .h(px(18.0))
                            .w(px(18.0))
                            .rounded_sm()
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|close| close.bg(border))
                            .child(
                                Icon::new(gpui_kit::assets::IconName::X)
                                    .text_color(muted)
                                    .with_size(px(12.)),
                            )
                            .on_click(move |_, _window, cx| {
                                cx.stop_propagation();
                                weak_close
                                    .update(cx, |this, cx| this.close_tab_at(index, cx))
                                    .ok();
                            }),
                    )
                    .on_click(move |_, _window, cx| {
                        weak_row.update(cx, |this, cx| this.select_tab(index, cx)).ok();
                    }),
            );
        }

        let stats = self
            .active_doc()
            .map(|doc| {
                format!(
                    "{} · {} headings",
                    util::human_bytes(doc.model.bytes),
                    doc.model.headings.len()
                )
            })
            .unwrap_or_default();

        div()
            .flex()
            .h(px(TAB_STRIP_HEIGHT))
            .w_full()
            .items_center()
            .bg(bar)
            .border_b_1()
            .border_color(border)
            .child(strip)
            .child(div().px_2().text_xs().text_color(muted).child(stats))
            .into_any_element()
    }

    fn render_document(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;

        let Some(doc) = self.active_doc() else {
            return self.render_empty_state(cx);
        };
        if let Some(error) = doc.model.error.clone() {
            return div()
                .v_flex()
                .size_full()
                .items_center()
                .justify_center()
                .gap_2()
                .child(div().text_lg().child("This file could not be read"))
                .child(div().text_sm().text_color(muted).child(error))
                .into_any_element();
        }

        let weak = cx.weak_entity();
        let dir = doc.model.dir().to_path_buf();
        let link_dir = dir.clone();
        let weak_task = weak.clone();
        let weak_link = weak.clone();

        div()
            .v_flex()
            .h_full()
            .w_full()
            .max_w(px(READING_WIDTH))
            .min_w_0()
            .child(
                TextView::new(&doc.state)
                    .selectable(true)
                    .scrollable(true)
                    .px_8()
                    .pt_5()
                    .pb_24()
                    .markdown_extensions(
                        MarkdownExtensions::default().frontmatter().raw_html(true),
                    )
                    .plugin(FrontmatterPlugin::new())
                    .image_source(move |uri: &SharedUri| {
                        let uri = uri.to_string();
                        match folio::outline::resolve_local(&dir, &uri) {
                            Some(path) => ImageSource::from(path),
                            None => ImageSource::from(uri),
                        }
                    })
                    .on_link_click(move |url, _event, _window, cx| {
                        let Some(path) = folio::outline::resolve_local(&link_dir, url.as_ref())
                        else {
                            if let Some(anchor) = url.strip_prefix('#') {
                                let anchor = anchor.to_string();
                                weak_link
                                    .update(cx, |this, cx| this.go_to_anchor(&anchor, cx))
                                    .ok();
                            } else {
                                cx.open_url(url.as_ref());
                            }
                            return;
                        };
                        if path.is_file() {
                            weak_link
                                .update(cx, |this, cx| this.open_path(&path, true, cx))
                                .ok();
                        } else {
                            cx.open_url(&format!("file://{}", path.display()));
                        }
                    })
                    .on_task_toggle(move |event, _window, cx| {
                        weak_task
                            .update(cx, |this, cx| this.toggle_task(event, cx))
                            .ok();
                    }),
            )
            .into_any_element()
    }

    fn render_empty_state(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let foreground = theme.foreground;
        let hover = theme.list_hover;
        let weak = cx.weak_entity();

        let mut places = div().v_flex().gap_1().items_center();
        for (index, (label, path)) in file_tree::places().into_iter().take(5).enumerate() {
            let weak = weak.clone();
            let target = path.clone();
            let display = folio::util::display_path(&path);
            places = places.child(
                div()
                    .id(("place", index))
                    .px_3()
                    .h(px(28.0))
                    .rounded_md()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .hover(move |row| row.bg(hover))
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(format!("{label}  ·  {display}")),
                    )
                    .on_click(move |_, _window, cx| {
                        weak.update(cx, |this, cx| this.open_root(target.clone(), cx))
                            .ok();
                    }),
            );
        }

        div()
            .v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_6()
            .child(
                div()
                    .v_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_2xl()
                            .font_semibold()
                            .text_color(foreground)
                            .child("Folio"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child("Markdown, rendered properly."),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child("⌘P quick open  ·  ⌘B sidebar  ·  ⌘F find  ·  ⌘⇧B outline"),
            )
            .child(places)
            .into_any_element()
    }

    fn render_outline(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let foreground = theme.foreground;
        let border = theme.border;
        let hover = theme.list_hover;
        let weak = cx.weak_entity();

        let mut panel = div()
            .v_flex()
            .id("outline")
            .h_full()
            .w_full()
            .overflow_y_scroll()
            .border_l_1()
            .border_color(border)
            .py_3()
            .px_2()
            .child(
                div()
                    .px_2()
                    .pb_2()
                    .text_xs()
                    .font_semibold()
                    .text_color(muted)
                    .child("OUTLINE"),
            );

        if let Some(doc) = self.active_doc() {
            if doc.model.headings.is_empty() {
                panel = panel.child(div().px_2().text_sm().text_color(muted).child("No headings"));
            }
            for (index, heading) in doc.model.headings.iter().enumerate() {
                let offset = heading.offset;
                let weak = weak.clone();
                let text = heading.text.clone();
                let level = heading.level;
                panel = panel.child(
                    div()
                        .id(("outline-item", index))
                        .pl(px(8.0 + (level.saturating_sub(1)) as f32 * 10.0))
                        .pr_2()
                        .py_1()
                        .rounded_sm()
                        .cursor_pointer()
                        .hover(move |row| row.bg(hover))
                        .child(
                            div()
                                .text_sm()
                                .when(level <= 2, |text| text.font_medium())
                                .text_color(if level <= 2 { foreground } else { muted })
                                .overflow_hidden()
                                .child(text),
                        )
                        .on_click(move |_, _window, cx| {
                            weak.update(cx, |this, cx| this.go_to_heading(offset, cx))
                                .ok();
                        }),
                );
            }
        }

        panel.into_any_element()
    }

    fn render_find_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.find.is_none() {
            return div().into_any_element();
        }
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let border = theme.border;
        let hover = theme.list_hover;
        let bar = theme.tab_bar;
        let count = self
            .find
            .as_ref()
            .map(|find| {
                if find.matches.is_empty() {
                    "no matches".to_string()
                } else {
                    format!("{}/{}", find.current + 1, find.matches.len())
                }
            })
            .unwrap_or_default();
        let weak_prev = cx.weak_entity();
        let weak_next = cx.weak_entity();

        div()
            .flex()
            .w_full()
            .px_4()
            .py_2()
            .gap_2()
            .items_center()
            .bg(bar)
            .border_b_1()
            .border_color(border)
            .child(div().flex_1().child(Input::new(&self.find_input)))
            .child(div().text_xs().text_color(muted).child(count))
            .child(
                div()
                    .id("find-prev")
                    .p_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |row| row.bg(hover))
                    .child(Icon::new(gpui_kit::assets::IconName::ChevronUp).with_size(px(14.)))
                    .on_click(move |_, _window, cx| {
                        weak_prev.update(cx, |this, cx| this.find_step(-1, cx)).ok();
                    }),
            )
            .child(
                div()
                    .id("find-next")
                    .p_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .hover(move |row| row.bg(hover))
                    .child(Icon::new(gpui_kit::assets::IconName::ChevronDown).with_size(px(14.)))
                    .on_click(move |_, _window, cx| {
                        weak_next.update(cx, |this, cx| this.find_step(1, cx)).ok();
                    }),
            )
            .into_any_element()
    }

    fn render_palette(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.palette.is_none() {
            return div().into_any_element();
        }
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let border = theme.border;
        // Floating panels need an opaque fill: `theme.popover` is translucent
        // in the shipped themes and lets the page show through.
        let panel_bg = theme.title_bar;
        let selected_bg = theme.list_active;
        let foreground = theme.foreground;

        let entity = cx.entity();
        let weak = cx.weak_entity();
        let weak_close = cx.weak_entity();
        let selected = self.palette.as_ref().map(|p| p.selected_index()).unwrap_or(0);
        let count = self.palette.as_ref().map(|p| p.len()).unwrap_or(0);
        let scanning = self.palette.as_ref().is_some_and(|p| p.scanning);
        let truncated = self.palette.as_ref().is_some_and(|p| p.truncated);
        let folder = self
            .palette
            .as_ref()
            .and_then(|palette| palette.folder_candidate());
        let weak_folder = cx.weak_entity();
        let folder_row = match folder {
            Some(dir) => {
                let display = folio::util::display_path(&dir);
                let label = format!("Open folder  ·  {display}");
                div()
                    .id("palette-folder")
                    .h(px(30.0))
                    .w_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .hover(|row| row.bg(selected_bg))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::FolderOpen)
                            .text_color(muted)
                            .with_size(px(13.)),
                    )
                    .child(div().text_sm().text_color(foreground).truncate().child(label))
                    .on_click(move |_, _window, cx| {
                        let dir = dir.clone();
                        weak_folder
                            .update(cx, |this, cx| {
                                this.palette = None;
                                this.refocus_tree = true;
                                this.open_root(dir, cx);
                            })
                            .ok();
                    })
                    .into_any_element()
            }
            None => div().into_any_element(),
        };

        let results = uniform_list("palette-results", count, move |range, _window, cx| {
            let this = entity.read(cx);
            let Some(palette) = this.palette.as_ref() else {
                return Vec::new();
            };
            range
                .filter_map(|index| {
                    let hit = palette.result_at(index)?;
                    let is_selected = index == selected;
                    let weak = weak.clone();
                    let path = hit.path.clone();
                    let label = hit.relative.clone();
                    Some(
                        div()
                            .id(("palette-hit", index))
                            .h(px(30.0))
                            .w_full()
                            .px_3()
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .when(is_selected, |row| row.bg(selected_bg))
                            .child(
                                Icon::new(gpui_kit::assets::IconName::FileText)
                                    .text_color(muted)
                                    .with_size(px(13.)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .text_sm()
                                    .text_color(foreground)
                                    .truncate()
                                    .child(label),
                            )
                            .on_click(move |_, _window, cx| {
                                weak.update(cx, |this, cx| {
                                    this.palette = None;
                                    this.open_path(&path, true, cx);
                                })
                                .ok();
                            }),
                    )
                })
                .collect()
        })
        .flex_1()
        .min_h_0()
        .track_scroll(&self.palette_scroll);

        let footer = if scanning {
            "Scanning…".to_string()
        } else if truncated {
            format!("{count} results (search capped)")
        } else {
            format!("{count} results")
        };

        div()
            .id("palette-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .v_flex()
            .items_center()
            .pt(px(96.0))
            .bg(black().opacity(0.28))
            .on_click(move |_, window, cx| {
                weak_close
                    .update(cx, |this, cx| this.close_palette(window, cx))
                    .ok();
            })
            .child(
                div()
                    .v_flex()
                    .key_context("Palette")
                    .id("palette-panel")
                    .w(px(640.0))
                    .h(px(440.0))
                    .rounded_lg()
                    .bg(panel_bg)
                    .text_color(foreground)
                    .border_1()
                    .border_color(border)
                    .overflow_hidden()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .border_b_1()
                            .border_color(border)
                            .child(Input::new(&self.palette_input)),
                    )
                    .child(folder_row)
                    .child(results)
                    .child(
                        div()
                            .px_3()
                            .py_2()
                            .border_t_1()
                            .border_color(border)
                            .text_xs()
                            .text_color(muted)
                            .child(footer),
                    ),
            )
            .into_any_element()
    }
}

impl Render for Folio {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let background = theme.background;
        let foreground = theme.foreground;
        let muted = theme.muted_foreground;
        let border = theme.border;

        if !TRACED_FIRST_FRAME.swap(true, Ordering::Relaxed)
            && let Some(start) = START.get()
        {
            trace(|| format!("first frame at {} ms", start.elapsed().as_millis()));
        }

        if !self.pending_opens.is_empty() {
            let opens = std::mem::take(&mut self.pending_opens);
            let active = self.pending_active.take();
            let retarget = std::mem::take(&mut self.pending_retarget);
            for (index, path) in opens.iter().enumerate() {
                self.open_path(path, retarget && index == 0, cx);
            }
            if let Some(active) = active
                && let Some(index) = self.docs.iter().position(|doc| doc.model.path == active)
            {
                self.active = Some(index);
            }
            if let Some(path) = self.active_doc().map(|doc| doc.model.path.clone()) {
                self.tree_selected = self.tree.reveal(&path);
            }
            self.persist_session();
            self.refocus_tree = true;
            cx.notify();
        }

        if self.refocus_tree {
            self.refocus_tree = false;
            let handle = self.tree_focus.clone();
            window.focus(&handle, cx);
        }

        let title = self
            .active_doc()
            .map(|doc| doc.model.title())
            .unwrap_or_else(|| "Folio".to_string());
        window.set_window_title(&format!("{title} — Folio"));

        // Remember window geometry, throttled. Reading it every frame is cheap;
        // writing it every frame is not.
        if self.last_bounds_save.elapsed() > Duration::from_millis(900)
            && let WindowBounds::Windowed(bounds) = window.window_bounds()
        {
            let rect = folio::session::WindowRect {
                x: f32::from(bounds.origin.x),
                y: f32::from(bounds.origin.y),
                width: f32::from(bounds.size.width),
                height: f32::from(bounds.size.height),
            };
            if self.session.window != Some(rect) {
                self.session.window = Some(rect);
                self.session.save();
                self.last_bounds_save = Instant::now();
            }
        }

        let sidebar_width = self.session.sidebar_width;
        let sidebar_visible = self.session.sidebar_visible;
        let root_label = self
            .tree
            .root()
            .map(folio::util::display_path)
            .unwrap_or_else(|| "No folder open".to_string());

        let header = div()
            .flex()
            .w_full()
            .h_full()
            .px_3()
            .gap_3()
            .items_center()
            .child(div().text_sm().font_medium().text_color(foreground).child(title))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .text_xs()
                    .text_color(muted)
                    .child(root_label),
            );

        let sidebar_panel = if sidebar_visible {
            resizable_panel()
                .size(px(sidebar_width.clamp(SIDEBAR_MIN, SIDEBAR_MAX)))
                .size_range(px(SIDEBAR_MIN)..px(SIDEBAR_MAX))
                .child(self.render_sidebar(cx))
        } else {
            resizable_panel().size(px(0.0)).visible(false)
        };

        let weak_resize = cx.weak_entity();
        let body = h_resizable("sidebar-split")
            .with_state(&self.split)
            .child(sidebar_panel)
            .child(
                resizable_panel().child(
                    div()
                        .v_flex()
                        .size_full()
                        .min_w_0()
                        .child(self.render_tabs(cx))
                        .child(self.render_find_bar(cx))
                        .child(
                            div()
                                .flex_1()
                                .min_h_0()
                                .child(
                            h_resizable("doc-split")
                                .child(
                                    resizable_panel().child(
                                        div()
                                            .v_flex()
                                            .size_full()
                                            .items_center()
                                            .overflow_hidden()
                                            .child(self.render_document(cx)),
                                    ),
                                )
                                .child(
                                    resizable_panel()
                                        .size(px(OUTLINE_WIDTH))
                                        .size_range(px(OUTLINE_MIN)..px(OUTLINE_MAX))
                                        .visible(self.session.outline_open)
                                        .child(self.render_outline(cx)),
                                ),
                                ),
                        ),
                ),
            )
            .on_resize(move |state, _window, cx| {
                let width = state.read(cx).sizes().first().copied();
                let Some(width) = width else { return };
                let width = f32::from(width);
                weak_resize
                    .update(cx, |this, _cx| {
                        this.session.sidebar_width = width;
                    })
                    .ok();
            });

        div()
            .v_flex()
            .id("folio-root")
            .relative()
            .size_full()
            .bg(background)
            .text_color(foreground)
            .child(
                TitleBar::new().child(
                    div()
                        .flex()
                        .w_full()
                        .h_full()
                        .pr_3()
                        .items_center()
                        .child(header),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .border_t_1()
                    .border_color(border)
                    .child(body),
            )
            .child(self.render_palette(cx))
            .on_action({
                let weak = cx.weak_entity();
                move |_: &Quit, _window, cx| {
                    weak.update(cx, |this, _cx| this.persist_session()).ok();
                    cx.quit();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &OpenPalette, window, cx| {
                    weak.update(cx, |this, cx| this.open_palette(window, cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &CloseOverlay, window, cx| {
                    weak.update(cx, |this, cx| {
                        if this.palette.is_some() {
                            this.close_palette(window, cx);
                        } else {
                            this.close_find(cx);
                        }
                    })
                    .ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &ToggleSidebar, _window, cx| {
                    weak.update(cx, |this, cx| this.toggle_sidebar(cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &ToggleOutline, _window, cx| {
                    weak.update(cx, |this, cx| this.toggle_outline(cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &ToggleTheme, window, cx| {
                    weak.update(cx, |this, cx| this.toggle_theme(window, cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &ToggleHidden, _window, cx| {
                    weak.update(cx, |this, cx| {
                        this.tree.toggle_hidden();
                        this.tree_selected = None;
                        cx.notify();
                    })
                    .ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &RefreshTree, _window, cx| {
                    weak.update(cx, |this, cx| {
                        this.tree.refresh();
                        cx.notify();
                    })
                    .ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &Reload, _window, cx| {
                    weak.update(cx, |this, cx| this.reload_active(cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &NextTab, _window, cx| {
                    weak.update(cx, |this, cx| this.cycle_tab(1, cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &PreviousTab, _window, cx| {
                    weak.update(cx, |this, cx| this.cycle_tab(-1, cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &FirstTab, _window, cx| {
                    weak.update(cx, |this, cx| this.select_tab(0, cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &LastTab, _window, cx| {
                    weak.update(cx, |this, cx| {
                        let last = this.docs.len().saturating_sub(1);
                        this.select_tab(last, cx);
                    })
                    .ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &CloseTab, _window, cx| {
                    weak.update(cx, |this, cx| this.close_active(cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &BeginFind, window, cx| {
                    weak.update(cx, |this, cx| this.begin_find(window, cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &FindNext, _window, cx| {
                    weak.update(cx, |this, cx| this.find_step(1, cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &FindPrevious, _window, cx| {
                    weak.update(cx, |this, cx| this.find_step(-1, cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &MoveSelectionDown, _window, cx| {
                    weak.update(cx, |this, cx| this.tree_move(1, cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &MoveSelectionUp, _window, cx| {
                    weak.update(cx, |this, cx| this.tree_move(-1, cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &CollapseSelection, _window, cx| {
                    weak.update(cx, |this, cx| this.tree_collapse(cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &ExpandSelection, _window, cx| {
                    weak.update(cx, |this, cx| this.tree_expand(cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &ActivateSelection, _window, cx| {
                    weak.update(cx, |this, cx| {
                        if let Some(row) = this.tree_selected {
                            this.tree_click(row, cx);
                        }
                    })
                    .ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &PaletteDown, _window, cx| {
                    weak.update(cx, |this, cx| this.palette_move(1, cx)).ok();
                }
            })
            .on_action({
                let weak = cx.weak_entity();
                move |_: &PaletteUp, _window, cx| {
                    weak.update(cx, |this, cx| this.palette_move(-1, cx)).ok();
                }
            })
    }
}
