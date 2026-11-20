# ADR 0002 — File and root model

**Status:** proposed · **Date:** 2026-11-20 · **Deciders:** Folio maintainers

## Context

The PoC can only reach the folder passed on the command line. Session restore
records one root and its open files (`src/session.rs`), the tree is lazy
(`src/file_tree.rs`), and quick-open performs a bounded recursive walk. There is
no open panel, no recents list, no drag-and-drop, no path typing beyond a
directory typed into quick-open, and no concept of a file outside the root.

v1 must open any Markdown file on the machine and any folder, without a restart,
and without losing open tabs or unsaved work.

## Options

### Window model

| Option | Trade-off |
| --- | --- |
| One window, switchable root (chosen) | Matches the existing session model and GPUI wiring; out-of-tree tabs cover reading across folders; one place for quit guards and settings. Cannot compare two roots side by side. |
| Multiple windows, one root each | Natural for comparison, but multiplies session, menu, watcher and quit-guard state; GPUI multi-window needs a per-window entity graph. Deferred to the roadmap. |
| One window per root, no switching | Forces a relaunch to change folders; rejected. |

### Opening files

Native panels are available in GPUI 0.2.2 and verified in its macOS backend:
`App::prompt_for_paths` (NSOpenPanel), `App::prompt_for_new_path` (NSSavePanel),
`App::reveal_path` (Finder) and `App::open_with_system`. No shelling out to
`open`, no third-party dialog crate.

### Root indexing

The PoC's tree reads only expanded directories. Quick-open walks the root with a
depth cap and a name-based skip list, but it has no cancellation and no limit on
entries scanned. Opening `$HOME` as a root is possible from the CLI, so the new
index must be bounded and cancellable, and must never scan the whole home
directory just because the app launched.

## Decision

**One window with switchable roots.** A root is a folder that the tree and
quick-open index. Open tabs are independent of the root.

### 1. Opening anything

| Entry point | Behaviour |
| --- | --- |
| `⌘O` | Native open panel, files only, multiple selections allowed. Each selected Markdown file opens in a tab. Non-Markdown files are refused with a visible reason. |
| `⇧⌘O` | Native open panel, directories only. First selection becomes the root; the tree rebuilds without a restart. |
| `⌘P` quick-open | Fuzzy over the current root's index. The query is also parsed as a path: `~/…`, `/abs/…`, or a path relative to the root. A file opens as a tab; a directory becomes the root; `~` alone opens the home folder as root. |
| Drag and drop | A file or files open as tabs. Exactly one folder becomes the root. A mixed drop opens the files and ignores the folder, with a toast. Files that are not Markdown are refused with a toast. |
| CLI | `folio <file>` opens the file; `folio <dir>` opens the folder. With no argument, restore the session; on first run show the welcome screen. |
| Recent files / folders | Palette empty state lists up to 20 of each, newest first. Selecting a missing entry removes it and says so. |
| Reveal in Finder | `App::reveal_path` on the active document. |
| Open containing folder | Sets the root to the document's parent and reveals the document in the tree. If the document is already in the root, just reveal it. |

**Out-of-tree tabs:** a file opened by path that is not under the root stays
open as a normal tab, labelled with enough of its path to disambiguate it (for
example `big-blue / PLAN.md`), and marked with an "outside root" indicator plus
its full path in an outside-root banner. The tree does not list it; "Open containing
folder" is the way to bring its folder into the tree. Saving it is unaffected by
the root.

**Root changes preserve everything:** all tabs, unsaved buffers, editor history
and scroll positions survive `⇧⌘O`, a typed path, a drop, or `⌘P`. There is no
"close all tabs?" prompt. An unsaved untitled document keeps its "Untitled"
identity and is outside every root until saved.

### 2. Tree

- Lazy: one `read_dir` per expanded directory; collapsed directories are never
  read. Directories first, case-insensitive name order.
- Hidden entries (leading `.`) hidden by default, toggled with `⇧⌘.`; `.DS_Store`
  is always skipped. Symlinked directories are shown but never followed.
- A failed `read_dir` (permissions, deleted folder) yields an inline row error,
  not a modal and not a crash.
- Filter box keeps matching files and their ancestors, as in the PoC.
- The tree is rooted at one folder. There is no synthetic multi-root tree in v1.

### 3. Quick-open index

A background walk, one at a time, owned by the app:

| Rule | Value |
| --- | --- |
| Traversal | Depth-first, directories only, symlinks never followed |
| Depth cap | 12 below the root |
| Entry cap | 50,000 directory entries examined |
| Result cap | 20,000 Markdown files |
| Soft deadline | 3 seconds; partial results are used and marked "partial" |
| Ignored names | `.git`, `.hg`, `.svn`, `node_modules`, `target`, `Library`, `.Trash`, `.venv`, `venv`, `__pycache__`, `dist`, `build`, `DerivedData` |
| Cancellation | A generation counter; a root change or quit makes the old walk drop its results. The worker checks the counter every directory. |
| Home directory | Never scanned implicitly. Opening `$HOME` as a root is allowed, but the caps and ignore list still apply, and the UI says when the index is partial. |

The index is for navigation only. It never reads file contents, so a large root
cannot cause a parse storm.

### 4. Recents

- Two MRU lists in the session file: 20 files, 20 folders. Deduplicated by
  canonical path, newest first, pruned when missing at load.
- Recents are updated on open, on save-as, and on root change — never on mere
  hover or scan.
- The palette shows them when the query is empty; a separate File → Open Recent
  submenu mirrors the file list.

### 5. Path handling

`folio-core::paths` owns one normalizer used by every entry point:

- `~` and `~/…` expand from `$HOME`; `~user` is unsupported and reported.
- Relative paths resolve against the current root, or the active document's
  directory when there is no root.
- `.` and `..` are normalized lexically, then the result is canonicalized when
  it exists (so symlinked paths compare equal to their target).
- Display shortens the home prefix to `~` and disambiguates duplicate file
  names by parent. Display never changes the path used for I/O.
- A nonexistent typed path is reported inline in the palette; Enter does nothing.

### 6. First run and last root

On launch with no argument and no saved session, show a welcome screen with
Open File / Open Folder / New File, recent lists, and the shortcut sheet. Do not
silently open `$HOME`. "Remember last root" is a setting (default on); when off,
launch restores tabs but no root.

## Consequences

- `folio-core::index` and `folio-core::recents` are testable without GPUI:
  ignore rules, depth/entry caps, cancellation, MRU ordering, path expansion.
- The app keeps a single `AppState` with a root, an index generation, tabs and a
  session; no per-window entity graph.
- Multi-window is a roadmap item; the session schema has a `version` field so it
  can grow windows later.
- A user who opens a 500k-file home folder gets a partial index with a stated
  reason, not a frozen window.
