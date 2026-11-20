# Folio v1.0 — design and acceptance criteria

This document is the contract for the rebuild. Every criterion is observable
and names its proof. Anything not listed here is out of scope (see
`docs/product.md`).

## Specification carried over from the proof of concept

The PoC README is treated as a specification, not marketing. These claims are
preserved unless a criterion says otherwise:

| Claim | Status in v1 |
| --- | --- |
| GPU-rendered, no web view | Preserved. |
| File tree with lazy expansion, filter, hidden toggle | Preserved; ignore rules extended (ADR 0002). |
| Tabs, outline, find with highlights, fuzzy quick-open | Preserved. |
| Live reload for clean files (≈300 ms debounce) | Preserved; dirty buffers are never clobbered (ADR 0003). |
| Task checkbox writes | Preserved, but through the safe-write path. |
| Session restore: window, root, tabs, active doc, sidebar width, theme | Preserved and extended: recents, per-tab scroll, editor mode, font size. |
| 256 KiB render cap | Preserved; measured reason unchanged. |
| Read-only except checkboxes | **Revised:** editing and saving are v1 requirements. |
| No open-file/open-folder path | **Revised:** `⌘O`, `⇧⌘O`, recents, drag-drop and typed paths are v1 requirements. |
| macOS only | Preserved. |
| Non-UTF-8 files explain themselves instead of rendering | Preserved and strengthened: they are never editable. |

## Acceptance criteria

### A1 — Clean-clone build

A fresh clone builds with one documented command on macOS:

```sh
git clone https://github.com/mrsase/folio.git && cd folio
cargo build --release --locked
```

**Proof:** CI job `build` on `macos-latest` is green on the tagged commit; a
local clean-clone build is recorded in `docs/verification.md` with wall time.
`rust-toolchain.toml` pins the compiler; `vendor/gpui-pre-macos` is committed and
its `[patch.crates-io]` entry survives the clone.

### A2 — Open any Markdown file

Given a file anywhere on the machine, the user can reach it by all of:

- `⌘O` native open panel (multiple files);
- a typed path in `⌘P`: absolute, `~/…`, or relative to the root;
- dragging one or more files onto the window;
- `folio /path/to/file.md` on the command line.

A file outside the tree root opens as a normal tab, visibly marked "outside
root", with its full path shown in the document's outside-root banner. Switching the root does not close it.

**Proof:** `docs/validation.md` steps, with a file under `~/workPlace/docs`
opened while the launch root is `md-viewer-rs`; screenshot of the tab and tree.

### A3 — Open any folder as the tree root

`⇧⌘O` chooses a directory and rebuilds the tree without restarting. The tree is
lazy (collapsed directories are never read), hides dotfiles by default, skips
`.DS_Store`, never follows symlinked directories, and reports unreadable
directories inline. The current tabs, unsaved buffers and scroll positions
survive the switch.

**Proof:** `docs/validation.md` re-roots from the app repository to
`~/workPlace/docs` with a dirty tab open; screenshot; `cargo test -p folio-core
file_tree` for lazy/ignore behaviour.

### A4 — Quick-open, typed paths and recents

`⌘P` ranks Markdown files under the root; it never reads file contents. An empty
query lists recent files and folders. Entering `~/…`, `/abs/…` or `root-relative`
opens a file or sets a folder root. The index is bounded (depth 12, 50,000
entries, 20,000 results, 3 s), cancellable on root change/quit, and reports
partial results. Recents survive relaunch, deduplicate and prune missing paths.

**Proof:** core tests for ignore rules, caps, cancellation and recents; manual
step opening a file outside the root by typed path, quitting and relaunching.

### A5 — Create a new Markdown file

`⌘N` opens an in-app sheet with:

- a location (default: current root, or active document's folder; changeable
  with the native directory panel);
- a name field that validates non-empty, no path separators, no `.`/`..`,
  appends `.md` when there is no extension, and refuses names that already
  exist until the user confirms overwrite;
- an optional frontmatter template (off by default), producing
  `---\ntitle: <stem>\ncreated: <YYYY-MM-DD>\n---\n\n# <stem>\n\n`;
- creation via `create_new` semantics and the atomic write path.

After creation the file is open in Source mode with the caret in the body.

**Proof:** integration test `new_file_*`; validation step with `xxd` of the
created file; screenshot of the sheet.

### A6 — Edit and save

The source editor provides cursor and selection, undo/redo, find (no replace),
a dirty indicator per tab, `⌘S`, `⇧⌘S`, `⌘E` preview toggle, and a live outline
that follows unsaved edits. Closing a dirty tab asks Save / Discard / Cancel;
closing the window or quitting asks Save All / Discard All / Cancel. The
rendered preview uses the PoC's `TextView` path and keeps its strengths.

**Proof:** validation screenshots (editor dirty, preview, conflict, new file);
manual undo/redo and find; `cargo test --workspace` for outline/checkbox logic.

### A7 — Save safety

Non-negotiable, with tests:

1. **Exact bytes.** UTF-8 BOM presence preserved; LF, CRLF and lone CR
   preserved; trailing-newline presence preserved; untouched lines of a
   mixed-ending file keep their terminators. Saving a clean buffer does not
   touch the file.
2. **Atomic.** Sibling temp file, `sync_all`, mode preserved, `rename`, parent
   directory fsync. Injected failure before rename leaves the original intact
   and removes the temp.
3. **Conflict.** At save, compare `(dev, ino, len, mtime)` with the stamp taken
   at load. Dirty buffer + changed disk → Reload / Overwrite / Save As / Cancel;
   clean buffer → reload. Deleted and replaced files are reported distinctly.
4. **Encoding refusal.** UTF-16 and invalid UTF-8 are opened read-only with the
   byte offset/encoding named; no write path exists.
5. **Limits.** 256 KiB preview cap; 2 MiB editable; 2–8 MiB read-only source;
   8–16 MiB preview-only; above 16 MiB refused. Checkbox writes allowed through
   the same path up to 8 MiB.
6. **Checkbox write-back** works in preview/read mode, preserves `[X]` case,
   and goes through the conflict-checked atomic write.

**Proof:** the test table in ADR 0003, all green in `cargo test -p folio-core`
and `cargo test -p folio-core --test save`; validation with `xxd` before/after
on CRLF+BOM and non-UTF-8 fixtures.

### A8 — Reader parity and session restore

The following are present and behave as in the PoC: file tree, tabs with
disambiguated labels, heading outline, find with match count and highlights,
fuzzy quick-open, live reload for clean files, task checkboxes, light/dark
theme, window geometry, sidebar width, sidebar visibility, outline visibility.

Session restore additionally preserves: recent files and folders, per-tab source
or preview mode, per-tab scroll position, document font size, and last root
(when the setting is on). Missing files are dropped from the session without
losing the rest.

**Proof:** validation checklist and screenshots after a quit/relaunch cycle;
session round-trip tests in `folio-core`.

### A9 — Settings

A settings sheet (not a separate window) exposes:

- theme: light / dark / system (system follows the macOS appearance);
- document font size: 12–24 pt, applied to the reader and preview (not the
  whole UI);
- reduced motion: inherited from the system through `gpui-base`, with no app
  animation that ignores it;
- remember last root: on/off.

All settings persist in the session file and survive relaunch.

**Proof:** validation screenshots in light and dark; relaunch; core session test
for the fields. Reduced motion is verified by toggling the macOS setting and
observing that built-in transitions are suppressed; if that cannot be observed
reliably, the limitation is stated instead of claimed.

### A10 — Ship

- Public repository under `mrsase`; `mrsase/folio` was free at planning time
  (checked via the GitHub API on 2026-11-20). If it becomes unavailable, ship as
  `folio-md` and say so.
- MIT for original code; Apache-2.0 attribution for the vendored GPUI snapshot
  in `THIRD_PARTY_NOTICES`.
- README as a release page: what it is, install/build, shortcuts, measured
  performance, honest limits, screenshots, and the `poc-archive` note.
- Topics: `rust`, `macos`, `markdown`, `gpui`, `desktop-app`, `editor`,
  `reader`.
- `v0.1.0` tag and release notes from `CHANGELOG.md`.
- Unsigned `.app` zip with Gatekeeper instructions if packaging works;
  otherwise source plus exact steps, stated plainly.
- CI green on the tagged commit.

**Proof:** repository URL, CI run URL, release URL, `docs/release.md`.

## Gate commands

| Gate | Command |
| --- | --- |
| Format | `cargo fmt --all --check` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` |
| Unit + integration | `cargo test --workspace` |
| Build | `cargo build --release --locked` |
| CI | GitHub Actions `ci` workflow on `macos-latest` |
| Manual UI | `docs/validation.md`, screenshots in `docs/screenshots/` |

## Definition of done

All A1–A10 have their proof; `docs/verification.md`, `validation.md`,
`evaluation.md`, `release.md` contain observed results with dates and machine;
no claim in the README lacks a number or a test; `poc-archive` still points at
the imported proof of concept; no file outside `md-viewer-rs` was modified.
