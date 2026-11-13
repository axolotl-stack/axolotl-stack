# Tasks: the bat's flight and resting

## 1. Measure

- [x] 1.1 `sweep.py bat` sets `cave`, `wake`, `low`, `near`, `unroof`, `canopy`, and `sweep.py ghast`
- [x] 1.2 `sweep.py vex` and `tadpole --items grow` (measured for later changes)
- [x] 1.3 `mob_fixture.py` reads `AddActor` flags from anywhere in the capture; the committed bat fixture

## 2. Implement

- [x] 2.1 `FloatNavigation`, `FloatWanderGoal` and the `CANFLY` flag
- [x] 2.2 The float control (`mob::ai::fly`) and the float branch of the AI tick
- [x] 2.3 Float-navigating physics in `mob::derive`; the bat's vertical factor and fall damage multiplier
- [x] 2.4 Resting (`mob::ai::roost`): born resting, settling, waking by roof, player and hurt

## 3. Verify and record

- [x] 3.1 Replays (`mob/tests/bat.rs`)
- [x] 3.2 Parity rows (`cargo run -p parity -- generate`, `-- check`), quirks, decision 0069, READMEs, a solution note
- [x] 3.3 `cargo fmt --all -- --check`, `cargo clippy -p unastar --all-targets -- -D warnings`, `cargo test -p unastar --lib --tests`, `openspec validate --all --strict`, `bun tools/agents/check-links.ts`, `python -m unittest discover -s tools/bds-sweep -p "test_*.py"`
