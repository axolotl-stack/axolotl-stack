## 1. Claim

- [x] 1.1 Replace the stale "Liquids" claims in `docs/parity/claims.toml` with a lane LIQUIDS
      claim (blocks family `liquids`, systems `world.liquids`; no `change` key, since the change
      is archived in the same pull request)

## 2. Measure

- [x] 2.1 `tools/bds-sweep/liquids.py` families, per-tick markers in `jobs.py`, layouts in `sweep.py`
- [x] 2.2 Run the sweeps on a BDS slot; `summarize.py liquids` into
      `crates/unastar-data/data/measured/liquids/`

## 3. Implement

- [x] 3.1 `unastar-data::liquids` decodes the per-block table
- [x] 3.2 `server/game/liquids/` engine, live system, batching, bucket API
- [x] 3.3 Liquid layer kept by the block write path; block changes wake liquids; coral sees the layer
- [x] 3.4 Replay tests (`server/game/liquids/tests.rs`)

## 4. Record

- [x] 4.1 Parity rows (`docs/parity/systems.toml`, `docs/parity/blocks.toml`), then
      `cargo run -p parity -- generate` and `cargo run -p parity -- check`
- [x] 4.2 Quirks in `docs/quirks/bds.md`, a solution note, decision 0051 (proposed)

## 5. Validate

- [x] 5.1 `cargo fmt --all -- --check`
- [x] 5.2 `cargo clippy -p unastar -p unastar-data -p bds-sweep --all-targets -- -D warnings`
- [x] 5.3 `cargo test -p unastar --lib --tests`; `cargo test -p unastar-data -p bds-sweep -p parity`
- [x] 5.4 `cargo run -p parity -- check`; `openspec validate --all --strict`;
      `bun tools/agents/check-links.ts`; `python -m unittest discover -s tools/bds-sweep -p "test_*.py"`
