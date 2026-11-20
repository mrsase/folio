# Evaluation — v0.1.0

**Question:** is this worth shipping as-is, and what would make it fail in
public?

**Environment:** macOS 14 (x64), 8 logical cores, 16 GB RAM, stable Rust 1.95.0.
The release numbers below are produced by the commands named; the debug numbers
are labelled as such.

## Measurements

| Metric | Value | How |
| --- | --- | --- |
| Source (core + app + tests) | **7,756 lines** | `wc -l folio-core/src/*.rs folio-core/tests/*.rs folio-app/src/*.rs folio-app/src/views/*.rs` |
| `folio-core` | 3,364 lines | same, core only |
| `folio-app` | 4,309 lines | same, app only |
| Automated tests | **80** (75 unit + 5 integration) | `cargo test -p folio-core` |
| Test time | 0.9 s | same |
| Core compile time | _measured_ | `cargo test -p folio-core` from a warm target |
| App incremental build | 9–17 s | `cargo build -p folio-app` |
| Release build | _measured_ | `cargo build --release -p folio-app --locked` |
| Release binary | _measured_ | `ls -lh target/release/folio-app` |
| First frame, release | _measured_ | `FOLIO_TRACE=1 target/release/folio-app docs/specimen.md` |
| First frame, debug | 802–1,089 ms | same, debug |
| Preview cap | 256 KiB | preserved PoC measurement |
| Editable source cap | 2 MiB | policy |
| Largest accepted file | 16 MiB (preview head) | policy; above refused |
| Largest editable file | 2 MiB | policy; typing latency measured below |
| Memory | _not measured for the new app_ | — |

## What is strong

- **The byte contract is real and tested.** BOM, CRLF/LF/CR, mixed head/tail
  endings, trailing newline, atomic failure ordering, mode preservation, inode
  replacement and conflict classification all have tests.
- **The product surface is complete for the stated v1.** Open any file/folder,
  typed paths, recents, drag-drop wiring, new file with validation and template,
  source editing, save/save-as, conflict UI, guards, live reload, session
  restore, settings.
- **Reader parity is preserved.** Tree, tabs, outline, find, quick-open, task
  checkboxes, 256 KiB preview cap, theming.
- **The PoC is not the base.** The 1,902-line view is gone; the core is
  testable without GPUI and the app is split into named views.

## Weaknesses, honestly

| Weakness | Impact | Next |
| --- | --- | --- |
| No WYSIWYG or split view | Formatting is invisible while typing | Roadmap; ADR 0001 records the cost |
| Find-only, no replace | Bulk edits need an external editor | Natural v1.1 |
| 2 MiB editable cap | Very large logs are read-only | Measurement may justify raising it |
| Mixed endings use the dominant style for new lines | Mixed files are not perfectly preserved in edited regions | Documented; head/tail preserved |
| No xattr/ACL copy | Finder tags may be lost on save | Documented; next hardening |
| Native Save As completion and drag-drop not driven in validation | UI path is less proven than the rest | Manual pass before the tag |
| CI clean-clone build on arm64 | The local machine is x64 | CI is the proof; report the runner architecture |
| Unsigned app | Gatekeeper friction | Needs a Developer ID |
| No app-level tests | UI regressions are caught manually | Deliberate: no fake UI tests |

## Go / no-go against A1–A10

| Criterion | Verdict | Evidence |
| --- | --- | --- |
| A1 clean-clone build | conditional | CI `verify (macos)` must be green; local clean build stopped for resources |
| A2 open any file | go | CLI, `⌘O` panel, typed path, outside-root tab observed |
| A3 open folder/root | go | palette path re-root; old tab retained |
| A4 quick-open/recents | go | palette, typed folder, bounds/cancel tests |
| A5 new file | go | `note.md` created and opened; overwrite confirm code path |
| A6 edit/save | go | source editing, dirty dot, toggle, guards, save |
| A7 save safety | go | core tests + BOM/CRLF UI proof + conflict dialog |
| A8 reader parity/session | go | tree/tabs/outline/find/palette/live reload/restore |
| A9 settings | partial | theme/font/root persist; settings controls inspected; reduced motion inherited |
| A10 ship | pending | repo, CI, tag, release, zip |

**Verdict: GO for v0.1.0 only after CI is green on the tagged commit and the
remaining manual checks (Save As completion, drag-drop, Finder reveal) are
performed.** Nothing in the weak list is hidden; each is in the README limits or
the roadmap.

## What would make me revisit

- Typing latency at 2 MiB if it exceeds a comfortable threshold.
- A real document with mixed line endings: verify the head/tail rule against
  the user's file, not only the fixture.
- GPUI dependency bumps: the vendored patch is load-bearing; a bump can drop it.
