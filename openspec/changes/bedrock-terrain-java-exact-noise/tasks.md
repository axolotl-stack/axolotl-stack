# Tasks

## 0. Decide

- [x] 0.1 Decision proposed as record 0053
- [ ] 0.2 Owner accepts 0053 (set `status: accepted`; 0032 becomes `superseded by 0053`)

## 1. Port

- [x] 1.1 Port #71's `848eb542`, `d49d66b1` and `e743e494` onto main
- [x] 1.2 Re-baseline `crates/unastar/tests/fixtures/worldgen_output_hash_bedrock.json`
  (identical to #71's Bedrock lane tip)
- [x] 1.3 Worldgen parity rows updated; `cargo run -p parity -- generate` and `-- check`

## 2. Validate

- [x] 2.1 `cargo fmt --all -- --check`
- [x] 2.2 `cargo clippy -p unastar --all-targets --features bedrock-worldgen -- -D warnings`
- [x] 2.3 `cargo test -p unastar --features bedrock-worldgen --test worldgen_output_hash --test bedrock_worldgen`
- [x] 2.4 `openspec validate --all --strict`

## 3. Archive

- [ ] 3.1 Archive after 0.2: merge the delta into `openspec/specs/worldgen-pipeline/spec.md`
- [ ] 3.2 Follow-up: remove the calibrated backend once nothing outside tests uses it
