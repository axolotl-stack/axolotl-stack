# Contributing

Thanks for helping. Read [AGENTS.md](AGENTS.md) for the workspace layout and conventions;
crate directories have their own READMEs.

## Finding work

[`docs/parity/PARITY.md`](docs/parity/PARITY.md) lists everything vanilla Bedrock 1.26.51
has and how far `unastar` is with each thing. Pick a `missing` or `partial` row; its `gaps`
say what is left.

## The parity rule

**A feature is not finished until its parity rows are updated in the same commit.**

1. Update the affected rows in `docs/parity/<domain>.toml`: `status`, `owner`, `tests`,
   `evidence`, `gaps` (see [docs/parity/README.md](docs/parity/README.md)).
2. `done` needs a real test in the workspace; `verified` also needs committed native
   evidence (a BDS or Java capture or fixture) that the test compares against.
3. Run `cargo run -p parity -- generate`, then `cargo run -p parity -- check`.

## Before opening a pull request

Run what CI runs (`.github/workflows/test.yml`):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p parity -- check
```

plus the `cargo test` commands for the crates you touched.

## Writing about vanilla behaviour

Comments, docs, test names, fixtures and commit messages describe the behaviour our code
implements. Do not write Bedrock server or client internal class, function or symbol names,
addresses or offsets. Black-box provenance ("matches the BDS 1.26.51 capture",
"measured on BDS 1.26.51") is fine.
