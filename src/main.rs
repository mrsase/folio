//! Folio — a native, fast markdown reader for macOS.
//!
//! Everything here is bootstrap: window options, theme, key bindings, menu bar.
//! The reader itself lives in [`app`].

mod app;
mod model;

use std::path::PathBuf;

use folio::session::{Session, ThemePreference};
use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::component::TitleBar;
use gpui_kit::*;

fn main() {
    app::mark_process_start();
    let session = Session::load();
    let initial: Option<PathBuf> = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        // Sessions store absolute paths, so resolve whatever the shell passed.
        .map(|path| std::fs::canonicalize(&path).unwrap_or(path));

    let saved_bounds = session.window;
    let theme_preference = session.theme;

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets)
        .run(move |cx| {
            gpui_kit::init(cx);
            app::bind_keys(cx);
            app::install_menus(cx);

            let window_bounds = saved_bounds.map(|rect| {
                WindowBounds::Windowed(Bounds {
                    origin: point(px(rect.x), px(rect.y)),
                    size: size(px(rect.width), px(rect.height)),
                })
            });

            let options = WindowOptions {
                window_bounds,
                window_min_size: Some(size(px(760.0), px(520.0))),
                ..TitleBar::window_options()
            };

            gpui_kit::open_window(options, cx, move |window, cx| {
                let mode = match theme_preference {
                    ThemePreference::System => ThemeMode::from(window.appearance()),
                    ThemePreference::Light => ThemeMode::Light,
                    ThemePreference::Dark => ThemeMode::Dark,
                };
                Theme::change(mode, Some(window), cx);
                cx.new(|cx| app::Folio::new(window, cx, session.clone(), initial.clone()))
            })
            .expect("failed to open the Folio window");
        });
}
