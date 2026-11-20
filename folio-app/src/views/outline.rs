//! Heading outline. Tracks the buffer (including unsaved edits in later
//! phases) and jumps the document to the heading's byte offset.

use crate::state::Folio;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

pub fn render(this: &Folio, cx: &mut Context<Folio>) -> AnyElement {
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

    if let Some(tab) = this.active_tab() {
        if tab.headings.is_empty() {
            panel = panel.child(
                div()
                    .px_2()
                    .text_sm()
                    .text_color(muted)
                    .child("No headings"),
            );
        }
        for (index, heading) in tab.headings.iter().enumerate() {
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
