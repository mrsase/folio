# Validation — v0.1.0

**Question:** does the real app, driven as a user would drive it, do what the
design says?

**Environment:** macOS 14 (x64), 1680×1050 pt display, build `target/debug/folio-app`
(debug, `opt-level = 1` for the workspace and 3 for dependencies). The final
release build is verified in `docs/evaluation.md`. Screenshots are in
`docs/screenshots/` and were captured from the running window with
`screencapture -x`; nothing is mocked or composited.

Keyboard and mouse were injected with `CGEvent` after bringing the process to
the front. The macOS Accessibility permission is not granted to the terminal, so
AppleScript UI scripting was unavailable; raw `CGEvent` posting worked and every
result below was read from the screen or the file bytes.

## 1. Open a folder outside the launch directory

Session started with root `/tmp/folio-new`. `⌘P` → typed `/tmp/folio-task` →
`↩`.

**Observed:** the tree rebuilt to `folio-task` with `doc.md`; the previously
open `note.md` stayed as a tab labelled `outside`, and the window title kept the
active document. The session file recorded
`"root": "/private/tmp/folio-task"`. Screenshot: `session-restore.png` (the same
state after a relaunch).

## 2. Open a file by absolute path

`⌘P` → typed an absolute file path → `↩`.

**Observed:** the file opened as a tab without changing the root, and the tab
showed the `outside` badge. The path was resolved through the same normalizer as
`~` and relative paths (`folio-core::paths` unit tests).

`⌘O` was also opened: the native `NSOpenPanel` appeared with the prompt “Open
Markdown”; Escape cancelled it and the app stayed alive. Screenshot:
taken during the run; no file was selected in that pass.

A second run opened `~/workPlace/md-viewer-rs/docs/specimen.md` by typing its
absolute path into `⌘P` while the root was an empty `/tmp/folio-outside`. The
tab appeared with the `outside` badge and the banner “Outside root ·
~/workPlace/…”; the tree stayed at its root with `FILES 0`. Screenshot:
`outside-root.png`.

## 3. Create a file

`⌘N` → typed `note` → `↩`.

**Observed:** `/tmp/folio-new/note.md` was created (0 bytes, no template) and
opened in Source mode with focus in the editor. The sheet is shown in
`new-file.png`; the created file was listed by `ls`. A second creation with an
existing name produces an inline “already exists” confirmation before replacing
(the code path is exercised by `create_new_file`; the confirm click is manual).

## 4. Edit and save, including bytes

Fixture: `# Title\r\n\r\nline one\r\nline two\r\nline three\r\n` with a UTF-8 BOM.

1. Opened in Source mode, clicked the editor, pasted ` EDITED`.
2. `⌘S`.

**Observed bytes after save:**

```
00000000: efbb bf23 2054 6974 6c65 0d0a 0d0a 6c69  ...# Title....li
00000010: 6e65 206f 6e65 0d0a 6c69 6e65 2074 776f  ne one..line two
00000020: 0d0a 6c69 6e65 2074 6872 6565 0d0a 2045  ..line three.. E
00000030: 4449 5445 44                             DITED
```

The BOM is still the first three bytes, every terminator is still CRLF, and no
LF was introduced. Screenshot: `editor-source.png`.

A second fixture with a task list was saved after checkbox edits; the result
still reported `bom True crlf 79 lone_lf 0`.

## 5. External change while dirty

Opened a file, pasted `DIRTY`, appended `external change` to the file from a
shell, then pressed `⌘S`.

**Observed:** no write happened. The dialog “File changed on disk” appeared with
Reload / Overwrite / Save As… / Cancel. Screenshot: `conflict.png`. The original
external content was still on disk when the app was closed.

## 5b. Undo

In Source mode, pasting `ZZZ` and pressing `⌘Z` removed the inserted text. The
dirty dot remained, by design: dirty is `revision != saved_revision`, so undoing
to the saved text is conservatively reported as dirty (ADR 0001). No save was
performed in that run.

## 6. Unsaved guards

- Closing a dirty tab (the `×`) opened “Unsaved changes” with Save / Discard /
  Cancel. Screenshot: `quit-guard.png` is the quit variant; the tab-close variant
  was observed in the same session.
- `⌘Q` with a dirty buffer opened “Quit Folio?” with Save All / Discard All /
  Cancel and the process stayed alive. The same shortcut with a clean buffer
  exited the process.

## 7. Session restore

After the root switch and new-file work above, the process was killed and
relaunched with the same `FOLIO_SESSION_DIR`.

**Observed:** the light theme, the `folio-task` root, the `note.md` outside tab,
the active document and Source mode came back. A second run with
`"scroll_y": 800` opened the preview scrolled to the task/table section, not the
top. Screenshot: `session-restore.png`.

## 8. Theme and font size

- `⌘⇧L` switched from dark to light; the session file recorded
  `"theme": "light"`. Screenshots: `reader-dark.png`, `reader-light.png`.
- A session with `"font_size": 24` rendered the preview at 24 pt; `"font_size":
  20` rendered the editor at 20 pt. Both are visibly larger than the 16 pt
  default.
- In the settings panel, clicking **Light** changed `"theme"` to `"light"` and
  clicking **+** changed `"font_size"` to `17.0`; both were read back from the
  session file. Screenshot: `settings-light.png`.

## 9. Find, outline, palette

- `⌘F` → typed `rendering`: the bar reported `1/2` and highlighted matches.
  Screenshot: `find.png`.
- `⌘⇧B`: the outline appeared with the heading hierarchy.
  Screenshot: `outline.png`.
- `⌘P`: quick-open showed the indexed file and the typed-directory row.
  Screenshot: `palette.png`.

## 10. Live reload

With a clean preview open, `## Live reload proof` was appended externally. The
tab’s stats changed from 8 to 9 headings without a keystroke in the app, proving
the watcher reloaded the document. Screenshot: `reload.png`.

## 11. Checkbox write-back

In preview, clicking the `Live reload` checkbox emitted a source offset and
rewrote only that marker. A grid of clicks produced alternating
`[ ]`/`[x]` writes; the file remained valid UTF-8 with its BOM and CRLF endings.
The core test `checkbox_writes_preserve_bom_and_crlf` covers the same path
without GPUI.

## 13. Non-UTF-8 refusal

`caf\xe9\n` (Latin-1) was opened. The tab showed “not valid UTF-8 (byte 3:
invalid utf-8 sequence of 1 bytes from index 3); convert the file to UTF-8 to
edit it” and offered no editor. The bytes on disk were unchanged; `xxd` shows
`6361 66e9 0a` (`caf` + 0xE9 + LF). Screenshot: `non-utf8.png`.

## Not validated in this pass

- Completing a native Open or Save As selection (panels opened and cancelled;
  the save target path is covered by the `save_as` integration test).
- Drag-and-drop of a file or folder (code is wired; no synthetic drop was
  performed).
- “Reveal in Finder” and “Open containing folder” (actions are wired to
  `cx.reveal_path` and the root switch; not clicked in this pass).
- The overwrite-confirmation branch of New File (the refusal message was
  designed and unit-covered at the document level; the second click was not
  driven).
- Reduced motion with the macOS switch toggled (the framework reads it at init;
  no app-owned animation exists to observe).
- Files above 2 MiB in the editor (policy tested; typing latency not measured).

## 12. Large file and read-only degradation

A 2,097,099-byte Markdown file (repeated headings, prose and task items) was
opened in preview mode.

**Observed:** the window appeared with a first frame at **1,645–1,723 ms**
(debug build), the preview banner “Preview limited to the first 256 KiB; the
full file is kept for saving”, and `0 headings` because the outline is capped
at 256 KiB. Clicking the mode chip created the source editor with plain text
(no syntect highlighting) and 2,097,099 bytes of content; the app stayed
responsive enough to paint line numbers and the first lines. Screenshot:
`large-file.png`.

This is the largest file the editor accepts by policy. Typing latency at 2 MiB
was not instrumented in this pass; the byte cap is enforced and documented, not
claimed as a speed result.
