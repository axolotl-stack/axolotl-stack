# Proposal: item cooldowns and spear attacks

## Why

Spears are 42 open item rows. Their `minecraft:cooldown`, `minecraft:enchantable`,
`minecraft:piercing_weapon` and `minecraft:kinetic_weapon` facets had no server behaviour. The
server dealt every spear hit at once, whatever the item's attack cooldown. It ignored the jab (item
use `UseAsAttack`) and holding a spear, and played no sound for a missed swing.

Round 1 of the items lane measured the spears' plain hits. It saw `PlayerStartItemCooldown` after
them but did not measure whether BDS refuses hits during the cooldown
(`docs/quirks/bds.md`, Items / spears).

## Prior work

- [decision 0013](../../../docs/decisions/0013-copy-bds-behaviour-exactly.md) (copy BDS exactly)
  and [0014](../../../docs/decisions/0014-measure-built-in-facts-from-bds.md) (measure built-in
  facts from BDS).
- `docs/quirks/bds.md`: Items / spears (the round 1 attack sweep).
- `openspec/specs/server-authority`: "Item use acts on the server's selected stack and clock". Its
  cooldown counterpart is new.
- Nothing in `docs/solutions` covered item cooldowns or the jab.

## What Changes

- **Measurement.** The scripted client gets item-use orders: a hit by name tag, the jab on its own
  or packed into the input, missed swings, use and release, hotbar selection, gliding, sprinting
  and moving. It also gets a `--decode` mode for recordings. `scripts/spears.txt` and
  `scripts/enchant-spears.txt` play against plain BDS, and `combat_fixture.py` cuts the
  committed fixtures.
- **Item cooldowns.** They come from `minecraft:cooldown`:
  - an attack cooldown starts on a hit on a living mob and on a jab that hits nothing;
  - `PlayerStartItemCooldown` gives the category and ticks;
  - while it runs, every hit and jab with an item of that category is refused: no damage, no
    sound, no new cooldown.
- **The jab.** `UseAsAttack`, also packed into `PlayerAuthInput`, plays the item's `attack_miss`
  sound and starts its cooldown; a sword's jab does nothing.
- **Missed swings.** The `MissedSwing` input flag plays the item's miss sound or
  `attack.nodamage`.
- **Holding.** Items with `minecraft:use_modifiers` that are not food (the spears) set the using
  flag while held.
- **Killing hits** send only the death event, as BDS does.
- **Tests.** Two replays of the BDS captures: `spear_tests.rs` (hits, cooldown, jabs, swings,
  holding, reach) and `spear_enchant_tests.rs` (`/enchant` on every spear).

Out of scope:
- a jab that pierces several entities: BDS damaged no second entity in any form the scripted
  client sent, so what the vanilla client sends for one is not known;
- the charge (kinetic damage, knockback and dismount by relative speed);
- spear repair (anvil);
- use cooldowns of other items (wind charge, ender pearl, chorus fruit, goat horn).

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `server-authority`: one added requirement, "Item cooldowns are kept by the server".

## Impact

- Crates:
  - `unastar`: `item/cooldown.rs`, `entity/mob/combat/hit.rs`, `server/game/item_use`,
    `server/game/movement.rs` and `server/game/packets.rs`;
  - `bds-sweep`: the scripted client.
- Tools: `tools/bds-sweep` (`scripts/spears.txt`, `scripts/enchant-spears.txt`,
  `combat_fixture.py`).

### Parity rows

| Domain | Row | Target |
|---|---|---|
| items | `minecraft:{wooden,stone,copper,iron,golden,diamond,netherite}_spear/cooldown` | verified (spear capture) |
| items | `.../enchantable` | verified (`/enchant` capture) |
| items | `.../piercing_weapon` | partial: the jab that hits nothing is copied; piercing is not |
| items | `.../kinetic_weapon` | partial: holding is copied; the charge is not |
| items | `.../durability` | stays partial; gaps updated |
| protocol | `PlayerStartItemCooldownPacket` | partial: sent for attack cooldowns |

The rows are covered by the lane ITEMS claim on the items families; this change adds no claim.
