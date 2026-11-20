# Implementation plan

**Status:** awaiting approval of ADR 0001, 0002, 0003. No app code is written
before that.

## Repository state

- PoC imported on `main`, branch `poc` and annotated tag `poc-archive` point at
  the same commit (`fd8525e`). `vendor/` is committed; `target/` and session
  files are ignored.
- GitHub `mrsase/folio` is free (checked 2026-11-20). Fallback name: `folio-md`.

## Target layout

```
md-viewer-rs/                  # repository root
  Cargo.toml                   # [workspace] members = ["folio-core", "folio-app"]
  rust-toolchain.toml          # pinned stable (1.95.0, the compiler that built the PoC)
  .cargo/config.toml           # jobs = 6
  vendor/gpui-pre-macos/       # load-bearing stable-Rust patch, committed
  folio-core/                  # no GPUI; all tests that matter
    src/{lib,document,encoding,atomic,conflict,checkbox,outline,file_tree,
         index,paths,recents,session,util}.rs
    tests/{save,create,paths}.rs
  folio-app/                   # GPUI views and wiring only
    src/{main,app,keys,menu,session_glue}.rs
    src/views/{browser,tabs,reader,editor,palette,find,new_file,settings,dialogs}.rs
  docs/{product,v1-design,test-plan,verification,validation,evaluation,release}.md
  docs/adr/000{1,2,3}-*.md
  LICENSE  THIRD_PARTY_NOTICES  CHANGELOG.md  README.md
```

`folio-app` keeps the PoC's "one stateful view" discipline but splits rendering
into named view functions/modules; no file returns to 1,900 lines.

## Phases and gates

Each phase is one branch, squash-merged into `main` with a conventional commit.

### Phase 0 — design (current)

- ADR 0001 editing model, ADR 0002 file/root model, ADR 0003 save safety.
- `docs/product.md`, `docs/v1-design.md` (A1–A10), `docs/test-plan.md`.
- Gate: maintainer approval. No code.

### Phase 1 — workspace + `folio-core`

Branch `phase/1-core`.

1. Convert the root to a workspace; move the PoC binary to `poc-archive`
   (already tagged) and stop compiling it. `folio-app` starts as a minimal
   GPUI shell so the workspace has a binary.
2. Port `file_tree`, `outline`, `session`, `util` and the pure `model` tests into
   `folio-core`, adapting names only.
3. Implement `document` (encoding/newlines), `atomic`, `conflict`, `checkbox`,
   `paths`, `recents`, `index`.
4. Integration tests with `tempfile`.

Gate: `cargo test -p folio-core` green; no GPUI compile needed. Count every
ported PoC test and keep the mapping in `docs/verification.md`.

### Phase 2 — app shell + reader parity

Branch `phase/2-reader`.

- GPUI window, menus, shortcuts, theme.
- Tree, tabs, reader (`TextView` port), outline, find, quick-open, watcher,
  session restore.
- Native `⌘O`/`⇧⌘O` and reveal commands wired (behaviour lands in phase 4).
- Out-of-tree tab rendering.

Gate: `cargo check -p folio-app`; one debug build; manual smoke of the reader
against `docs/specimen.md`.

### Phase 3 — editor + save safety

Branch `phase/3-editor`.

- `EditorState` per tab, source/preview toggle, dirty tracking, undo/redo,
  find in both modes, outline from buffer, cursor/scroll persistence.
- Save/save-as, conflict dialog, unsaved-change guards, size policy.
- Checkbox writes through `Document`.

Gate: `cargo test -p folio-core` (save/conflict/checkbox) plus one app build and
a manual edit/save with `xxd` on a CRLF+BOM fixture. Measure typing and save
latency at 2 MiB and record it.

### Phase 4 — open, root, recents, new file

Branch `phase/4-open`.

- Typed paths (`~`, absolute, relative), drag-drop, recents UI and menu,
  "Reveal in Finder", "Open containing folder", root switching.
- New File sheet with validation, overwrite confirmation, frontmatter template.
- Welcome screen for first launch.

Gate: validation steps 1–4 and 6; core tests for paths/recents/index.

### Phase 5 — settings and session polish

Branch `phase/5-settings`.

- Theme, document font size, reduced motion, remember last root.
- Versioned session schema; per-tab mode/scroll; window geometry.
- Empty/error states, toasts, keyboard shortcut sheet.

Gate: relaunch test; session round-trip tests.

### Phase 6 — verify

Branch `phase/6-verify`.

- One `cargo fmt --all --check`, one `cargo clippy --workspace --all-targets --
  -D warnings`, one `cargo test --workspace`, one `cargo build --release`.
- CI workflow; fix what it finds.
- `docs/verification.md` with commands, timings, counts, machine.
- `CHANGELOG.md`, licenses, `THIRD_PARTY_NOTICES`, `rust-toolchain.toml`.

Gate: all commands exit 0 locally; CI green on `main`.

### Phase 7 — validate, evaluate, ship

Branch `phase/7-release`.

- Drive the real app through `docs/validation.md`; capture screenshots.
- `docs/evaluation.md`: LOC, test count, cold/warm build, binary size, startup
  to first document, largest editable file, go/no-go against A1–A10.
- Rewrite README as the release page.
- Package an unsigned `.app` zip if `cargo bundle`/manual bundling works;
  otherwise document source steps.
- Create `mrsase/folio`, push, set topics, tag `v0.1.0`, publish release notes.
- Confirm CI on the tagged commit.

Gate: public repo + green CI + `v0.1.0` + evidence documents.

## Compile budget

Given the low-resource machine:

| Work | Compile |
| --- | --- |
| Docs, ADRs, README | none |
| Core edits | `cargo check/test -p folio-core` (no GPUI) |
| App edits | batched; `cargo check -p folio-app` at slice boundaries |
| UI iteration | one debug build per phase, reused incrementally |
| Full gates | once in phase 6 |
| Clean clone | CI only |
| Release build | once in phase 6/7 |

The 5.4 GB `target/` directory is retained. It already contains the GPUI
dependency graph; deleting it would cost a 6–13 minute rebuild.

## Risks and mitigations

| Risk | Mitigation |
| --- | --- |
| GPUI editor API differs from the crate source read at planning | Phase 3 compiles a minimal editor first; fallback is `Textarea` + app undo. |
| `TextView` scroll offset is internal | Wrap in an outer scroll container with a `ScrollHandle`; if that breaks selection, store a source-offset anchor and restore with `reveal_range`. Verify manually. |
| `on_window_should_close` + `⌘Q` menu path differ | Bind `⌘Q` to an app action that runs the guard, and test both the red button and the menu. |
| Mixed line endings | Common prefix/suffix preservation; documented; tested. |
| CI runner cannot build GPUI | Split core/full jobs and state the limitation; never fake a green app build. |
| Clean-clone build unproven locally | CI is the proof; a local clean clone is optional and only on request. |
| Naming collision on GitHub | Fallback `folio-md`, reported. |
