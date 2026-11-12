# Tasks

- [x] 1.1 Rows are covered by lane BLOCKS's existing claim (`docs/parity/claims.toml`); no new claim
- [x] 1.2 Pilot the facing and timing; push, per-block, slime, power, timing, chain and actor families (`tools/bds-sweep/pistons.py`); piston click captures (`placements.py` `pistons`)
- [x] 1.3 `summarize.py round4` to `data/measured/pistons/`; `unastar_data::pistons`
- [x] 2.1 `server/game/redstone/pistons.rs`: power in the consumer phase, structure resolution, moves, landing, actors and sounds
- [x] 2.2 Live world: breaking, block actors (containers move with their contents), sounds
- [x] 2.3 Send each block change once
- [x] 3.1 Replays of every piston family; drops of broken blocks; packet comparison with the watcher capture
- [x] 3.2 Update `docs/parity/blocks.toml` and `systems.toml`; `cargo run -p parity -- generate` and `-- check`
- [x] 3.3 Quirk rows, solution note, decision 0050
- [x] 3.4 Validation: `cargo fmt --all -- --check`, `cargo clippy -p unastar -p unastar-data -p bds-sweep --all-targets -- -D warnings`, `cargo test -p unastar --lib --tests`, `cargo test -p unastar-data -p bds-sweep -p parity`, `cargo run -p parity -- check`, `openspec validate --all --strict`, `bun tools/agents/check-links.ts`, `python -m unittest discover -s tools/bds-sweep -p "test_*.py"`
