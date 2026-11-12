# Tasks

## 1. Port

- [x] 1.1 Cherry-pick the streaming lane (jolyne radius cap first), resolving onto main's storage-error and held-chunk behaviour
- [x] 1.2 Cherry-pick the loading lane, the dependent worldgen perf commits and the `stab-load` follow-ups
- [x] 1.3 Background unload saves hand a failed column back (`HeldColumn`)

## 2. Records

- [x] 2.1 `performance.chunk_streaming` in `docs/parity/systems.toml`; `cargo run -p parity -- generate` and `-- check`
- [x] 2.2 Quirk row "World / chunk streaming" in `docs/quirks/bds.md`; `crates/unastar/README.md`; solution notes

## 3. Validate

- [x] 3.1 `cargo fmt --all -- --check`
- [x] 3.2 `cargo clippy -p unastar -p jolyne --all-targets -- -D warnings` and the jolyne feature combinations
- [x] 3.3 `cargo test -p unastar --lib --tests`, `cargo test -p jolyne`, `cargo test -p parity`
- [x] 3.4 `openspec validate --all --strict`; run `review-change`
