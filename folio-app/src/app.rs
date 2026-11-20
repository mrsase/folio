//! The window: layout, action wiring and the single `Render` implementation.
//! State lives in `state`; views live in `views`.

pub use crate::state::Folio;

use crate::actions::*;
use crate::state::Dialog;
use crate::views;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::input::{Input, InputEvent};
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::component::{TitleBar, h_resizable, resizable_panel};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::time::Duration;

const SIDEBAR_MIN: f32 = 200.0;
const SIDEBAR_MAX: f32 = 560.0;
const OUTLINE_WIDTH: f32 = 236.0;
const OUTLINE_MIN: f32 = 160.0;
const OUTLINE_MAX: f32 = 420.0;

impl Folio {
    fn apply_pending(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for index in 0..self.tabs.len() {
            let reload = self.tabs[index].needs_reload;
            let sync = self.tabs[index].needs_sync;
            if reload {
                let outcome = self.tabs[index].doc.as_mut().map(|doc| doc.reload());
                match outcome {
                    Some(Ok(_)) => {
                        self.tabs[index].needs_reload = false;
                        self.tabs[index].refresh_view(cx);
                        self.tabs[index].sync_editor(window, cx);
                    }
                    Some(Err(error)) => {
                        self.tabs[index].needs_reload = false;
                        self.tabs[index].show_error(error.to_string(), cx);
                    }
                    None => self.tabs[index].needs_reload = false,
                }
            } else if sync {
                self.tabs[index].needs_sync = false;
                self.tabs[index].sync_editor(window, cx);
            }
        }

        if !self.pending_opens.is_empty() {
            let opens = std::mem::take(&mut self.pending_opens);
            let active = self.pending_active.take();
            for (path, mode, scroll, cursor) in opens {
                let path = folio_core::paths::canonical(&path);
                if let Some(index) = self.tabs.iter().position(|tab| tab.path == path) {
                    self.active = Some(index);
                    continue;
                }
                let mut tab = crate::state::Tab::load(path, mode, scroll, cursor, cx);
                if tab.mode == folio_core::session::ViewMode::Source {
                    tab.ensure_editor(window, cx);
                    if tab.editor.is_none() {
                        tab.mode = folio_core::session::ViewMode::Preview;
                    }
                }
                if let Some(editor) = tab.editor.clone() {
                    cx.subscribe(&editor, |this, editor, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::Change) {
                            this.editor_changed(editor, cx);
                        }
                    })
                    .detach();
                }
                self.tabs.push(tab);
                self.active = Some(self.tabs.len() - 1);
            }
            if let Some(active) = active
                && let Some(index) = self.tabs.iter().position(|tab| tab.path == active)
            {
                self.active = Some(index);
            }
            if let Some(path) = self.active_tab().map(|tab| tab.path.clone()) {
                self.tree_selected = self.tree.reveal(&path);
            }
            self.refocus_tree = true;
            self.persist_session(cx);
            cx.notify();
        }

        if self.refocus_tree {
            self.refocus_tree = false;
            let editor_focus = self.active_tab().and_then(|tab| {
                (tab.mode == folio_core::session::ViewMode::Source)
                    .then(|| tab.editor.clone())
                    .flatten()
                    .map(|editor| editor.read(cx).focus_handle(cx).clone())
            });
            let handle = editor_focus.unwrap_or_else(|| self.tree_focus.clone());
            window.focus(&handle, cx);
        }
    }

    fn render_notice(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(message) = self.notice.clone() else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let weak = cx.weak_entity();
        div()
            .id("notice")
            .absolute()
            .bottom(px(18.0))
            .left(px(18.0))
            .right(px(18.0))
            .flex()
            .items_center()
            .gap_3()
            .px_4()
            .py_2()
            .rounded_md()
            .bg(theme.tab_bar)
            .border_1()
            .border_color(theme.border)
            .text_sm()
            .text_color(theme.foreground)
            .child(div().flex_1().child(message))
            .child(
                div()
                    .id("notice-dismiss")
                    .px_2()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Dismiss")
                    .on_click(move |_, _window, cx| {
                        weak.update(cx, |this, cx| {
                            this.notice = None;
                            cx.notify();
                        })
                        .ok();
                    }),
            )
            .into_any_element()
    }

    fn render_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if !self.settings_open {
            return div().into_any_element();
        }
        let theme = cx.theme();
        let border = theme.border;
        let foreground = theme.foreground;
        let muted = theme.muted_foreground;
        let accent = theme.accent;
        let weak = cx.weak_entity();

        let mut theme_row = div().flex().gap_2();
        for (label, preference) in [
            ("System", folio_core::session::ThemePreference::System),
            ("Light", folio_core::session::ThemePreference::Light),
            ("Dark", folio_core::session::ThemePreference::Dark),
        ] {
            let selected = self.session.theme == preference;
            let listener = cx.listener(move |this, _, window, cx| {
                this.set_theme(preference, window, cx);
            });
            theme_row = theme_row.child(
                div()
                    .id(label)
                    .px_3()
                    .py_1()
                    .rounded_md()
                    .border_1()
                    .border_color(if selected { accent } else { border })
                    .text_sm()
                    .text_color(if selected { foreground } else { muted })
                    .cursor_pointer()
                    .hover(move |button| button.bg(muted.opacity(0.12)))
                    .child(label)
                    .on_click(listener),
            );
        }

        let font_size = self.session.font_size;
        let weak_minus = weak.clone();
        let weak_plus = weak.clone();
        let weak_remember = weak.clone();
        let weak_close = weak.clone();

        div()
            .id("settings-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .v_flex()
            .items_center()
            .justify_center()
            .bg(black().opacity(0.28))
            .on_click(move |_, _window, cx| {
                weak.update(cx, |this, cx| {
                    this.settings_open = false;
                    cx.notify();
                })
                .ok();
            })
            .child(
                div()
                    .v_flex()
                    .id("settings-panel")
                    .w(px(440.0))
                    .gap_4()
                    .p_5()
                    .rounded_lg()
                    .bg(theme.title_bar)
                    .border_1()
                    .border_color(border)
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(foreground)
                            .child("Settings"),
                    )
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .child(div().text_xs().text_color(muted).child("Theme"))
                            .child(theme_row),
                    )
                    .child(
                        div()
                            .v_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child("Document font size"),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id("font-minus")
                                            .px_3()
                                            .py_1()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(border)
                                            .cursor_pointer()
                                            .text_sm()
                                            .text_color(foreground)
                                            .child("−")
                                            .on_click(move |_, _window, cx| {
                                                weak_minus
                                                    .update(cx, |this, cx| {
                                                        let next = this.session.font_size - 1.0;
                                                        this.set_font_size(next, cx);
                                                    })
                                                    .ok();
                                            }),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(foreground)
                                            .child(format!("{font_size:.0} pt")),
                                    )
                                    .child(
                                        div()
                                            .id("font-plus")
                                            .px_3()
                                            .py_1()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(border)
                                            .cursor_pointer()
                                            .text_sm()
                                            .text_color(foreground)
                                            .child("+")
                                            .on_click(move |_, _window, cx| {
                                                weak_plus
                                                    .update(cx, |this, cx| {
                                                        let next = this.session.font_size + 1.0;
                                                        this.set_font_size(next, cx);
                                                    })
                                                    .ok();
                                            }),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(foreground)
                                    .child("Remember last root"),
                            )
                            .child(
                                div()
                                    .id("remember-root")
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(if self.session.remember_root {
                                        accent
                                    } else {
                                        border
                                    })
                                    .cursor_pointer()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(if self.session.remember_root {
                                        "On"
                                    } else {
                                        "Off"
                                    })
                                    .on_click(move |_, _window, cx| {
                                        weak_remember
                                            .update(cx, |this, cx| this.toggle_remember_root(cx))
                                            .ok();
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child("Reduced motion follows the macOS accessibility setting."),
                    )
                    .child(
                        div().flex().justify_end().child(
                            div()
                                .id("settings-close")
                                .px_3()
                                .py_1()
                                .rounded_md()
                                .border_1()
                                .border_color(border)
                                .text_sm()
                                .text_color(foreground)
                                .cursor_pointer()
                                .child("Done")
                                .on_click(move |_, _window, cx| {
                                    weak_close
                                        .update(cx, |this, cx| {
                                            this.settings_open = false;
                                            cx.notify();
                                        })
                                        .ok();
                                }),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn render_new_file(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(state) = self.new_file.as_ref() else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let border = theme.border;
        let foreground = theme.foreground;
        let muted = theme.muted_foreground;
        let weak = cx.weak_entity();
        let dir = folio_core::util::display_path(&state.dir);
        let error = state.error.clone();
        let template = state.template;
        let overwrite = state.overwrite;
        let typed = state.name.read(cx).value().to_string();
        let target = if typed.trim().is_empty() {
            dir.clone()
        } else {
            format!("{dir}/{}", typed.trim())
        };
        let create_label = if overwrite { "Replace" } else { "Create" };

        let choose = cx.listener(|this, _, window, cx| {
            this.choose_new_file_dir(window, cx);
        });
        let weak_template = weak.clone();
        let weak_create = weak.clone();
        let weak_cancel = weak.clone();

        div()
            .id("new-file-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .v_flex()
            .items_center()
            .justify_center()
            .bg(black().opacity(0.3))
            .child(
                div()
                    .v_flex()
                    .id("new-file-panel")
                    .w(px(520.0))
                    .gap_3()
                    .p_5()
                    .rounded_lg()
                    .bg(theme.title_bar)
                    .border_1()
                    .border_color(border)
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(foreground)
                            .child("New Markdown file"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(dir),
                            )
                            .child(
                                div()
                                    .id("new-file-choose")
                                    .px_2()
                                    .py_1()
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(border)
                                    .text_xs()
                                    .text_color(foreground)
                                    .cursor_pointer()
                                    .child("Choose…")
                                    .on_click(choose),
                            ),
                    )
                    .child(Input::new(&state.name))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("Creates {target}")),
                    )
                    .child(
                        div()
                            .id("new-file-template")
                            .flex()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .child(
                                div()
                                    .w(px(14.0))
                                    .h(px(14.0))
                                    .rounded_sm()
                                    .border_1()
                                    .border_color(if template { foreground } else { border })
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_xs()
                                    .text_color(foreground)
                                    .child(if template { "✓" } else { "" }),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(foreground)
                                    .child("Add frontmatter template"),
                            )
                            .on_click(move |_, _window, cx| {
                                weak_template
                                    .update(cx, |this, cx| {
                                        if let Some(state) = this.new_file.as_mut() {
                                            state.template = !state.template;
                                            state.error = None;
                                        }
                                        cx.notify();
                                    })
                                    .ok();
                            }),
                    )
                    .when_some(error, |panel, error| {
                        panel.child(div().text_xs().text_color(theme.danger).child(error))
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                div()
                                    .id("new-file-cancel")
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(border)
                                    .text_sm()
                                    .text_color(muted)
                                    .cursor_pointer()
                                    .child("Cancel")
                                    .on_click(move |_, _window, cx| {
                                        weak_cancel
                                            .update(cx, |this, cx| this.close_new_file(cx))
                                            .ok();
                                    }),
                            )
                            .child(
                                div()
                                    .id("new-file-create")
                                    .px_3()
                                    .py_1()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(foreground)
                                    .text_sm()
                                    .text_color(foreground)
                                    .cursor_pointer()
                                    .child(create_label)
                                    .on_click(move |_, _window, cx| {
                                        weak_create
                                            .update(cx, |this, cx| this.create_new_file(cx))
                                            .ok();
                                    }),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn render_dialog(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(dialog) = self.dialog.clone() else {
            return div().into_any_element();
        };
        let theme = cx.theme();
        let border = theme.border;
        let foreground = theme.foreground;
        let muted = theme.muted_foreground;
        let surface = theme.title_bar;
        let weak = cx.weak_entity();

        let (title, message, actions): (String, String, AnyElement) = match dialog {
            Dialog::Conflict {
                index,
                conflict,
                after,
            } => {
                let path = self
                    .tabs
                    .get(index)
                    .map(|tab| folio_core::util::display_path(&tab.path))
                    .unwrap_or_default();
                let buttons = div()
                    .flex()
                    .gap_2()
                    .child(dialog_button(
                        "Reload",
                        weak.clone(),
                        border,
                        foreground,
                        muted,
                        false,
                        move |this, cx| this.conflict_reload(index, after, cx),
                    ))
                    .child(dialog_button(
                        "Overwrite",
                        weak.clone(),
                        border,
                        foreground,
                        muted,
                        true,
                        move |this, cx| this.conflict_overwrite(index, after, cx),
                    ))
                    .child(dialog_button(
                        "Save As…",
                        weak.clone(),
                        border,
                        foreground,
                        muted,
                        false,
                        move |this, cx| this.conflict_save_as(index, after, cx),
                    ))
                    .child(dialog_button(
                        "Cancel",
                        weak.clone(),
                        border,
                        foreground,
                        muted,
                        false,
                        |this, cx| this.cancel_dialog(cx),
                    ))
                    .into_any_element();
                let what = match conflict {
                    folio_core::conflict::Conflict::Modified => "changed on disk",
                    folio_core::conflict::Conflict::Deleted => "was deleted on disk",
                    folio_core::conflict::Conflict::Replaced => "was replaced by another file",
                    folio_core::conflict::Conflict::Exists => "already exists",
                };
                (
                    "File changed on disk".to_string(),
                    format!(
                        "{path}\n\nThe file {what}. Reloading discards your buffer; \
                         overwriting replaces the file on disk."
                    ),
                    buttons,
                )
            }
            Dialog::Unsaved { index } => {
                let path = self
                    .tabs
                    .get(index)
                    .map(|tab| folio_core::util::display_path(&tab.path))
                    .unwrap_or_default();
                let buttons = div()
                    .flex()
                    .gap_2()
                    .child(dialog_button(
                        "Save",
                        weak.clone(),
                        border,
                        foreground,
                        muted,
                        true,
                        move |this, cx| this.unsaved_save(index, cx),
                    ))
                    .child(dialog_button(
                        "Discard",
                        weak.clone(),
                        border,
                        foreground,
                        muted,
                        false,
                        move |this, cx| this.unsaved_discard(index, cx),
                    ))
                    .child(dialog_button(
                        "Cancel",
                        weak.clone(),
                        border,
                        foreground,
                        muted,
                        false,
                        |this, cx| this.cancel_dialog(cx),
                    ))
                    .into_any_element();
                (
                    "Unsaved changes".to_string(),
                    format!("{path} has unsaved changes."),
                    buttons,
                )
            }
            Dialog::Quit { dirty } => {
                let count = dirty.len();
                let buttons = div()
                    .flex()
                    .gap_2()
                    .child(dialog_button(
                        "Save All",
                        weak.clone(),
                        border,
                        foreground,
                        muted,
                        true,
                        move |this, cx| this.quit_save_all(dirty.clone(), cx),
                    ))
                    .child(dialog_button(
                        "Discard All",
                        weak.clone(),
                        border,
                        foreground,
                        muted,
                        false,
                        move |this, cx| this.quit_discard_all(cx),
                    ))
                    .child(dialog_button(
                        "Cancel",
                        weak.clone(),
                        border,
                        foreground,
                        muted,
                        false,
                        |this, cx| this.cancel_dialog(cx),
                    ))
                    .into_any_element();
                (
                    "Quit Folio?".to_string(),
                    format!("{count} document(s) have unsaved changes."),
                    buttons,
                )
            }
        };

        div()
            .id("dialog-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .v_flex()
            .items_center()
            .justify_center()
            .bg(black().opacity(0.35))
            .child(
                div()
                    .v_flex()
                    .key_context("Dialog")
                    .id("dialog-panel")
                    .w(px(500.0))
                    .gap_3()
                    .p_5()
                    .rounded_lg()
                    .bg(surface)
                    .border_1()
                    .border_color(border)
                    .child(
                        div()
                            .text_sm()
                            .font_semibold()
                            .text_color(foreground)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .whitespace_normal()
                            .child(message),
                    )
                    .child(div().flex().justify_end().child(actions)),
            )
            .into_any_element()
    }

    fn render_root(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.apply_pending(window, cx);

        let theme = cx.theme();
        let background = theme.background;
        let foreground = theme.foreground;
        let muted = theme.muted_foreground;
        let border = theme.border;

        crate::state::trace_first_frame();

        if self.last_bounds_save.elapsed() > Duration::from_millis(900)
            && let WindowBounds::Windowed(bounds) = window.window_bounds()
        {
            let rect = folio_core::session::WindowRect {
                x: f32::from(bounds.origin.x),
                y: f32::from(bounds.origin.y),
                width: f32::from(bounds.size.width),
                height: f32::from(bounds.size.height),
            };
            if self.session.window != Some(rect) {
                self.session.window = Some(rect);
                self.session.save();
                self.last_bounds_save = std::time::Instant::now();
            }
        }

        let title = self
            .active_tab()
            .map(|tab| tab.title())
            .unwrap_or_else(|| "Folio".to_string());
        window.set_window_title(&format!("{title} — Folio"));

        let sidebar_width = self.session.sidebar_width;
        let sidebar_visible = self.session.sidebar_visible;
        let root_label = self
            .tree
            .root()
            .map(folio_core::util::display_path)
            .unwrap_or_else(|| "No folder open".to_string());

        let header = div()
            .flex()
            .w_full()
            .h_full()
            .px_3()
            .gap_3()
            .items_center()
            .child(
                div()
                    .text_sm()
                    .font_medium()
                    .text_color(foreground)
                    .child(title),
            )
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
                .child(views::browser::render(self, cx))
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
                        .child(views::tabs::render(self, cx))
                        .child(views::find::render(self, cx))
                        .child(
                            div().flex_1().min_h_0().child(
                                h_resizable("doc-split")
                                    .child(
                                        resizable_panel().child(
                                            div()
                                                .v_flex()
                                                .size_full()
                                                .items_center()
                                                .overflow_hidden()
                                                .child(views::reader::render(self, cx)),
                                        ),
                                    )
                                    .child(
                                        resizable_panel()
                                            .size(px(OUTLINE_WIDTH))
                                            .size_range(px(OUTLINE_MIN)..px(OUTLINE_MAX))
                                            .visible(self.session.outline_open)
                                            .child(views::outline::render(self, cx)),
                                    ),
                            ),
                        ),
                ),
            )
            .on_resize(move |state, _window, cx| {
                let width = state.read(cx).sizes().first().copied();
                let Some(width) = width else { return };
                weak_resize
                    .update(cx, |this, _cx| {
                        this.session.sidebar_width = f32::from(width);
                    })
                    .ok();
            });

        let weak = cx.weak_entity();
        let root = div()
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
            .child(views::palette::render(self, cx))
            .child(self.render_dialog(cx))
            .child(self.render_new_file(cx))
            .child(self.render_settings(window, cx))
            .child(self.render_notice(cx));

        root.on_action({
            let weak = weak.clone();
            move |_: &Quit, _window, cx| {
                weak.update(cx, |this, cx| {
                    if this.request_quit(cx) {
                        cx.quit();
                    }
                })
                .ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &OpenPalette, window, cx| {
                weak.update(cx, |this, cx| this.open_palette(window, cx))
                    .ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &CloseOverlay, window, cx| {
                weak.update(cx, |this, cx| this.close_overlay(window, cx))
                    .ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &ToggleSidebar, _window, cx| {
                weak.update(cx, |this, cx| {
                    this.session.sidebar_visible = !this.session.sidebar_visible;
                    this.persist_session(cx);
                    cx.notify();
                })
                .ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &ToggleOutline, _window, cx| {
                weak.update(cx, |this, cx| {
                    this.session.outline_open = !this.session.outline_open;
                    this.persist_session(cx);
                    cx.notify();
                })
                .ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &ToggleTheme, window, cx| {
                weak.update(cx, |this, cx| {
                    let next = if cx.theme().mode.is_dark() {
                        gpui_kit::component::theme::ThemeMode::Light
                    } else {
                        gpui_kit::component::theme::ThemeMode::Dark
                    };
                    gpui_kit::component::theme::Theme::change(next, Some(window), cx);
                    this.session.theme = if next.is_dark() {
                        folio_core::session::ThemePreference::Dark
                    } else {
                        folio_core::session::ThemePreference::Light
                    };
                    this.persist_session(cx);
                    cx.notify();
                })
                .ok();
            }
        })
        .on_action({
            let weak = weak.clone();
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
            let weak = weak.clone();
            move |_: &RefreshTree, _window, cx| {
                weak.update(cx, |this, cx| {
                    this.tree.refresh();
                    cx.notify();
                })
                .ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &Reload, _window, cx| {
                weak.update(cx, |this, cx| this.reload_active(cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &NextTab, _window, cx| {
                weak.update(cx, |this, cx| this.cycle_tab(1, cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &PreviousTab, _window, cx| {
                weak.update(cx, |this, cx| this.cycle_tab(-1, cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &FirstTab, _window, cx| {
                weak.update(cx, |this, cx| this.select_tab(0, cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &LastTab, _window, cx| {
                weak.update(cx, |this, cx| {
                    let last = this.tabs.len().saturating_sub(1);
                    this.select_tab(last, cx);
                })
                .ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &CloseTab, _window, cx| {
                weak.update(cx, |this, cx| this.close_active(cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &BeginFind, window, cx| {
                weak.update(cx, |this, cx| this.begin_find(window, cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &FindNext, _window, cx| {
                weak.update(cx, |this, cx| this.find_step(1, cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &FindPrevious, _window, cx| {
                weak.update(cx, |this, cx| this.find_step(-1, cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &MoveSelectionDown, _window, cx| {
                weak.update(cx, |this, cx| this.tree_move(1, cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &MoveSelectionUp, _window, cx| {
                weak.update(cx, |this, cx| this.tree_move(-1, cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &CollapseSelection, _window, cx| {
                weak.update(cx, |this, cx| this.tree_collapse(cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &ExpandSelection, _window, cx| {
                weak.update(cx, |this, cx| this.tree_expand(cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &ActivateSelection, _window, cx| {
                weak.update(cx, |this, cx| this.tree_activate(cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &PaletteDown, _window, cx| {
                weak.update(cx, |this, cx| this.palette_move(1, cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &PaletteUp, _window, cx| {
                weak.update(cx, |this, cx| this.palette_move(-1, cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &RevealInFinder, _window, cx| {
                weak.update(cx, |this, cx| this.reveal_active(cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &OpenContainingFolder, _window, cx| {
                weak.update(cx, |this, cx| this.open_containing_folder(cx))
                    .ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &OpenSettings, _window, cx| {
                weak.update(cx, |this, cx| {
                    this.settings_open = true;
                    cx.notify();
                })
                .ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &CancelDialog, _window, cx| {
                weak.update(cx, |this, cx| this.cancel_dialog(cx)).ok();
            }
        })
        .on_action(|_: &ConfirmDialog, _window, _cx| {})
        .on_action({
            let weak = weak.clone();
            move |_: &OpenFile, window, cx| {
                weak.update(cx, |this, cx| this.open_file_dialog(window, cx))
                    .ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &OpenFolder, window, cx| {
                weak.update(cx, |this, cx| this.open_folder_dialog(window, cx))
                    .ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &NewFile, window, cx| {
                weak.update(cx, |this, cx| this.open_new_file(window, cx))
                    .ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &Save, _window, cx| {
                weak.update(cx, |this, cx| this.save_active(cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &SaveAs, _window, cx| {
                weak.update(cx, |this, cx| this.save_as_active(cx)).ok();
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &TogglePreview, window, cx| {
                weak.update(cx, |this, cx| this.toggle_preview(window, cx))
                    .ok();
            }
        })
        .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
            let mut folder = None;
            let mut files = Vec::new();
            let mut refused = 0usize;
            for path in paths.paths() {
                if path.is_dir() {
                    folder.get_or_insert_with(|| path.clone());
                } else if crate::state::is_markdown(path) {
                    files.push(path.clone());
                } else {
                    refused += 1;
                }
            }
            if let Some(dir) = folder {
                this.open_root(dir, cx);
            }
            for path in files {
                this.open_path(&path, cx);
            }
            if refused > 0 {
                this.notice = Some(format!(
                    "{refused} dropped file(s) were not Markdown and were ignored."
                ));
            }
            cx.notify();
        }))
        .into_any_element()
    }
}

#[allow(clippy::too_many_arguments)]
fn dialog_button<F>(
    label: &'static str,
    weak: WeakEntity<Folio>,
    border: Hsla,
    foreground: Hsla,
    muted: Hsla,
    primary: bool,
    action: F,
) -> AnyElement
where
    F: Fn(&mut Folio, &mut Context<Folio>) + 'static,
{
    div()
        .id(label)
        .h(px(30.0))
        .px_3()
        .rounded_md()
        .border_1()
        .border_color(if primary { foreground } else { border })
        .flex()
        .items_center()
        .cursor_pointer()
        .hover(move |button| button.bg(muted.opacity(0.14)))
        .child(div().text_sm().text_color(foreground).child(label))
        .on_click(move |_, _window, cx| {
            weak.update(cx, |this, cx| action(this, cx)).ok();
        })
        .into_any_element()
}

impl Render for Folio {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_root(window, cx)
    }
}
