# Folio

A native markdown reader for macOS, built in Rust on [GPUI Kit](https://gpui-kit.com)
(GPUI + `gpui-base` + `gpui-component`). GPU-rendered, no web view, no Electron.

Folio is to markdown what a spreadsheet app is to CSV: the tool you reach for
when you actually have to *read* the thing — a real file browser instead of the
macOS open panel, tabs, live reload, a heading outline, and in-document search.

## Running it

```sh
cargo run --release            # opens the last session, or $HOME
cargo run --release -- .       # opens this repository as the root
cargo run --release -- docs/specimen.md
```

The window geometry, root folder, open tabs, active document, sidebar width and
theme are restored on launch
(`~/Library/Application Support/Folio/session.json`).

## Keyboard

| Shortcut | Action |
| --- | --- |
| `⌘P` / `⌘⇧O` | Quick open: fuzzy search every markdown file under the root |
| type `/…` or `~/…` in quick open | Open that folder as the new root (no file dialog) |
| `↑` `↓` | Move in the file tree / in quick open |
| `→` / `←` | Expand / collapse a folder |
| `↩` | Open the selected row |
| `⌘B` | Show / hide the file browser |
| `⌘⇧B` | Show / hide the heading outline |
| `⌘F`, `⌘G`, `⌘⇧G` | Find, next match, previous match (all matches highlighted) |
| `⌘W` | Close the active tab |
| `⌘⇧]` / `⌘⇧[`, `⌘1` / `⌘9` | Next / previous / first / last tab |
| `⌘R` | Reload the active document from disk |
| `⌘⇧L` | Toggle light / dark |
| `⌘⇧.` | Show hidden files |
| `esc` | Dismiss quick open or the find bar |

Clicking a task list checkbox rewrites that marker in the file.

## What it renders

Markdown is parsed and laid out by `gpui-base`'s `TextView`, so Folio inherits a
real block layout engine rather than a pile of styled labels:

- CommonMark + GFM: headings, emphasis, strikethrough, tables, task lists,
  autolinks, block quotes, nested lists.
- Syntax-highlighted fenced code (syntect, ~200 languages via `two-face`).
- Tables with header styling and hover rows.
- YAML/TOML frontmatter as a metadata card (simple scalar values; compound YAML
  falls back to a YAML code block).
- Images resolved relative to the document; remote image URLs go through GPUI's
  loader.
- Selectable text (`⌘C` copies exactly what is on screen), `#anchor` links
  resolve to headings, relative `.md` links open in a new tab.

## Verified behaviour

Everything below was exercised against the running app, not inferred:

- file tree over the real filesystem (lazy expansion, filter, hidden-file
  toggle, keyboard navigation, selection follows the open document);
- session restore of two tabs with disambiguated labels, tab close, tab switch;
- outline panel, find bar with match count and highlights, quick open with
  background scan and fuzzy ranking, folder switch by typing a path;
- task checkbox click → marker rewritten on disk; external edit → document and
  tree refresh within ~300 ms;
- light and dark themes, macOS menu bar, traffic lights, window geometry
  restore.

## Architecture

```
src/
  main.rs        window, theme, menu bar, key bindings
  app.rs         the single stateful view (browser, tabs, reader, outline, palette, find)
  model.rs       OpenDoc / Find / Palette — no GPUI types
  lib.rs         file_tree, outline, session, util (headless, unit-tested)
  file_tree.rs   lazy, flattened filesystem tree + recursive markdown scan
  outline.rs     mdast heading extraction, task checkbox edits, link resolution
  session.rs     atomic JSON persistence
  util.rs        fuzzy matching, tab labels, path display
```

Design decisions worth knowing:

- **One stateful view.** `Folio` owns all state; child UI is rebuilt from it each
  frame, so invalidation is a single `cx.notify()` and there is no entity graph
  to keep consistent.
- **Collapsed directories are never read.** The tree is a flat row vector
  rebuilt from the set of expanded paths, so browsing a monorepo costs one
  `read_dir` per directory you actually opened.
- **One parse per document, not per frame.** Each tab owns an
  `Entity<TextViewState>`, so repainting reuses the parsed document, layout and
  selection; a reload only re-parses when bytes actually changed.
- **Window appears before documents load.** Files queued from the CLI or the
  session open on the first frame, so a slow file can never leave a blank
  window.
- **Virtualized rows** for the tree and quick open (`uniform_list`), with
  scroll-follow for keyboard selection.
- **Background work off the UI thread:** quick-open scans, task writes, and the
  filesystem watcher (a 300 ms fold into one update rather than a repaint per
  event).
- **Bounded layout.** The layout engine is fed at most the first 256 KB of a
  document, cut at a paragraph boundary, with a notice appended; the full text
  is kept for writes. Uncapped, a 2 MB file pushed `TextView` layout into
  multi-second stalls and >1 GB of retained layout state.

## Performance

Measured on this machine (release build, 2 KB doc unless noted):

| Metric | Result |
| --- | --- |
| Process start → first frame, first launch after a build | 1520 ms |
| Process start → first frame, warm | 747–894 ms |
| Open 12 KB document (read + parse) | <1 ms + <1 ms |
| Open 2 MB document (read, slice, outline the head) | 605 ms |
| Memory, empty reader | 69 MB |
| Memory, 256 KB document | 252 MB |
| Memory, 2 MB document, uncapped | 1.3 GB and still laying out — why the cap exists |

Build time from scratch with `jobs = 6`: 12m49s for the full dependency tree plus
thin LTO (release profile).

## Development

```sh
cargo test --lib         # headless model tests
cargo test --bin folio   # app model tests
cargo build --release
FOLIO_TRACE=1 ./target/release/folio docs/specimen.md   # startup/parse timings on stderr
```

`.cargo/config.toml` sets `jobs = 6`.

`vendor/gpui-pre-macos` is a copy of the crates.io release with three
`Option::map_or_default` calls replaced by `map_or_else(Default::default, …)`:
that method is still nightly-only, and `gpui-kit` 0.10 pins `gpui-pre-macos`
0.3.12. The patch is wired in `[patch.crates-io]`. Without it the stack does not
compile on stable.

## Limits

- macOS only; the tree, shortcuts and window chrome are tuned for it.
- Read-only except task list checkboxes.
- Documents over 256 KB render their head plus a notice.
- No split view, printing or PDF export in this version.
- A file that is not valid UTF-8 opens a tab that explains why instead of
  rendering garbage.
