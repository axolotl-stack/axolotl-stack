# Tasks

## 1. Claim and measure

- [x] 1.1 Check the claims. The spear rows sit in the items families the lane ITEMS claim already covers, so this change adds no claim. Verify with `cargo run -p parity -- check`.
- [x] 1.2 Give `bds-sweep-client` the item-use orders:
  - `attack-named`, `jab`, `jab-input`;
  - `swing`, `animate`, `missed-swing`;
  - `use`, `use-on`, `release-item`, `consume`;
  - `select`, `glide`, `unglide`, `sprint`, `unsprint`, `look`, `move`.

  Inputs also carry the camera orientation, and `--decode` prints a recording. Verify with `cargo test -p bds-sweep`.
- [x] 1.3 Play `tools/bds-sweep/scripts/spears.txt` and `scripts/enchant-spears.txt` against BDS 1.26.51 (`script.py`). Cut `crates/unastar/src/entity/mob/combat/hit/testdata/bds-spears.jsonl` and `crates/unastar/src/item/testdata/bds-enchant-spears.jsonl` with `tools/bds-sweep/combat_fixture.py`.

## 2. Cooldowns and spear attacks

- [x] 2.1 Add `item::cooldown`: the cooldown of an item from `minecraft:cooldown`, the player's running cooldowns and `PlayerStartItemCooldown`. Verify with `spear_cooldowns_come_from_the_item_documents`.
- [x] 2.2 Refuse spear hits while the attack cooldown runs, and start it on a hit on a living mob (`player_attack`). A killing hit sends only the death event.
- [x] 2.3 Route the jab (`UseAsAttack`, also packed into `PlayerAuthInput`) to item use: the miss sound and the cooldown. Route the `MissedSwing` flag to the miss sound. Hold items with `use_modifiers` that are not food.
- [x] 2.4 Replay the spear capture (`spear_tests.rs::spear_hits_jabs_and_swings_answer_like_bds`) and the `/enchant` capture (`spear_enchant_tests.rs::spears_take_the_enchantments_bds_puts_on_them`).
- [x] 2.5 Record the copied behaviour in `docs/quirks/bds.md` (Items / spears, Items / swings, Entities / melee). Describe it in `crates/unastar/README.md`, `tools/bds-sweep/README.md` and `docs/guides/tools.md`.

## 3. Parity and validation

- [x] 3.1 Update the spear rows in `docs/parity/items.toml` (cooldown and enchantable verified, piercing_weapon and kinetic_weapon partial, durability gaps) and `PlayerStartItemCooldownPacket` in `docs/parity/protocol.toml`. Run `cargo run -p parity -- generate` and `cargo run -p parity -- check`.
- [x] 3.2 Run the validation:
  - `cargo fmt --all -- --check`;
  - `cargo clippy -p unastar -p bds-sweep --all-targets -- -D warnings`;
  - `cargo test -p unastar --lib --tests` and `cargo test -p bds-sweep`;
  - `python -m unittest discover -s tools/bds-sweep -p "test_*.py"`;
  - `openspec validate --all --strict`.
