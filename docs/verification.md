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
- `vendor/gpui-pre-macos` is present, wired through the path patch in
  `Cargo.lock`, and committed. Path packages carry no checksum, so the commit
  itself is the integrity record.

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

## 2026-11-20 — Phase 1: workspace and `folio-core`

**Scope:** workspace, pure-Rust core, ported PoC tests, new byte-safety modules.
No GPUI code was compiled; `folio-app` is an inert binary for now.

| Check | Command | Result |
| --- | --- | --- |
| Core unit tests | `cargo test -p folio-core` | **75 passed**, 0 failed |
| Core integration tests | same | **5 passed**, 0 failed (create/save/save-as/overwrite/refusal) |
| Total for this phase | same | **80 tests**, 0.7 s |
| Lint | `cargo clippy -p folio-core --all-targets -- -D warnings` | ✅ 0 warnings |
| Format | `cargo fmt --all --check` | ✅ clean |
| Toolchain | `rust-toolchain.toml` | `1.95.0` installed and building |
| GPUI compile cost | — | avoided; only `folio-core` was built |

PoC tests ported: file tree (4; the recursive scan case moved to
`index::tests`), outline (5; checkbox cases moved to `checkbox::tests`),
session (4 original cases, extended), util (6), display slicing and find (4),
palette (2). New coverage: encoding/BOM/newline round-trips, atomic failure
ordering, mode preservation, conflict classification, checkbox encoding
preservation, path expansion/canonicalization, ignore rules/caps/cancellation,
recents MRU/pruning, versioned session migration, size policies.

### Provisional gates for the remaining phases

| Gate | Status |
| --- | --- |
| `cargo fmt --all --check` | ✅ for the current tree |
| `cargo clippy --workspace --all-targets -- -D warnings` | core clean; workspace not yet built (GPUI) |
| `cargo test --workspace` | core green; app tests not written |
| `cargo build --release --locked` | proven on the PoC tree only |
| CI green on `main` | workflow not yet written |
| Clean-clone build | deferred to CI (see above) |

## 2026-11-21 — Phase 2–5: app shell, editor, open/new, settings

**Build:** `cargo build -p folio-app` exits 0. Incremental builds after the
dependency graph was warm took 5–17 s; no clean dependency build was repeated
locally (see the resource note below).

| Check | Command / evidence | Result |
| --- | --- | --- |
| Core tests | `cargo test -p folio-core` | **78 unit + 7 integration = 85**, 0 failed |
| App build | `cargo build -p folio-app` | ✅ exit 0, no warnings in the final run |
| Format | `cargo fmt --all` | ✅ applied; `--check` is a CI gate |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` | run in phase 6 / CI |
| First frame, debug | `FOLIO_TRACE=1`, 2 KB document | 802–1089 ms across runs |
| Native panels | `⌘O`, `⇧⌘S` | NSOpenPanel/NSSavePanel appeared; Escape cancels without a crash |
| UI evidence | `docs/screenshots/` | reader dark/light, source editor, new file, conflict, quit guard, settings, palette, find, outline, session restore, live reload |

**Manual observations** (full detail in `docs/validation.md`):

- opening a folder by typed palette path re-rooted the tree and kept the old
  tab, marked `outside`;
- editing a BOM+CRLF file and pressing `⌘S` preserved the BOM and every CRLF;
- an external change while dirty produced the conflict dialog and no write;
- dirty close/quit produced the guards; clean quit exited;
- session restore brought back root, tabs, mode, theme and scroll;
- `⌘E` switched Source/Preview; `⌘F`, `⌘⇧B`, `⌘P` rendered;
- checkbox clicks rewrote only the marker and preserved BOM/CRLF;
- an external append changed the preview's heading count from 8 to 9.

**Not yet proven:** clean-clone build (CI), release-build startup and binary
size (phase 6), Save As completion through the native panel (core integration
test covers the write), drag-and-drop and Finder reveal (wired, not clicked),
>2 MiB editing latency.

### Resource constraint

The maintainer asked to avoid unnecessary compiles on a low-resource machine.
The PoC's dependency graph was reused; only `folio-core` (a few seconds) and the
app incremental link (5–17 s) were compiled repeatedly. A source-only clean
build was started once and stopped deliberately at 6 m 41 s; CI is the clean-clone
proof.

## 2026-11-21 — Phase 6/7: final gates

| Check | Command | Result |
| --- | --- | --- |
| Format | `cargo fmt --all -- --check` | ✅ clean |
| Core tests | `cargo test -p folio-core` | **78 unit + 7 integration = 85**, 0 failed, 0.8 s |
| App build | `cargo build -p folio-app` | ✅ exit 0, no warnings |
| Core lint | `cargo clippy -p folio-core --all-targets -- -D warnings` | ✅ clean |
| Release build | `cargo build --release -p folio-app --locked` | ✅ 7 m 30 s first; 2 m 19 s app-only rebuild |
| Release binary | `stat -f %z target/release/folio-app` | 26,929,828 bytes (25.7 MiB) |
| Startup | `FOLIO_TRACE=1`, release | 782–988 ms (2 KB doc); 693 ms (2 MiB preview) |
| Packaging | `./scripts/package.sh 0.1.0` | ✅ `target/Folio-0.1.0-macos.zip`, 11 MB, ad-hoc signed x86_64 |
| Workspace gate | `cargo test --workspace`, clippy, release | ✅ CI [44823618324](https://github.com/mrsase/folio/actions/runs/44823618324) on `fcff2e5` (27 m 41 s) [44827776596](https://github.com/mrsase/folio/actions/runs/44827776596) (14 m 29 s) and [44829851350](https://github.com/mrsase/folio/actions/runs/44829851350) on the tagged commit `8093ca9` (3 m 51 s); all steps green |

The first CI run failed on four clippy lints (two closures, one `contains`, one
collapsible `if`, one boolean expression), all fixed in `d78dfa3`/`fcff2e5`.
The second run was cancelled by the workflow's `cancel-in-progress` when the
final commit arrived; the final run is the one referenced in `docs/release.md`.

### Environment caveat

The local machine was heavily loaded (load average 15–50) during part of the
work, which made one release-startup sample 3.1–4.1 s. After the load dropped,
the same binary measured 782–988 ms. The lower numbers are reported in the
README; the loaded sample is not.
