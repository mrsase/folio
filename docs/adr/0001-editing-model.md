# ADR 0001 — Editing model

**Status:** proposed · **Date:** 2026-11-20 · **Deciders:** Folio maintainers

## Context

The proof of concept (tag `poc-archive`) renders mdast through GPUI's `TextView`
and is read-only apart from task-list checkboxes. Editing is a required v1
capability. The PoC has no buffer, no cursor, no undo history and no save path
beyond `fs::write` of the normalized source (`src/app.rs:512`, `src/model.rs:150`).

`gpui-component` 0.10 ships an `EditorState` (the code-editor kind of the shared
input engine) with the primitives v1 needs. Verified in the crate source, not
assumed:

- `EditorState::new(window, cx).default_value(text).language("plaintext")`
- `value()`, `set_value()`, `text()` (a `ropey::Rope`), `cursor()`,
  `selected_range()`, `set_selected_range()`, `select_all()`, `replace_all()`
- built-in `Undo` / `Redo` actions backed by an undo manager
- `readonly(bool)`, line numbers, folding, decorations, its own scrolling
- `InputEvent::Change` for dirty tracking

This makes a real source editor cheap to adopt and a WYSIWYG layer unnecessary.

## Options

### A. Source editor + rendered preview (chosen)

Edit the raw Markdown in a monospace `EditorState`; toggle to the existing
rendered `TextView` with one shortcut. One buffer per tab.

- Cost: a source-mode view, a preview toggle, dirty/save plumbing, find over the
  buffer, and the size policy. All of it is ordinary app code.
- Failure mode: formatting is invisible while typing. Acceptable for a Markdown
  tool whose users know the syntax, and the preview is one keystroke away.
- Byte safety: the editor owns text, so the save path can preserve BOM, line
  endings and trailing newline exactly (ADR 0003).

### B. Block/inline WYSIWYG over the rendered view

Edit mdast nodes in place with caret placement, block splitting/merging, inline
marks, table editing, image/link UI, and a serializer that round-trips to
Markdown.

- Cost: multiple weeks to a usable state; the serializer is where documents get
  corrupted, and every CommonMark/GFM construct needs a tested mapping.
- It would also fight `TextView`, which is a renderer, not an editable model.
- Rejected for v1. It is not a stretch goal; it is a different product.

### C. Read-only + external editor

- Cheap and honest, but it fails the stated requirement that Folio can edit.
- Rejected.

## Decision

**A. Source editor + rendered preview.** `EditorState` (gpui-component) is the
source surface; `TextViewState` remains the preview and the read mode.

### Surface and shortcuts

| Concern | Decision |
| --- | --- |
| Modes | `Source` (editor) and `Preview` (rendered). Per-tab mode, default Preview for existing files, Source for a new file. |
| Toggle | `⌘E` toggles Source/Preview. `⌘E` is repurposed from macOS "Use Selection for Find"; recorded here because it is a deliberate conflict. |
| Cursor/selection | Editor-native: click, drag, shift+arrows, `⌘A`, word/line selection. Cursor offset is stored per tab on switch/quit and restored with the tab. |
| Undo/redo | Editor-native `Undo`/`Redo` (`⌘Z`, `⇧⌘Z`), mapped in the Edit menu. History lives with the tab's `EditorState`; closing the tab discards it. Saving does **not** clear history. |
| Dirty state | `InputEvent::Change` increments a revision; `dirty = revision != saved_revision`. Saving sets `saved_revision = revision`. Undoing back to the saved text still reports dirty (conservative: never claim clean when unsure). |
| Find | Find-only, no replace. `⌘F` opens one bar; it searches the editor buffer in Source mode and the rendered text in Preview mode. `⌘G`/`⇧⌘G` step matches; matches are highlighted (source: `set_selected_range` + decorations; preview: existing `Find`). Find never edits. |
| Save | `⌘S`: atomic write to the tab's path (ADR 0003). Clean buffer → no-op. `⇧⌘S`: native save panel (`prompt_for_new_path`), then atomic write and retarget the tab. |
| New file | `⌘N` opens the in-app New File sheet (location, validated name, overwrite confirmation, optional frontmatter template), creates the file with `create_new`, then opens it in Source mode. |
| Close tab | Unsaved guard: Save / Discard / Cancel. Cancel keeps the tab. |
| Quit / close window | Same guard for all dirty tabs: Save All / Discard All / Cancel. Implemented with `Window::on_window_should_close` returning `false` until resolved. |
| Outline | Rebuilt from the buffer after a 300 ms debounce (headless `outline()`), so it tracks unsaved edits. Clicking a heading moves the editor caret to that byte offset. Above 256 KiB the outline is not rebuilt (the preview cap) and says so. |
| Quick-open | Unchanged while editing: it opens another tab and never discards the current buffer. A dirty new file that has no path yet is not in quick-open; it stays as "Untitled" until saved. |
| Live reload | Watcher events never overwrite a dirty buffer. A dirty tab whose file changed on disk gets a "changed on disk" banner; the save path then offers Reload / Overwrite / Save As (ADR 0003). A clean tab reloads in place. |
| Checkbox write-back | Works in Preview and Source. It goes through the same conflict-checked atomic write. Not offered for non-UTF-8 files. |

### Size policy

| Size | Behaviour |
| --- | --- |
| ≤ 256 KiB | Preview renders the whole file; editor fully editable. |
| 256 KiB – 2 MiB | Preview keeps the existing head-and-notice behaviour; editor fully editable. |
| 2 MiB – 8 MiB | Source is **read-only** with a visible notice; preview shows the head. No outline parse. |
| > 8 MiB | Preview shows the head and a notice; no editor. |

The 256 KiB display cap is the PoC's measured limit and is preserved. The 2 MiB
editable cap is a conservative starting point, not a measured claim. Phase 3
must measure typing and save latency at the cap and record the largest file the
editor accepts in `docs/evaluation.md`. Raising the cap requires a measurement;
lowering it does not.

### What v1 deliberately does not do

- No WYSIWYG, no inline formatting commands, no table editor.
- No replace-all, multi-cursor, column selection, or Vim mode.
- No syntax highlighting in source mode. The editor is monospace with line
  numbers; Markdown highlighting is cosmetic and not required for correctness.
- No autosave. Saving is explicit; the dirty indicator is the contract.

## Consequences

- The reader and editor share tabs, outline, find, tree and session; only the
  innermost view changes.
- All byte-preservation logic lives in `folio-core`, not in the GPUI view, and
  is tested without a window server.
- The `poc-archive` `TextView` code is ported as the preview path, not as the
  editor.
- If `gpui-component`'s editor regresses, the fallback is a plain `Textarea`
  with app-owned undo — more work but no change to the save model.
