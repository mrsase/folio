# Folio — product

## What it is

A native macOS Markdown reader and editor. One window, a real folder tree,
tabs, a heading outline, find, fuzzy quick-open, live reload, and a source
editor with a rendered preview. GPU-rendered through GPUI; no web view, no
Electron, no telemetry, no network.

## Who it is for

People who keep Markdown in folders and read or edit it every day: engineering
notes, ADRs, runbooks, specifications, a personal knowledge base. They already
have a text editor; what they lack is a fast way to *navigate* a folder of
documents, read one properly, make a small correction, and save it without
thinking about encodings or losing the file.

The proof-of-concept session file is the evidence: 12 open documents, a root
outside the app repository, a restored 1536×970 window, and a recorded theme
preference. The app is used, not demonstrated.

## Scene

- A folder of project docs is open in the sidebar. The user types `⌘P`, three
  letters, and lands on the right file.
- They read a long specification with tables, code fences and task lists,
  jump through its outline, and find a phrase.
- They spot a wrong sentence, press `⌘E`, fix it in the source, press `⌘S`, and
  go back to reading. The file keeps its BOM, its CRLF endings and its trailing
  newline.
- A second document lives outside the root. They open it by path from `⌘O`; it
  stays as a tab marked "outside root" and does not disturb the tree.
- They create a new ADR with `⌘N`, choose the folder, name it, and start typing.
- The next morning, the same window, root, tabs, active document, scroll
  position, sidebar width and theme come back.

## Product principles

1. **The file is the source of truth.** Folio never owns a document. It edits
   bytes on disk, preserves their encoding details, and refuses rather than
   guesses when it cannot do so safely.
2. **Reading first.** Opening a document is never slower or noisier than it was
   in the reader. The editor is one keystroke away, not the default surface for
   every file.
3. **No silent data loss.** Atomic saves, conflict detection, unsaved guards and
   explicit limits. A crash may lose the last keystroke; it never truncates the
   file.
4. **Boring technology.** Pure-Rust core for logic, GPUI only for presentation.
   No plugin system, no sync, no database, no background indexing of file
   contents.
5. **Measured claims.** Performance, limits and test counts in the README are
   numbers produced by the commands in `docs/evaluation.md`, on a named machine.

## Constraints

- macOS only. Built and tested on macOS 14 (x64); the GPUI stack also targets
  Apple Silicon, but arm64 will only be claimed after a build and a run.
- Rust stable, pinned in `rust-toolchain.toml`, because the GPUI dependency
  graph needs a stable compiler and a vendored patch (`vendor/gpui-pre-macos`).
- One window. Multiple windows are deferred because the session, watchers,
  menus and quit guards would all need per-window state.
- The preview render cap stays at 256 KiB, the PoC's measured limit.
- The source editor is capped at 2 MiB and degrades to read-only above it.
- No network access at runtime. Remote images in a document still use GPUI's
  loader only when the document asks for one; nothing is uploaded.

## v1 scope

1. Open any Markdown file (`⌘O`, typed path, drag-drop, CLI) or folder (`⇧⌘O`),
   with recent files and folders.
2. Create a new Markdown file (`⌘N`) with a validated name, a location, an
   overwrite confirmation and an optional frontmatter template.
3. Edit and save (`⌘S`, `⇧⌘S`), with dirty indicators, undo/redo, find, outline
   tracking and unsaved-change guards.
4. Reader parity: tree, tabs, outline, find, fuzzy quick-open, live reload,
   task checkboxes, session restore including per-tab scroll.
5. Settings that matter: theme (light/dark/system), reduced motion respected,
   document font size, remember last root.
6. Safety: exact-byte preservation, atomic writes, external-change conflicts,
   non-UTF-8 refusal, size limits.

## Non-goals for v1.0

Each is deferred for a stated reason, not forgotten.

| Non-goal | Why deferred |
| --- | --- |
| WYSIWYG editing | A Markdown serializer that round-trips every GFM construct is a project of its own and the main way documents get corrupted (ADR 0001). |
| Split view / side-by-side preview | Useful, but it doubles layout, scroll and find state; source+preview toggle covers the need at a fraction of the risk. |
| PDF / print export | Needs pagination and print CSS outside GPUI's renderer; not a reading/editing requirement. |
| Plugins | No plugin ABI, no sandbox, no versioning. A security and maintenance surface with no v1 user. |
| Sync / cloud | Conflicts, accounts, encryption, privacy. The product is local-first by design. |
| iOS | GPUI's mobile support is not the same product; input, menus and file access differ. |
| Code signing / notarization | Requires a paid Developer ID. v1 ships source and, if packaging works, an unsigned app with Gatekeeper instructions. |
| Multiple windows | Session, watchers, menus and quit guards become per-window state (ADR 0002). |
| Full-text search across the home directory | Expensive, privacy-sensitive, and not needed to open a known document; quick-open indexes names only. |

## Success

A stranger clones the repository, builds with the documented command, opens any
folder or file, reads, edits, creates and saves without data loss. CI is green
on the tagged commit. The release page states what was measured, what was not,
and what is deliberately missing.
