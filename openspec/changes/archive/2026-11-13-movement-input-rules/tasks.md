# Tasks

## 1. Rules

- [x] 1.1 Sneak release, sprint threshold and timing, double tap, walls, the air acceleration,
  honey's side, touching blocks and the collision tolerance in `unastar_rules::movement`, each
  fitted to a seed and confirmed by probes on BDS
- [x] 1.2 Two random batches played on BDS with the final simulation; the failing seeds added
  to the corpus are listed as known gaps

## 2. Harness

- [x] 2.1 The knockback check in `movement-diff extract`
- [x] 2.2 `bds_sweep::json` and the fixtures rewritten
- [x] 2.3 Corpus: probes, seeds, mutation catchers, bed drops; the committed run, the repeat,
  offset and keys-only runs, the shifted velocities
- [x] 2.4 `mutate.py`: new rules' mutations, observable sizes; `mutations.json`

## 3. Records

- [x] 3.1 Quirk rows and the `player.movement` parity row; `cargo run -p parity -- generate`
  and `-- check`
- [x] 3.2 Decision 0067, solution note, README and report
- [x] 3.3 Validation: `cargo fmt --all -- --check`; `cargo clippy -p unastar -p unastar-rules
  -p bds-sweep --all-targets -- -D warnings`; `cargo test -p unastar-rules -p bds-sweep`;
  `cargo test -p unastar --lib --tests`; `openspec validate --all --strict`;
  `bun tools/agents/check-links.ts`; the Python suites
