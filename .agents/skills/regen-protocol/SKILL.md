---
name: regen-protocol
description: Regenerate and verify the Valentine Bedrock protocol crate after changing valentine_gen, its overrides/corrections, or the protocolgen submodule pin. Use when the task touches crates/valentine_gen, when generated protocol code looks wrong, or when CI's "Verify checked-in protocolgen output" step fails.
---

# Regenerate the protocol crate

Generated code (`crates/valentine/bedrock_versions/v1_26_51`, `crates/valentine/src/bedrock`) is
never edited by hand. Read `crates/valentine_gen/AGENTS.md` first.

## Steps

1. Make the change in the generator (parser, IR analysis, emitter) or in a correction file:
   - capture-proven protocolgen defect → `crates/valentine_gen/src/parser/protocolgen_corrections.rs`
     (pin the replaced node + proving protocol version) and a report in `PROTOCOLGEN_ISSUES.md`;
   - Mojang/Endstone source fix → `overrides/` or `overrides-endstone/`, fail-closed, with a `why`.
2. Lint and test the generator:

   ```sh
   cargo clippy -p valentine_gen --all-targets -- -D warnings
   cargo test -p valentine_gen
   ```
3. Regenerate exactly like CI:

   ```sh
   cargo run -p valentine_gen -- --latest
   cargo fmt --all
   git diff --stat -- crates/valentine/bedrock_versions/v1_26_51 crates/valentine/src/bedrock crates/valentine/Cargo.toml
   ```
4. **Explain the diff.** Every changed generated line must follow from your change. Unrelated
   churn means non-determinism or a wrong submodule checkout (`git submodule update --init --recursive`).
5. Validate consumers:

   ```sh
   cargo test -p valentine
   cargo clippy -p jolyne --all-targets --all-features -- -D warnings
   ```
   If packets changed shape, also `cargo check -p unastar --all-targets`.
6. If a packet's support changed, update `docs/parity/protocol.toml` (`parity-update` skill).
7. If the change alters a contract in `openspec/specs/protocol-generator` or `protocol-codec`,
   it belongs in an OpenSpec change with a delta spec.

Never fall back to guessing a wire type or ordinal; make generation fail and fix the source.
