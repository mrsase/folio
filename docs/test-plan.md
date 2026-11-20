# Test plan

## Shape

| Layer | Location | Runs in | Proves |
| --- | --- | --- | --- |
| Core unit | `folio-core/src/**` `#[cfg(test)]` | `cargo test -p folio-core` | Encoding, newlines, atomic write, conflicts, checkboxes, outline, paths, ignore rules, recents, session |
| Core integration | `folio-core/tests/*.rs` with `tempfile` | same | Real files on disk: create, save, save-as, overwrite, refuse non-UTF-8, crash-before-rename |
| Ported PoC oracle | `folio-core` tests | same | The behaviours the PoC already had: tree, outline, find, fuzzy match, tab labels, session defaults |
| UI | none automated | manual | `docs/validation.md` only. No headless GPUI tests and no mock clicks. |

The PoC's 30 tests are the behavioural oracle. They are ported, not copied
wholesale: `file_tree`, `outline`, `session`, `util`, and the pure parts of
`model` (display slicing, normalization, find, palette ranking). The monolithic
`app.rs` view code is not ported.

## Core cases

### `document` — encoding, newlines, save

| Case | Assertion |
| --- | --- |
| LF, no BOM, trailing newline | Edit one word → bytes identical except that word. |
| CRLF, no BOM | Same, every terminator still CRLF. |
| UTF-8 BOM + CRLF | BOM still first three bytes; CRLF throughout. |
| No trailing newline | Stays absent after an edit that does not touch EOF. |
| Lone CR | Preserved on untouched lines. |
| Mixed endings | Head/tail terminators preserved; edited lines use the dominant ending. |
| Clean save | No temp file, no mtime change. |
| Invalid UTF-8 | `LoadError::NotUtf8 { offset }`; no write function available. |
| UTF-16 LE/BE | `LoadError::Utf16`; no write. |
| Oversized | 16 MiB+ refused; 8–16 MiB read-only; 2–8 MiB read-only source; 2 MiB editable. |

### `atomic` — durability

| Case | Assertion |
| --- | --- |
| Success | Target replaced; mode preserved; no temp remains. |
| Failure before rename (injected) | Original bytes and mtime unchanged; temp removed. |
| Failure during write | Same. |
| Failure during rename | Original intact; temp removed. |
| New file | `create_new` semantics; an existing target is not silently overwritten. |
| Save-as over existing | Target mode adopted; atomic replace. |
| Parent fsync failure | Reported; target already renamed (documented ordering). |

### `conflict`

| Case | Assertion |
| --- | --- |
| Same size, new mtime | `Conflict::Modified`. |
| Different size | `Conflict::Modified`. |
| Deleted | `Conflict::Deleted`. |
| Inode replaced | `Conflict::Replaced`. |
| Unchanged | `SaveOutcome::Saved` / `Unchanged`. |
| Own write event | Watcher sees no conflict. |

### `checkbox`

| Case | Assertion |
| --- | --- |
| `[ ]` → `[x]` | One byte changes; BOM/CRLF intact. |
| `[X]` → `[ ]` → `[X]` | Uppercase preserved. |
| Click outside a marker | No change. |
| Non-UTF-8 file | Toggle unavailable. |
| Conflict | Dialog path, no blind write. |

### `paths`

| Case | Assertion |
| --- | --- |
| `~` / `~/x` | Expands from `$HOME`. |
| Relative | Resolves against root or active document's folder. |
| `..` | Lexical normalization then canonicalization. |
| Symlink | Canonical path compares equal to target. |
| Display | Home shortened; duplicate names get parent hints. |

### `index` / `file_tree`

| Case | Assertion |
| --- | --- |
| Ignore list | `.git`, `node_modules`, `target`, `Library`, etc. skipped. |
| Depth/entry caps | Stops at the documented limits and marks partial. |
| Cancellation | A cancelled generation returns no results and stops walking. |
| Lazy tree | Collapsed directories are not read. |
| Hidden toggle | Dotfiles hidden by default. |
| Symlinked dirs | Shown, never followed. |

### `session` / `recents`

| Case | Assertion |
| --- | --- |
| Round-trip | All fields survive JSON. |
| Unknown/missing fields | Defaults. |
| Version | Old session without new fields loads. |
| Recents | MRU order, dedupe, prune missing, caps at 20+20. |
| Per-tab scroll/mode | Preserved. |
| Missing root | Root cleared; tabs retained. |

### `outline` / `find` / `util`

Ported from the PoC: headings skip code fences, slugs match GitHub anchors,
frontmatter bounds, section lookup, task counts, fuzzy ranking, match positions,
tab labels, byte formatting, display slicing at a block boundary.

## Manual UI matrix

Run against a release build, one session, screenshots in `docs/screenshots/`:

1. First launch with no session: welcome, recent lists empty.
2. Open a folder outside the launch directory; tree lazy-expands.
3. Open a file by `⌘O`, by typed `~/…` path, and by drag-drop; one is outside
   the root and marked.
4. Create a new file with and without the frontmatter template.
5. Edit, undo, redo, find, toggle preview, save; dirty dot appears/disappears.
6. External edit a clean file → auto reload; external edit a dirty file →
   conflict dialog; choose each branch in a throwaway copy.
7. Quit with a dirty tab → Save All; relaunch → root, tabs, active doc, scroll,
   sidebar width, theme, font size restored.
8. Toggle light/dark/system; set font size; relaunch.
9. Toggle macOS Reduce Motion; observe transitions.
10. Open a CRLF+BOM file, edit, save, `xxd` before/after.
11. Open a Latin-1 file; read the refusal; confirm no edit affordance.
12. Open a 3 MiB file; confirm read-only notice; preview head + notice.

Screenshots are captured with `screencapture` if macOS permissions allow. If
they do not, the maintainer supplies them; a screenshot is never fabricated.

## Resource-conscious execution

This machine is low on resources and Rust/GPUI compiles are expensive.

- `folio-core` is compiled and tested alone (`cargo test -p folio-core`); it has
  no GPUI dependency, so its tests are seconds, not minutes.
- App code is changed in batches and checked with `cargo check -p folio-app`
  at slice boundaries, not after every file.
- The existing `target/` directory is reused; it is never deleted for
  convenience. A clean-clone build is proven by CI on `macos-latest`, not
  locally, unless the maintainer explicitly asks for a local run.
- `cargo clippy --workspace` and `cargo build --release` run once at the verify
  phase, not during development.
- No `cargo watch`, no repeated `cargo test --workspace` loops, no release
  builds for UI iteration.

## CI

`.github/workflows/ci.yml`, `macos-latest`:

1. `cargo fmt --all --check`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo test --workspace`
4. `cargo build --release --locked`
5. Upload the test binary? No. Build cache via `Swatinem/rust-cache`.

If GPUI cannot build on the runner (missing macOS SDK pieces, runner image
change), the job splits: a `core` job (fmt, clippy on core, core tests) that must
be green, and a `full` job that may be `continue-on-error` with the reason
stated in the README. The tag is not cut on a red `core` job. Do not silently
skip the app build and call it verified.
