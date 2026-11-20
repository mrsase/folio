# Release record — v0.1.0

**Repository:** <https://github.com/mrsase/folio> (public, MIT)
**Tag:** `v0.1.0`
**Date:** 2026-11-21

## Gate results at the tagged commit

| Gate | Result |
| --- | --- |
| `cargo fmt --all --check` | pending final run |
| `cargo clippy --workspace --all-targets -- -D warnings` | pending |
| `cargo test --workspace` | pending |
| `cargo build --release --locked` | 7 m 30 s locally (thin LTO) |
| GitHub Actions `CI` | pending |
| Data hygiene | no `target/`, session files, or `~/Library` content tracked |

## What shipped

- Workspace: `folio-core` (pure logic and tests) and `folio-app` (GPUI).
- The proof of concept remains at tag `poc-archive` / branch `poc`.
- Screenshots and a visible validation record in `docs/`.
- Unsigned `.app` zip produced by `scripts/package.sh` when packaging works;
  otherwise source plus the exact build steps in README.

## Known limits at this tag

- Save As panel completion and drag-and-drop were wired and inspected but not
  driven end-to-end in the validation pass; the underlying write path is
  covered by integration tests.
- 2 MiB editing was accepted; typing latency at that size was not instrumented.
- macOS only; unsigned; no WYSIWYG, split view, print, plugins, sync, iOS or
  multiple windows.

## First-week watch list

- Whether macOS Sequoia/Tahoe changes NSOpenPanel/NSSavePanel behaviour.
- Whether a GPUI bump drops the vendored `gpui-pre-macos` patch.
- Reports of mixed line endings where a file's dominant style is ambiguous.
- Feedback on the 2 MiB editable cap and the 256 KiB outline/preview cap.
