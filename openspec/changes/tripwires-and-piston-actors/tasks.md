# Tasks

- [x] 1.1 Tripwire, riding and infested families (`tools/bds-sweep/plates.py`, `pistons.py`, `infested.py`); `mine` and `stop` timeline events in the sweep plugin
- [x] 2.1 `redstone/tripwire.rs`: lines, attaching on placement, links to hooks, hook power
- [x] 2.2 `redstone/plates.rs`: the `Tripwire` kind; `press_plates` writes string and hooks as changes
- [x] 2.3 `redstone/piston_actors.rs`: pushes and carries per move tick; `push_actors` after the circuit step
- [x] 3.1 Replays: every tripwire row and every piston row with actors, from the measured actor positions
- [x] 3.2 Parity rows, quirk rows, decision 0063, `cargo run -p parity -- generate` and `-- check`
- [x] 3.3 Validation: `cargo fmt --all -- --check`, `cargo clippy -p unastar --all-targets -- -D warnings`, `cargo test -p unastar --lib --tests`, `cargo run -p parity -- check`, `openspec validate --all --strict`, `bun tools/agents/check-links.ts`, the bds-sweep unit tests
