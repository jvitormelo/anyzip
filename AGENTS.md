# AGENTS.md

Rules for this repo (humans and agents). Context and architectural decisions live in `README.md`
("Why it is built this way").

## Layout

- `crates/anyzip`: the core. `#![forbid(unsafe_code)]`.
  - `detect`, `extract`, `extract_each` and `list`.
  - Limits, paths, and one module per format.
- `crates/unrar-sys`: vendored UnRAR plus FFI. The only crate with C++ FFI.
- `crates/anyzip-napi`: the N-API binding. It converts types only; no logic.
- `npm/`: the `anyzip` package.
  - The `anyzip.js`/`anyzip.d.ts` wrapper.
  - The tests, which run under both `bun test` and `node --test`.
- `fixtures/`:
  - `libarchive/` and `generated/` are committed.
  - `expected/` holds the golden files.
  - `real/` is git-ignored: real editais never go in git.
- `bench/`, `fuzz/`, `scripts/`.

## Rules

- **Lints:**
  - `cargo clippy --all-targets -- -D warnings` with `clippy::pedantic`.
  - Exceptions use `#[expect(..., reason = "...")]`, never a bare `allow`.
- **No `unwrap()`/`expect()` outside tests.** Errors are typed (`anyzip::Error` with an
  `ErrorCode`).
- **`unsafe` only in `*-sys` and `*-napi`,** each block with a `// SAFETY:` comment.
- **Hostile input never panics through the API.** Engines run under `catch_unwind`, but a panic
  found by fuzzing is still a bug to fix.
- **Limits count decoded bytes.** Never trust a header's size beyond an allocation hint.
- **Golden changes need review:** `UPDATE_EXPECTED=1 cargo test -p anyzip --test golden`, then read
  the diff of `fixtures/expected/`.
- **A new difference from bsdtar** either gets fixed or gets an entry with its reason in
  `fixtures/diffs.json`.
- **A fuzz crash becomes a fixture** in `fixtures/generated` (via `scripts/make-fixtures.py`) plus
  its golden file.
- **Decisions that don't fit in code** go to "Why it is built this way" in `README.md`.
- **Performance changes come with numbers.** Run `bench/bench.ts` (Bun and Node) or
  `target/release/examples/dump` and update `bench/BASELINE.md`.

## Before finishing a change

```sh
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cargo build --release -p anyzip --example dump && python3 -I scripts/diff-bsdtar.py $(ls -d fixtures/libarchive/* fixtures/generated/* | grep -vE '/(COMMIT|COPYING)$')
cd npm && bun run build && bun run typecheck && bun test && node --test test/*.test.ts
```
