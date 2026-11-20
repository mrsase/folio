//! Tab strip: labels, dirty dots, out-of-root badges, close buttons.

use crate::state::Folio;
use folio_core::util;
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use std::path::Path as FsPath;

const TAB_STRIP_HEIGHT: f32 = 38.0;

pub fn render(this: &Folio, cx: &mut Context<Folio>) -> AnyElement {
    let theme = cx.theme();
    let bar = theme.tab_bar;
    let border = theme.border;
    let active_bg = theme.tab_active;
    let active_fg = theme.tab_active_foreground;
    let fg = theme.tab_foreground;
    let muted = theme.muted_foreground;
    let hover = theme.list_hover;
    let warning = theme.warning;
    let weak = cx.weak_entity();
    let root = this.tree.root().map(FsPath::to_path_buf);

    let mut strip = div()
        .id("tab-strip")
        .flex()
        .flex_1()
        .min_w_0()
        .overflow_x_scroll()
        .px_2()
        .gap_1()
        .items_center();

    let names: Vec<String> = this.tabs.iter().map(|tab| tab.title()).collect();
    for (index, tab) in this.tabs.iter().enumerate() {
        let is_active = this.active == Some(index);
        let duplicate = names.iter().filter(|name| **name == names[index]).count() > 1;
        let label = util::tab_label(&tab.path, duplicate);
        let outside = root
            .as_ref()
            .is_some_and(|root| !folio_core::paths::is_within(root, &tab.path));
        let dirty = tab.is_dirty();
        let weak_row = weak.clone();
        let weak_close = weak.clone();
        strip = strip.child(
            div()
                .id(("tab", index))
                .flex()
                .items_center()
                .max_w(px(240.0))
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
                .when(outside, |row| {
                    row.child(
                        div()
                            .px_1()
                            .rounded_sm()
                            .border_1()
                            .border_color(border)
                            .text_xs()
                            .text_color(muted)
                            .child("outside"),
                    )
                })
                .when(dirty, |row| {
                    row.child(div().w(px(7.0)).h(px(7.0)).rounded_full().bg(warning))
                })
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
                    weak_row
                        .update(cx, |this, cx| this.select_tab(index, cx))
                        .ok();
                }),
        );
    }

    let stats = this
        .active_tab()
        .and_then(|tab| {
            tab.doc.as_ref().map(|doc| {
                format!(
                    "{} · {} headings",
                    util::human_bytes(doc.text.len() as u64),
                    tab.headings.len()
                )
            })
        })
        .unwrap_or_default();
    let mode_label = if this
        .active_tab()
        .is_some_and(|tab| tab.mode == folio_core::session::ViewMode::Source)
    {
        "Source"
    } else {
        "Preview"
    };
    let weak_mode = weak.clone();
    let mode_chip = div()
        .id("mode-toggle")
        .mr_2()
        .px_2()
        .py_1()
        .rounded_sm()
        .border_1()
        .border_color(border)
        .cursor_pointer()
        .text_xs()
        .text_color(muted)
        .hover(move |chip| chip.bg(hover))
        .child(mode_label)
        .on_click(move |_, window, cx| {
            weak_mode
                .update(cx, |this, cx| this.toggle_preview(window, cx))
                .ok();
        });

    div()
        .flex()
        .h(px(TAB_STRIP_HEIGHT))
        .w_full()
        .items_center()
        .bg(bar)
        .border_b_1()
        .border_color(border)
        .child(strip)
        .child(mode_chip)
        .child(div().px_2().text_xs().text_color(muted).child(stats))
        .into_any_element()
}
