# Tasks

## 1. Claim

- [x] 1.1 No `[[claim]]`: proposed, applied and archived in one commit; no claim covers the rows

## 2. Measure

- [x] 2.1 `protocol_probe.py pick` and `pick-actors` on a locked slot; `!pick-block` `~` coordinates
- [x] 2.2 `pick_table.py` and its unit test; the measured table and its README row
- [x] 2.3 Commit the pick fixtures

## 3. Implement

- [x] 3.1 `unastar_data::actor_picks` with typed errors
- [x] 3.2 `inventory::actor_pick` (`ActorPickItems`, `picked_item`), `InventoryAction::ActorPick`,
  `select_item` names the pick
- [x] 3.3 Tests against the fixtures

## 4. Record

- [x] 4.1 Parity rows `ActorPickRequestPacket`, `GuiDataPickItemPacket`; `parity generate`/`check`
- [x] 4.2 Quirk rows (actor picks, picks with data)

## 5. Validate

- [x] 5.1 `cargo fmt --all -- --check`
- [x] 5.2 `cargo clippy -p unastar -p jolyne -p bds-extractor --all-targets -- -D warnings`
- [x] 5.3 `cargo test -p unastar --lib --tests`, `cargo test -p unastar-data`
- [x] 5.4 `python -m unittest discover -s tools/bds-sweep -p "test_*.py"`
- [x] 5.5 `openspec validate --all --strict`, `bun tools/agents/check-links.ts`
