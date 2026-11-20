//! The document pane: rendered Markdown with an outer scroll handle so the
//! reading position survives tab switches and relaunch.

use crate::state::Folio;
use folio_core::outline;
use folio_core::paths;
use folio_core::util;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::input::Editor;
use gpui_kit::component::text::{FrontmatterPlugin, MarkdownExtensions, TextView};
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::*;
use std::path::PathBuf;

const READING_WIDTH: f32 = 780.0;

pub fn render(this: &Folio, cx: &mut Context<Folio>) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let warning = theme.warning;

    let Some(tab) = this.active_tab() else {
        return super::welcome::render(this, cx);
    };
    if tab.doc.is_none() {
        return super::welcome::render_error(tab, cx);
    }

    let font_size = this.session.font_size;
    if tab.mode == folio_core::session::ViewMode::Source {
        let Some(editor) = tab.editor.as_ref() else {
            return super::welcome::render_error(tab, cx);
        };
        let readonly = tab.doc.as_ref().is_some_and(|doc| !doc.can_edit());
        return div()
            .v_flex()
            .h_full()
            .w_full()
            .min_w_0()
            .text_size(px(font_size))
            .child(
                Editor::new(editor)
                    .appearance(false)
                    .bordered(false)
                    .readonly(readonly)
                    .h(relative(1.))
                    .px_4()
                    .py_3(),
            )
            .into_any_element();
    }

    let weak = cx.weak_entity();
    let dir = tab
        .path
        .parent()
        .map(|path| path.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/"));
    let link_dir = dir.clone();
    let weak_task = weak.clone();
    let weak_link = weak.clone();
    let handle = tab.scroll.clone();
    let outside = this
        .tree
        .root()
        .is_some_and(|root| !paths::is_within(root, &tab.path));
    let display_path = util::display_path(&tab.path);
    let disk_changed = tab.disk_changed;
    let truncated = tab.truncated;

    let mut pane = div()
        .v_flex()
        .h_full()
        .w_full()
        .min_w_0()
        .text_size(px(font_size));

    if outside {
        pane = pane.child(
            div()
                .w_full()
                .px_4()
                .py_1()
                .bg(theme.tab_bar)
                .border_b_1()
                .border_color(theme.border)
                .text_xs()
                .text_color(muted)
                .child(format!("Outside root · {display_path}")),
        );
    }
    if disk_changed {
        pane = pane.child(
            div()
                .w_full()
                .px_4()
                .py_1()
                .bg(theme.tab_bar)
                .border_b_1()
                .border_color(theme.border)
                .text_xs()
                .text_color(warning)
                .child("This file changed on disk while the buffer has unsaved edits."),
        );
    }
    if truncated {
        pane = pane.child(
            div()
                .w_full()
                .px_4()
                .py_1()
                .bg(theme.tab_bar)
                .border_b_1()
                .border_color(theme.border)
                .text_xs()
                .text_color(muted)
                .child("Preview limited to the first 256 KiB; the full file is kept for saving."),
        );
    }

    let scroll = div()
        .id("document-scroll")
        .flex_1()
        .min_h_0()
        .w_full()
        .overflow_y_scroll()
        .track_scroll(&handle)
        .child(
            div().v_flex().w_full().items_center().child(
                div().w_full().max_w(px(READING_WIDTH)).min_w_0().child(
                    TextView::new(&tab.view)
                        .selectable(true)
                        .scrollable(false)
                        .px_8()
                        .pt_5()
                        .pb_24()
                        .markdown_extensions(
                            MarkdownExtensions::default().frontmatter().raw_html(true),
                        )
                        .plugin(FrontmatterPlugin::new())
                        .image_source(move |uri: &SharedUri| {
                            let uri = uri.to_string();
                            match outline::resolve_local(&dir, &uri) {
                                Some(path) => ImageSource::from(path),
                                None => ImageSource::from(uri),
                            }
                        })
                        .on_reveal({
                            let handle = handle.clone();
                            move |bounds, _window, _cx| {
                                let viewport = handle.bounds();
                                let mut offset = handle.offset();
                                if bounds.top() < viewport.top() {
                                    offset.y += viewport.top() - bounds.top();
                                } else if bounds.bottom() > viewport.bottom() {
                                    offset.y -= bounds.bottom() - viewport.bottom();
                                }
                                handle.set_offset(offset);
                            }
                        })
                        .on_link_click(move |url, _event, _window, cx| {
                            let Some(path) = outline::resolve_local(&link_dir, url.as_ref()) else {
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
                                    .update(cx, |this, cx| this.open_path(&path, cx))
                                    .ok();
                            } else {
                                cx.open_url(&format!("file://{}", path.display()));
                            }
                        })
                        .on_task_toggle(move |event, _window, cx| {
                            let offset = event.marker_range().start;
                            let checked = event.checked();
                            weak_task
                                .update(cx, |this, cx| this.toggle_task(offset, checked, cx))
                                .ok();
                        }),
                ),
            ),
        );

    pane.child(scroll).into_any_element()
}
