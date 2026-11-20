//! First screen: open actions, recent files and folders, shortcuts. Also the
//! error state for a tab that could not be decoded.

use crate::actions::{NewFile, OpenFile, OpenFolder};
use crate::state::{Folio, Tab};
use folio_core::util;
use gpui_kit::base::StyledExt as _;
use gpui_kit::component::theme::ActiveTheme;
use gpui_kit::component::{Icon, Sizable};
use gpui_kit::*;

pub fn render(this: &Folio, cx: &mut Context<Folio>) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let foreground = theme.foreground;
    let hover = theme.list_hover;
    let border = theme.border;
    let weak = cx.weak_entity();

    let mut recents = div().v_flex().gap_1().items_center().max_w(px(620.0));
    let files: Vec<_> = this.session.recents.files.iter().take(5).cloned().collect();
    let folders: Vec<_> = this
        .session
        .recents
        .folders
        .iter()
        .take(5)
        .cloned()
        .collect();

    if !files.is_empty() {
        recents = recents.child(
            div()
                .w_full()
                .pt_2()
                .text_xs()
                .font_semibold()
                .text_color(muted)
                .child("RECENT FILES"),
        );
        for (index, path) in files.into_iter().enumerate() {
            let weak = weak.clone();
            let target = path.clone();
            let label = util::display_path(&path);
            recents = recents.child(
                row(index, "recent-file", label, hover, muted, foreground).on_click(
                    move |_, _window, cx| {
                        weak.update(cx, |this, cx| this.open_path(&target, cx)).ok();
                    },
                ),
            );
        }
    }
    if !folders.is_empty() {
        recents = recents.child(
            div()
                .w_full()
                .pt_2()
                .text_xs()
                .font_semibold()
                .text_color(muted)
                .child("RECENT FOLDERS"),
        );
        for (index, path) in folders.into_iter().enumerate() {
            let weak = weak.clone();
            let target = path.clone();
            let label = util::display_path(&path);
            recents = recents.child(
                row(index, "recent-folder", label, hover, muted, foreground).on_click(
                    move |_, _window, cx| {
                        weak.update(cx, |this, cx| this.open_root(target.clone(), cx))
                            .ok();
                    },
                ),
            );
        }
    }

    let actions = div()
        .flex()
        .gap_2()
        .child(action_button(
            "Open File…",
            || Box::new(OpenFile),
            border,
            foreground,
            muted,
        ))
        .child(action_button(
            "Open Folder…",
            || Box::new(OpenFolder),
            border,
            foreground,
            muted,
        ))
        .child(action_button(
            "New File…",
            || Box::new(NewFile),
            border,
            foreground,
            muted,
        ));

    div()
        .v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_5()
        .px_6()
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
                        .child("Markdown, rendered properly. Open a file or a folder to begin."),
                ),
        )
        .child(actions)
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child("⌘P quick open  ·  ⌘O open file  ·  ⇧⌘O open folder  ·  ⌘N new file"),
        )
        .child(recents)
        .into_any_element()
}

pub fn render_error(tab: &Tab, cx: &mut Context<Folio>) -> AnyElement {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let foreground = theme.foreground;
    let warning = theme.warning;
    div()
        .v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .gap_3()
        .px_8()
        .child(
            div()
                .text_lg()
                .font_semibold()
                .text_color(foreground)
                .child("This file could not be opened"),
        )
        .child(
            div().text_sm().text_color(warning).child(
                tab.error
                    .clone()
                    .unwrap_or_else(|| "Unknown error".to_string()),
            ),
        )
        .child(
            div()
                .text_xs()
                .text_color(muted)
                .child(util::display_path(&tab.path)),
        )
        .into_any_element()
}

fn row(
    index: usize,
    id: &'static str,
    label: String,
    hover: Hsla,
    muted: Hsla,
    foreground: Hsla,
) -> Stateful<Div> {
    div()
        .id((id, index))
        .w_full()
        .h(px(30.0))
        .px_3()
        .rounded_sm()
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .hover(move |row| row.bg(hover))
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
                .child(label),
        )
}

fn action_button(
    label: &'static str,
    make: fn() -> Box<dyn Action>,
    border: Hsla,
    foreground: Hsla,
    muted: Hsla,
) -> AnyElement {
    div()
        .id(label)
        .h(px(32.0))
        .px_3()
        .rounded_md()
        .border_1()
        .border_color(border)
        .flex()
        .items_center()
        .cursor_pointer()
        .hover(|button| button.bg(muted.opacity(0.12)))
        .child(div().text_sm().text_color(foreground).child(label))
        .on_click(move |_, window, cx| {
            window.dispatch_action(make(), cx);
        })
        .into_any_element()
}
