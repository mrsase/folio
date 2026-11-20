//! Sidebar: filter, lazy tree rows, root shortcut.

use crate::state::Folio;
use folio_core::util;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::input::Input;
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::{Path as FsPath, PathBuf};

const TREE_ROW_HEIGHT: f32 = 26.0;

pub fn render(this: &Folio, cx: &mut Context<Folio>) -> AnyElement {
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
    let rows = this.tree.len();
    let selected = this.tree_selected;
    let open_paths: Vec<PathBuf> = this.tabs.iter().map(|tab| tab.path.clone()).collect();

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
                        .child(if node.is_dir {
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
                        })
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
    .track_scroll(&this.tree_scroll);

    div()
        .v_flex()
        .id("sidebar")
        .h_full()
        .w_full()
        .bg(sidebar_bg)
        .border_r_1()
        .border_color(border)
        .key_context("FileTree")
        .track_focus(&this.tree_focus)
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
                .child(Input::new(&this.sidebar_filter)),
        )
        .child(list)
        .child({
            let weak = cx.weak_entity();
            let root = this.tree.root().map(FsPath::to_path_buf);
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
