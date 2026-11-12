# Tasks

- [x] 1.1 Plate and tripwire families (`tools/bds-sweep/plates.py`); summarize to `data/measured/redstone/plates_*.json`
- [x] 2.1 `redstone/plates.rs`: counting, pressing, checks; plate kind in the circuit's per-state facts; a pressed wooden or stone plate powers 15
- [x] 2.2 Live system `press_plates` before the circuit step
- [x] 3.1 Replay of every plate row from the measured actor positions; a running server pressing a wooden plate with an item
- [x] 3.2 Parity rows, quirk row, `cargo run -p parity -- generate` and `-- check`
- [x] 3.3 Validation: `cargo fmt --all -- --check`, `cargo clippy -p unastar -p unastar-data -p bds-sweep --all-targets -- -D warnings`, `cargo test -p unastar --lib --tests`, `cargo run -p parity -- check`, `openspec validate --all --strict`, `bun tools/agents/check-links.ts`
