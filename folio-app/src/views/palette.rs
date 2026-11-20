//! Quick open: ranked index hits, a typed folder row, and a typed file row.

use crate::state::Folio;
use folio_core::paths;
use folio_core::util;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::input::Input;
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub fn render(this: &Folio, cx: &mut Context<Folio>) -> AnyElement {
    if this.palette.is_none() {
        return div().into_any_element();
    }
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let border = theme.border;
    let panel_bg = theme.title_bar;
    let selected_bg = theme.list_active;
    let foreground = theme.foreground;

    let entity = cx.entity();
    let weak = cx.weak_entity();
    let weak_close = cx.weak_entity();
    let selected = this
        .palette
        .as_ref()
        .map(|p| p.selected_index())
        .unwrap_or(0);
    let count = this.palette.as_ref().map(|p| p.len()).unwrap_or(0);
    let scanning = this.palette.as_ref().is_some_and(|p| p.scanning);
    let truncated = this.palette.as_ref().is_some_and(|p| p.truncated);
    let query = this
        .palette
        .as_ref()
        .map(|palette| palette.query.clone())
        .unwrap_or_default();
    let root = this.tree.root().map(|root| root.to_path_buf());

    let folder = this
        .palette
        .as_ref()
        .and_then(|palette| palette.folder_candidate_in(root.as_deref()));
    let folder_row = folder
        .as_ref()
        .map(|dir| folder_row(dir, &weak, selected_bg, muted, foreground))
        .unwrap_or_else(|| div().into_any_element());

    let typed_file = typed_path_candidate(&query, root.as_deref()).filter(|path| {
        path.is_file()
            && !this
                .palette
                .as_ref()
                .is_some_and(|palette| palette.selected_hit().is_some())
    });
    let typed_file_row = typed_file
        .as_ref()
        .map(|path| {
            let weak = weak.clone();
            let target = path.clone();
            let label = util::display_path(path);
            div()
                .id("palette-typed-file")
                .h(px(30.0))
                .w_full()
                .px_3()
                .flex()
                .items_center()
                .gap_2()
                .cursor_pointer()
                .hover(move |row| row.bg(selected_bg))
                .child(
                    Icon::new(gpui_kit::assets::IconName::FileText)
                        .text_color(muted)
                        .with_size(px(13.)),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(foreground)
                        .truncate()
                        .child(format!("Open file  ·  {label}")),
                )
                .on_click(move |_, _window, cx| {
                    weak.update(cx, |this, cx| {
                        this.palette = None;
                        this.open_path(&target, cx);
                    })
                    .ok();
                })
                .into_any_element()
        })
        .unwrap_or_else(|| div().into_any_element());

    let mut recent_rows = div().v_flex();
    if query.trim().is_empty() {
        for (index, path) in this.session.recents.files.iter().take(3).enumerate() {
            let weak = weak.clone();
            let target = path.clone();
            let label = util::display_path(path);
            recent_rows = recent_rows.child(
                div()
                    .id(("recent-file", index))
                    .h(px(28.0))
                    .w_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .hover(move |row| row.bg(selected_bg))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::FileText)
                            .text_color(muted)
                            .with_size(px(13.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(foreground)
                            .child(label),
                    )
                    .on_click(move |_, _window, cx| {
                        weak.update(cx, |this, cx| {
                            this.palette = None;
                            this.open_path(&target, cx);
                        })
                        .ok();
                    }),
            );
        }
        for (index, path) in this.session.recents.folders.iter().take(2).enumerate() {
            let weak = weak.clone();
            let target = path.clone();
            let label = util::display_path(path);
            recent_rows = recent_rows.child(
                div()
                    .id(("recent-folder", index))
                    .h(px(28.0))
                    .w_full()
                    .px_3()
                    .flex()
                    .items_center()
                    .gap_2()
                    .cursor_pointer()
                    .hover(move |row| row.bg(selected_bg))
                    .child(
                        Icon::new(gpui_kit::assets::IconName::FolderOpen)
                            .text_color(muted)
                            .with_size(px(13.)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_sm()
                            .text_color(foreground)
                            .child(label),
                    )
                    .on_click(move |_, _window, cx| {
                        weak.update(cx, |this, cx| {
                            this.palette = None;
                            this.open_root(target.clone(), cx);
                        })
                        .ok();
                    }),
            );
        }
    }

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
                                this.open_path(&path, cx);
                            })
                            .ok();
                        }),
                )
            })
            .collect()
    })
    .flex_1()
    .min_h_0()
    .track_scroll(&this.palette_scroll);

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
                        .child(Input::new(&this.palette_input)),
                )
                .child(folder_row)
                .child(typed_file_row)
                .child(recent_rows)
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

fn folder_row(
    dir: &std::path::Path,
    weak: &WeakEntity<Folio>,
    selected_bg: Hsla,
    muted: Hsla,
    foreground: Hsla,
) -> AnyElement {
    let weak = weak.clone();
    let dir = dir.to_path_buf();
    let display = util::display_path(&dir);
    div()
        .id("palette-folder")
        .h(px(30.0))
        .w_full()
        .px_3()
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .hover(move |row| row.bg(selected_bg))
        .child(
            Icon::new(gpui_kit::assets::IconName::FolderOpen)
                .text_color(muted)
                .with_size(px(13.)),
        )
        .child(
            div()
                .text_sm()
                .text_color(foreground)
                .truncate()
                .child(format!("Open folder  ·  {display}")),
        )
        .on_click(move |_, _window, cx| {
            let dir = dir.clone();
            weak.update(cx, |this, cx| {
                this.palette = None;
                this.refocus_tree = true;
                this.open_root(dir, cx);
            })
            .ok();
        })
        .into_any_element()
}

fn typed_path_candidate(query: &str, root: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    if !paths::looks_like_path(query) {
        return None;
    }
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    paths::resolve(query, root, home.as_deref()).ok()
}
