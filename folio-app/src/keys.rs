//! Key bindings. Contexts keep tree and palette navigation from stealing keys
//! from the editor.

use crate::actions::*;
use gpui_kit::*;

pub fn bind(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-o", OpenFile, None),
        KeyBinding::new("cmd-shift-o", OpenFolder, None),
        KeyBinding::new("cmd-n", NewFile, None),
        KeyBinding::new("cmd-p", OpenPalette, None),
        KeyBinding::new("cmd-s", Save, None),
        KeyBinding::new("cmd-shift-s", SaveAs, None),
        KeyBinding::new("cmd-e", TogglePreview, None),
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("escape", CloseOverlay, None),
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-shift-b", ToggleOutline, None),
        KeyBinding::new("cmd-shift-l", ToggleTheme, None),
        KeyBinding::new("cmd-shift-.", ToggleHidden, None),
        KeyBinding::new("cmd-r", Reload, None),
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("cmd-shift-]", NextTab, None),
        KeyBinding::new("cmd-shift-[", PreviousTab, None),
        KeyBinding::new("cmd-f", BeginFind, None),
        KeyBinding::new("cmd-g", FindNext, None),
        KeyBinding::new("cmd-shift-g", FindPrevious, None),
        KeyBinding::new("cmd-1", FirstTab, None),
        KeyBinding::new("cmd-9", LastTab, None),
        KeyBinding::new("cmd-shift-r", RevealInFinder, None),
        KeyBinding::new("cmd-shift-f", OpenContainingFolder, None),
        KeyBinding::new("down", MoveSelectionDown, Some("FileTree")),
        KeyBinding::new("up", MoveSelectionUp, Some("FileTree")),
        KeyBinding::new("left", CollapseSelection, Some("FileTree")),
        KeyBinding::new("right", ExpandSelection, Some("FileTree")),
        KeyBinding::new("enter", ActivateSelection, Some("FileTree")),
        KeyBinding::new("down", PaletteDown, Some("Palette")),
        KeyBinding::new("up", PaletteUp, Some("Palette")),
        KeyBinding::new("enter", ConfirmDialog, Some("Dialog")),
        KeyBinding::new("escape", CancelDialog, Some("Dialog")),
    ]);
}
