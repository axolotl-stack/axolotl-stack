# `valentine`

This crate exposes Bedrock protocol types via feature flags like `bedrock_1_21_130`.

The per-version protocol sources live in `crates/valentine/bedrock_versions/`, but the generated Rust code is not committed to the repository. After cloning, run the generator to produce the version(s) you need.

## Generate protocol code

From the repo root:

```bash
git submodule update --init --recursive
cargo run -p valentine_gen -- --latest
```

## Selecting versions

- Enable a version feature, e.g. `--features bedrock_1_21_130`.
- Import via `valentine::bedrock::protocol::v1_21_130::...` or `valentine::bedrock::version::v1_21_130::*`.

