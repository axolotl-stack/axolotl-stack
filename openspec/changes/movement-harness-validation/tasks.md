# Tasks

## 1. Harness validation

- [x] 1.1 Setup, input and relay checks in `movement-diff extract` (`bds_sweep::validation`)
- [x] 1.2 Known-answer scenarios and `bds_reads_the_known_answers_bit_for_bit`
- [x] 1.3 `movement-diff check`, the checks file and `other_bds_runs_give_the_committed_traces`
- [x] 1.4 Runner world dumps (`movement-diff world`, `--world-out`) and `compare.py --worlds`
- [x] 1.5 `mutate.py` and its results in the report
- [x] 1.6 `movement-diff shift` / `velocities` and the dense-velocity fixture
- [x] 1.7 `movement-diff random` and a batch on BDS
- [x] 1.8 CI: `cargo test -p bds-sweep`, the comparator tests, `go vet` and `go test`

## 2. Simulation and trust

- [x] 2.1 Fixes in `unastar_rules::movement`, each against the BDS traces (replay test)
- [x] 2.2 `unastar_data::motion`; the registry re-exports the liquid types
- [x] 2.3 Effect modifiers on the movement attribute (`attributes.rs` test)
- [x] 2.4 Trust removed for slowness, levitation, lava sources, scaffolding, berries and the
  climbing plants (`player_life/tests/authority.rs`)

## 3. Records

- [x] 3.1 Quirk rows, `player.movement` and `UpdateAttributesPacket` parity rows;
  `cargo run -p parity -- generate` and `-- check`
- [x] 3.2 Decision 0060, solution note, README and report
- [x] 3.3 Validation: `cargo fmt --all -- --check`; `cargo clippy -p unastar -p unastar-rules
  -p bds-sweep --all-targets -- -D warnings`; `cargo test -p unastar-rules -p bds-sweep`;
  `cargo test -p unastar --lib --tests`; `openspec validate --all --strict`;
  `bun tools/agents/check-links.ts`
