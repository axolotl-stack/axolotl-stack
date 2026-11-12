# Proposal: persist the player's air supply

## Why

BDS 1.26.51 keeps a player's air supply when it leaves and rejoins: the join `SetActorData`
still says 300, and the first update after it carries the saved value (39 → 43 on land, -1 → -2
under water; player-data capture phases `rejoin_on_land` and `rejoin_under_water`). unastar
restored every other part of the player but started it at full air again. The
`world-storage` spec lists the restored fields and the validation failures of a player record,
so both change.

Prior work: `docs/solutions/server/player-actor-data-updates-follow-bds-sync-points.md` and the
quirk rows "Entities / player air" in `docs/quirks/bds.md` (written with the implementation);
nothing earlier covers the air supply on disk.

## What Changes

- `PlayerData` gains `air` (ticks, default 300 when missing), saved from the player's breath and
  restored into it at spawn; a player saved while dead is stored with full air.
- `PlayerData::validate` rejects air outside -19..=300 (-20 never stays: the drowning hit resets
  it to 0).

Out of scope: the other actor data (flags, pose) is not persisted; BDS rebuilds it on join.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `world-storage`: the restored player fields include the air supply; a record with an air supply
  outside -19..=300 is invalid (a scenario of the invalid-record requirement).

## Impact

- Crates: `unastar` (`storage/provider.rs`, `server/game/mod.rs`, `entity/player_life/breath.rs`).

### Parity rows

| Domain | Row | Target |
|---|---|---|
| systems | `player.damage_sources` | stays `partial`; "the air saved across a rejoin" moved from the gaps to what matches BDS (done with the implementation in commit "fix(unastar): replicate a player's actor data like BDS") |

No claim: the work landed before this change was written up, and the change is archived with it.
