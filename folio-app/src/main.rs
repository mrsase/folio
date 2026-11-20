//! Folio: a native macOS Markdown reader and editor built on GPUI.

mod actions;
mod app;
mod keys;
mod menu;
mod state;
mod theme;
mod views;

use std::path::PathBuf;

use folio_core::paths;
use folio_core::session::Session;
use gpui_kit::*;

fn main() {
    state::mark_process_start();
    let session = Session::load();
    let initial: Option<PathBuf> = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .map(|path| paths::canonical(&path));

    let saved_bounds = session.window;
    let theme_preference = session.theme;

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            keys::bind(cx);
            menu::install(cx);

            let window_bounds = saved_bounds.map(|rect| {
                WindowBounds::Windowed(Bounds {
                    origin: point(px(rect.x), px(rect.y)),
                    size: size(px(rect.width), px(rect.height)),
                })
            });

            let options = WindowOptions {
                window_bounds,
                window_min_size: Some(size(px(760.0), px(520.0))),
                ..gpui_kit::component::TitleBar::window_options()
            };

            gpui_kit::open_window(options, cx, move |window, cx| {
                theme::apply(theme_preference, window, cx);
                cx.new(|cx| app::Folio::new(window, cx, session.clone(), initial.clone()))
            })
            .expect("failed to open the Folio window");
        });
}
