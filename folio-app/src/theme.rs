//! Theme application. The preference is stored in the session; the actual
//! `ThemeMode` is derived from it (or from the macOS appearance for `System`).

use crate::state::ThemePreference;
use gpui_kit::component::theme::{Theme, ThemeMode};
use gpui_kit::*;

pub fn apply(preference: ThemePreference, window: &mut Window, cx: &mut App) {
    let mode = match preference {
        ThemePreference::System => ThemeMode::from(window.appearance()),
        ThemePreference::Light => ThemeMode::Light,
        ThemePreference::Dark => ThemeMode::Dark,
    };
    Theme::change(mode, Some(window), cx);
}
