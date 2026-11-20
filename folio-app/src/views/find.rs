//! Find bar. Works on the rendered text in preview and on the source buffer in
//! the editor.

use crate::state::Folio;
use gpui_kit::component::input::Input;
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::*;

pub fn render(this: &Folio, cx: &mut Context<Folio>) -> AnyElement {
    if this.find.is_none() {
        return div().into_any_element();
    }
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let border = theme.border;
    let hover = theme.list_hover;
    let bar = theme.tab_bar;
    let count = this
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
        .child(div().flex_1().child(Input::new(&this.find_input)))
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
