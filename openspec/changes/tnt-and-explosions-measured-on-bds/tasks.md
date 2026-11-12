# Tasks

- [x] 1.1 TNT families (`tnt_prime`, `tnt_actors`, `tnt_hits`, `tnt_blast_*`) and the `tnt` placements case; `summarize.py round4` to `data/measured/explosions/`
- [x] 2.1 `server/game/explosion.rs`: rays, resistance, exposure, impact and damage; fitted centre
- [x] 2.2 `minecraft:explode` body and component, `tick_fuses`; power primes TNT in the circuit update
- [x] 2.3 Live explosion: actors hurt through the mob and player hurt paths, blocks broken with their drops, TNT chained, packets as captured
- [x] 3.1 Tests: blast distribution, drops, damage, the fuse in a running server, the primed actor's motion and chain fuses; the redstone replays now include TNT
- [x] 3.2 Parity rows, quirk rows, solution note, `cargo run -p parity -- generate` and `-- check`
- [x] 3.3 Validation: `cargo fmt --all -- --check`, `cargo clippy -p unastar -p unastar-data -p bds-sweep --all-targets -- -D warnings`, `cargo test -p unastar --lib --tests`, `cargo test -p unastar-data -p bds-sweep -p parity`, `cargo run -p parity -- check`, `openspec validate --all --strict`, `bun tools/agents/check-links.ts`
