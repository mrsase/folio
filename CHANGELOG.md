# Changelog

All notable changes to Folio are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[semantic versioning](https://semver.org/).

## [0.1.0] — 2026-11-21

### Added

- Open any Markdown file (`⌘O`, typed path, drag and drop, CLI) or folder
  (`⇧⌘O`), with recent files and folders.
- Create a Markdown file (`⌘N`) with a validated name, folder choice, overwrite
  confirmation and an optional frontmatter template.
- A source editor with cursor/selection, undo/redo, find, dirty indicators,
  `⌘S`, `⇧⌘S`, a rendered preview toggle (`⌘E`) and a live heading outline.
- Byte-exact saves: UTF-8 BOM, LF/CRLF/CR endings, mixed-ending head/tail lines
  and trailing-newline state are preserved; writes are temp-file + fsync +
  rename with the original mode.
- External-change detection with Reload / Overwrite / Save As / Cancel, and
  unsaved-change guards on tab close and quit.
- Task-list checkbox write-back through the same conflict-checked save path.
- Session restore of window geometry, root, tabs, per-tab mode and scroll,
  sidebar width/visibility, outline, theme, font size and recents.
- Settings for theme (light/dark/system), document font size and remembering
  the last root; reduced motion follows macOS.
- A bounded, cancellable quick-open index that never follows symlinked
  directories and skips build/VCS directories.

### Changed

- The proof of concept's read-only surface is now editable; its 256 KiB
  preview cap is preserved.
- One workspace: `folio-core` (pure logic and tests) and `folio-app` (GPUI).

### Fixed

- The PoC's checkbox write path rewrote normalized text with `fs::write`,
  dropping the BOM and line endings. It now replaces one marker byte and saves
  through the atomic, conflict-checked path.

### Preserved

- The proof of concept remains readable at tag `poc-archive` and branch `poc`.

### Limits

- macOS only.
- No WYSIWYG, split view, print/PDF, plugins, sync, iOS, multiple windows or
  full-text search across a home directory.
- Unsigned build; no notarization.
- 256 KiB preview cap; 2 MiB editable source, read-only to 8 MiB, preview-only
  to 16 MiB.
