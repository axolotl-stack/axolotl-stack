# Tasks

## 1. Claim and measure

- [x] 1.1 Claim `player.combat` in `docs/parity/claims.toml` with `change = "add-player-versus-player-combat"`
- [x] 1.2 Lockstep clients (`bds-sweep-client --also`, `crates/bds-sweep/src/lockstep.rs`), orders `resync`, `slot`, `claim`, `claim-slot`, `swing`
- [x] 1.3 Endstone `pvp` job (packet, damage and knockback recording on one sequence; yaw marks), `pvp_items.py`, `sweep.py pvp`
- [x] 1.4 Run `numbers`, `reach`, `hurt` (3 repeats), strict `reach` (2), `extra` (3) on BDS 1.26.51; distil with `pvp_fixture.py` into `crates/unastar/src/entity/player_life/testdata/bds-1.26.51-pvp.json`

## 2. Implement

- [x] 2.1 `player_life::pvp`: checks, damage, crits, the second push, `Swing` / `InputStart`, hit sounds
- [x] 2.2 `hurt.rs`: armour, Protection and Resistance before the window; excess hit pushes, shows the hurt and restarts the window; arrival tick; velocity kept; knockback resistance; motion to viewers; `SetLastHurtBy` and particles
- [x] 2.3 Route attacks on players from the mob-action queue; `pvp` game rule; Sharpness for mob hits too; Protection, Sharpness, Knockback enchantment ids; netherite knockback resistance
- [x] 2.4 Tests: `crates/unastar/src/entity/player_life/tests/pvp.rs` (replay of every trial, packet-path tests, ignored ordering tests)

## 3. Records

- [x] 3.1 Parity rows `player.combat`, `crafting.enchantments`, `player.status_effects`, `player.damage_sources`; `cargo run -p parity -- generate` and `-- check`
- [x] 3.2 Quirk rows "PvP / *" and the corrected "Survival: hurt cooldown"; solution notes; decision 0046 (proposed); READMEs

## 4. Validate

- [x] 4.1 `cargo fmt --all -- --check`
- [x] 4.2 `cargo clippy -p unastar -p bds-sweep -p unastar-data --all-targets -- -D warnings`
- [x] 4.3 `cargo test -p unastar --lib --tests`, `cargo test -p parity`
- [x] 4.4 `python -m unittest discover -s tools/bds-sweep -p "test_*.py"`
- [x] 4.5 `openspec validate --all --strict`; `bun tools/agents/check-links.ts`; `review-change`
