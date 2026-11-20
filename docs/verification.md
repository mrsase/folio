# Verification log

Chronological evidence. Phase 6 replaces the provisional sections with the final
gate run; nothing here is a claim about untested code.

## 2026-11-20 — audit of the proof of concept

**Machine:** macOS 14 (x64), 8 logical cores, stable Rust 1.95.0
(`rustc 1.95.0 (59807616e 2026-04-14)`).

| Check | Command | Result |
| --- | --- | --- |
| Workspace size | `wc -l src/*.rs` | 3,653 lines in 8 files (`app.rs` alone 1,902) |
| Tests | `cargo test --lib --bins` | **30 passed**, 0 failed (23 lib + 7 bin) in 0.01 s after build |
| Existing release binary | `ls -lh target/release/folio` | 26 MB, built 2026-11-16 |
| Existing debug binary | `ls -lh target/debug/folio` | 37 MB, built 2026-11-16 |
| Locked dependency graph | `cargo metadata --locked` | exit 0; 4,066,273 bytes of metadata; 48 s (registry index update) |
| Incremental release build | `cargo build --release --locked` | exit 0; 1 m 27 s, dominated by the thin-LTO final link |
| Git | `git log --oneline` | `fd8525e` PoC, tagged `poc-archive`, branch `poc`; docs on `main` |
| GitHub name | `gh repo view mrsase/folio` | not found → name is free |
| Daily-use evidence | `~/Library/Application Support/Folio/session.json` | 12 open files, root outside the repo, 1536×970 window, theme `system` |

### Clean-clone build: not yet proven

A source-only copy was made in `/tmp` and `cargo build --release --locked` was
started. It was **stopped deliberately at 6 m 41 s** (about 24 minutes of CPU) to
respect the maintainer's resource constraint; it had not failed. The copy was
removed.

What is proven instead:

- the locked dependency graph resolves on this machine (`cargo metadata
  --locked`, exit 0);
- the current tree builds in release with the lockfile, using the existing
  `target/` (incremental, exit 0);
- `vendor/gpui-pre-macos` is present, hashed in `Cargo.lock` via the path patch,
  and committed.

What is **not** proven: that a machine with an empty `target/` and an empty
Cargo registry cache can build the tree in one command. That proof is assigned
to the `macos-latest` CI job (criterion A1) and to an optional local clean build
only if the maintainer asks for one. It will not be claimed before then.

### Known PoC defects confirmed by reading the write path

- `src/model.rs:150` strips the UTF-8 BOM and rewrites CRLF/CR to LF on load.
- `src/app.rs:512` writes that normalized buffer back with `std::fs::write`.
- Therefore toggling one task checkbox can rewrite every line ending and drop
  the BOM. No temp file, no fsync, no rename, no conflict check.
- The watcher calls `reload_if_stale()` unconditionally; with an editable buffer
  this would clobber unsaved work.

These are the reasons for ADR 0003 and are covered by its required tests.

## Provisional gates for the rebuild

| Gate | Status |
| --- | --- |
| `cargo fmt --all --check` | not yet applicable (workspace not created) |
| `cargo clippy --workspace --all-targets -- -D warnings` | not yet applicable |
| `cargo test --workspace` | not yet applicable |
| `cargo build --release --locked` | proven on the PoC tree only |
| CI green on `main` | workflow not yet written |
| Clean-clone build | deferred to CI (see above) |
