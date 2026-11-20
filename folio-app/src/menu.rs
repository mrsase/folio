//! macOS menu bar. Menu items dispatch the same actions as the shortcuts, so
//! there is one behaviour per command.

use crate::actions::*;
use gpui_kit::*;

pub fn install(cx: &mut App) {
    cx.set_menus([
        Menu::new("File").items([
            MenuItem::action("Open File…", OpenFile),
            MenuItem::action("Open Folder…", OpenFolder),
            MenuItem::action("New File…", NewFile),
            MenuItem::action("Quick Open…", OpenPalette),
            MenuItem::separator(),
            MenuItem::action("Save", Save),
            MenuItem::action("Save As…", SaveAs),
            MenuItem::action("Reload Document", Reload),
            MenuItem::separator(),
            MenuItem::action("Reveal in Finder", RevealInFinder),
            MenuItem::action("Open Containing Folder", OpenContainingFolder),
            MenuItem::separator(),
            MenuItem::action("Close Tab", CloseTab),
            MenuItem::action("Quit Folio", Quit),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Find in Document", BeginFind),
            MenuItem::action("Find Next", FindNext),
            MenuItem::action("Find Previous", FindPrevious),
        ]),
        Menu::new("View").items([
            MenuItem::action("Toggle Source/Preview", TogglePreview),
            MenuItem::separator(),
            MenuItem::action("Toggle Sidebar", ToggleSidebar),
            MenuItem::action("Toggle Outline", ToggleOutline),
            MenuItem::action("Toggle Theme", ToggleTheme),
            MenuItem::action("Show Hidden Files", ToggleHidden),
            MenuItem::separator(),
            MenuItem::action("Settings…", OpenSettings),
        ]),
    ]);
}
