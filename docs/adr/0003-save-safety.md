# ADR 0003 — Save safety

**Status:** proposed · **Date:** 2026-11-20 · **Deciders:** Folio maintainers

## Context

The PoC is read-only apart from task checkboxes. Its write path is a background
`std::fs::write(&path, source)` (`src/app.rs:512`). The `source` it writes has
already been normalized on load: `normalize()` strips a UTF-8 BOM and rewrites
CRLF/CR to LF (`src/model.rs:150`). Therefore toggling one checkbox silently
rewrites the whole file, drops the BOM and converts line endings. There is no
temp file, no fsync, no rename, no file identity check and no conflict handling.
A watcher event calls `reload_if_stale()` unconditionally.

Editing makes every one of those a data-loss path. This ADR defines the contract
that `folio-core` must enforce, independent of GPUI.

## Options

| Option | Trade-off |
| --- | --- |
| In-place write of the buffer with `fs::write` (PoC) | Simplest; truncates on failure, loses encoding details, no conflict detection. Rejected. |
| Temp file + fsync + rename, keep bytes as decoded (chosen) | A crash leaves either the old file or the complete new one. Requires a buffer that remembers BOM and line endings, and a file-identity check. Bounded, testable work. |
| Write-ahead journal / versioned backups | Stronger recovery, but new on-disk artifacts, cleanup policy and UI. Not justified for a single-file editor; rejected for v1. |

## Decision

### 1. Buffer model (`folio-core::document`)

A loaded file becomes a `Document` with two representations:

- `text`: the editable buffer, always valid UTF-8, always LF-only.
- `format`: everything needed to reproduce the file's bytes —
  - `bom: bool`
  - `endings: Vec<Ending>` (`Lf`, `Crlf`, `Cr`) in original line order
  - `dominant: Ending` (most frequent; ties → `Lf`)
  - `trailing_newline: bool`
  - `mode: u32` (Unix permission bits)
- `disk: DiskStamp { len, modified, dev, ino }` captured at read time.
- `saved_revision` / `revision` for dirty tracking.

Loading:

1. Read bytes. Refuse above 16 MiB for editing (preview still shows the head).
2. A UTF-8 BOM is recorded and stripped. A UTF-16 BOM is refused with the
   encoding named. Other invalid UTF-8 is refused with the byte offset and a
   `std::str::Utf8Error` description.
3. Split on `\r\n`, `\n` and lone `\r`; record one `Ending` per line and join
   with `\n` into `text`. `trailing_newline` records whether the file ended with
   a terminator. A lone `\r` is preserved as `Cr`.

Saving:

1. If `revision == saved_revision`, return `Ok(Unchanged)` without touching the
   disk. A clean file is byte-identical by construction.
2. Build bytes: prepend `\xEF\xBB\xBF` when `bom`; for each line, emit its
   original ending when that line is unchanged, otherwise the dominant ending
   (common prefix/suffix line matching, so untouched head and tail lines keep
   their exact terminators even in mixed-ending files); emit the final
   terminator only when the buffer ends with a newline or the original had one.
3. Run the atomic write in §2.

This preserves BOM, LF vs CRLF, lone CR, trailing-newline presence, and
untouched lines of a mixed-ending file. Files that are not edited are never
rewritten at all.

### 2. Atomic write

For an existing target:

1. Re-stat the target and compare `(dev, ino, len, modified)` with `Document.disk`.
   A mismatch is a `Conflict` (see §3) and aborts before any write.
2. Create a sibling temp file: `.<name>.folio-<pid>-<counter>.tmp` with
   `create_new(true)` in the **same directory** (same filesystem, so rename is
   atomic).
3. `write_all`, then `sync_all` on the temp file.
4. Apply the original `mode` with `set_permissions` (do not invent a mode).
5. `rename(temp, target)` — the only step that changes the visible file.
6. Open the parent directory and `sync_all` it, so the rename is durable.
7. Re-stat the target and store the new `DiskStamp`.

Any failure before step 5 removes the temp file and leaves the original
untouched. A crash before step 5 leaves the original untouched and at worst a
`.tmp` sibling; the next save removes stale `.folio-*.tmp` files older than an
hour in that directory.

For a new file (or save-as to a path that did not exist):

- Build the same temp file and `rename` it into place only after confirming the
  target still does not exist (`create_new` intent). If it appeared, surface a
  conflict instead of overwriting.
- Default mode `0o644` before the user's umask is applied by the OS; no
  executable bit.

Save-as to an existing path is an explicit overwrite: the native save panel has
already asked. It still uses the atomic write and adopts the target's mode when
one exists.

### 3. External change detection

- `DiskStamp` is compared at every save and on every watcher event.
- **Clean buffer + disk changed:** reload the buffer in place, reset editor
  state, keep the tab.
- **Dirty buffer + disk changed:** never overwrite silently and never reload
  automatically. Mark the tab "changed on disk" and, on save or on the user's
  click, show: **Reload** (discard buffer), **Overwrite** (write buffer),
  **Save As…**, **Cancel**. The dialog names the path and states which side is
  newer; no choice is pre-selected as destructive.
- **File deleted:** saving reports "deleted on disk"; offer Save As or recreate.
- **File replaced** (different inode, same path): treated as a conflict even if
  size and mtime match.
- Watcher events caused by Folio's own write are ignored because the post-write
  stamp equals the event's stamp.
- Watcher debounce stays at 300 ms, as measured in the PoC.

### 4. Unsaved-change guards

| Trigger | Behaviour |
| --- | --- |
| Close tab (`⌘W`) | Save / Discard / Cancel. Cancel keeps the tab. |
| Close window / `⌘Q` | One dialog listing every dirty tab: Save All / Discard All / Cancel. `Window::on_window_should_close` returns `false` until resolved. |
| Root switch, open file/folder, drag-drop | No prompt: tabs and buffers are preserved. |
| Live reload of a clean tab | No prompt. |
| Conflict | §3 dialog. |

### 5. Size and encoding limits

| Condition | Behaviour |
| --- | --- |
| Valid UTF-8, ≤ 2 MiB | Fully editable. |
| Valid UTF-8, 2–8 MiB | Read-only source with a notice; preview head; checkbox writes allowed through this same path. |
| Valid UTF-8, 8–16 MiB | Preview-only (head + notice); no writes of any kind. |
| > 16 MiB | Refused with the size named; no tab content beyond a notice. |
| UTF-16 / invalid UTF-8 | Reader explains the encoding and byte offset; editing and checkbox writes disabled. |
| Preview render cap | Unchanged at 256 KiB, the PoC's measured limit. |

The 2 MiB editable and 8/16 MiB hard limits are conservative starting values,
not measured claims. Phase 3 measures typing/save latency at 2 MiB and records
the largest accepted file in `docs/evaluation.md`. Raising a limit requires a
measurement; lowering one does not.

### 6. Checkbox write-back

Preview and source both route through `Document::toggle_checkbox`:

1. Locate the marker on the line; replace only the byte between `[` and `]`.
2. Preserve the marker case: `[X]` stays uppercase when re-checked.
3. Mark the document dirty, then use the normal save path (conflict check,
   atomic write).
4. If the file is non-UTF-8, oversized, or in conflict, the checkbox is disabled
   or the conflict dialog appears — never a blind write.

## Required tests (`folio-core` + integration, no UI)

| Test | Assertion |
| --- | --- |
| `bom_and_crlf_round_trip` | Load BOM+CRLF, edit one word, save: BOM present, every terminator CRLF, bytes otherwise identical. |
| `lf_no_bom_round_trip` | Same for plain LF, no BOM. |
| `trailing_newline_preserved` | File without a final newline stays without one; with one stays with one. |
| `lone_cr_preserved` | Old-Mac line endings survive an edit. |
| `mixed_endings_head_and_tail` | Editing the middle keeps head/tail terminators exactly. |
| `refuses_invalid_utf8` | Latin-1 byte 0xE9 → error names the byte offset; no write. |
| `refuses_utf16` | UTF-16LE/BE BOM → error names the encoding; no write. |
| `crash_before_rename_keeps_original` | Injected failure after temp write/fync: original bytes unchanged, temp removed. |
| `atomic_replace_preserves_mode` | 0o600 file stays 0o600 after save. |
| `conflict_same_size_new_mtime` | Save returns `Conflict`; original on disk unchanged. |
| `conflict_deleted_file` | Save returns `Conflict::Deleted`. |
| `conflict_inode_replaced` | Save returns `Conflict::Replaced`. |
| `clean_save_is_noop` | No temp file, mtime unchanged. |
| `checkbox_preserves_encoding` | Toggle in BOM+CRLF file: one marker byte changes, everything else identical. |
| `checkbox_uppercase_preserved` | `[X]` → `[ ]` → `[X]` keeps uppercase. |
| `new_file_create_new` | Existing target is not overwritten without confirmation. |
| `save_as_adopts_target_mode` | Save-as over an existing 0o600 file keeps 0o600. |

The crash test uses an internal failpoint (a test-only `Disk` trait or a
`#[cfg(test)]` fault hook) invoked between fsync and rename — not a real crash,
but it exercises the same ordering.

## Consequences

- `folio-core` owns all byte-level behaviour and can be tested in milliseconds.
- The app never writes a buffer directly; there is one save function.
- The PoC's checkbox bug is fixed by construction and covered by tests.
- A file with mixed line endings is preserved where untouched; edited lines use
  the dominant style. This is stated in the README limits.
- No backup files, no journals, no hidden version history in v1.
