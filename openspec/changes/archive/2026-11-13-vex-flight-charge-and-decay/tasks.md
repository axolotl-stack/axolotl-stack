# Tasks: the vex's flight, charge and decay

## 1. Measure

- [x] 1.1 A full blocker dump of the tadpole, vex, goat, frog and evoker
- [x] 1.2 The tadpole's bobbing fitted from the round 8 swims (the pitch left open)
- [x] 1.3 `sweep.py vex` (`idle`, `charge`, `look`, `decay` kept in view) and `sweep.py goat`
- [x] 1.4 `mob_fixture.py --events-only`; the committed vex fixture

## 2. Implement

- [x] 2.1 `PhysicsBody::collides`; the vex class's drag
- [x] 2.2 The vex's move control and native wander (`mob::ai::vex`, `FlyState::steer`)
- [x] 2.3 `ChargeAttackGoal`, `tick_charge_hits` and melee hits without a melee goal
- [x] 2.4 `DamageOverTime` and `LookAtEntityGoal`

## 3. Verify and record

- [x] 3.1 Replays (`mob/tests/vex.rs`)
- [x] 3.2 Parity rows, quirks, decision 0070, the research note, READMEs, a solution note
- [x] 3.3 `cargo fmt --all -- --check`, `cargo clippy -p unastar --all-targets -- -D warnings`, `cargo test -p unastar --lib --tests`, `openspec validate --all --strict`, `bun tools/agents/check-links.ts`, `python -m unittest discover -s tools/bds-sweep -p "test_*.py"`
