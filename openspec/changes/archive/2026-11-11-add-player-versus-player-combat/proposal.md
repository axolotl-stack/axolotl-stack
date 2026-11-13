# Proposal: player-versus-player melee like BDS

## Why

The owner wants PvP to behave exactly like BDS 1.26.51, including the races between packets.
`player.combat` said "no player-versus-player damage, melee critical hits, shields": an attack on
another player was dropped, because the attack queue only resolved mob runtime IDs.

Prior work: the mob hit and hurt-window code (`entity/mob/combat/hit.rs`), the player hurt path
(`entity/player_life/hurt.rs`), the knockback notes
`docs/solutions/server/knocked-back-mobs-veered-from-air-control-and-friction-timing.md`, the
quirk rows "Survival: hurt cooldown" (an unmeasured claim that an excess hit is silent) and
"Entities / killing hit", and decision 0036 (command order). The packet-order design work
(decision 0045, another lane) owns the general arrival-order schedule; this change measures the
combat numbers and implements one hit function the ordered path can call.

## What Changes

- Tooling: `bds-sweep-client --also NAME` runs several clients in one process in lockstep: the
  sweep plugin sends a `bds-sweep:tick` line every server tick naming each client's inputs,
  attacks, orders and batches in order; every line turns each client by 0.01 degrees, so the
  tick BDS consumed each input in is measured. New orders `resync`, `slot`, `claim`,
  `claim-slot`, `swing`. A `pvp` sweep kind (three clients, Endstone recording of every packet,
  damage and knockback event in one sequence) and `pvp_fixture.py`.
- unastar: `player_life::pvp::player_hits_player` handles one hit with the world as it is when
  BDS applies it: the checks (dead attacker, victim gone or immune, the `pvp` game rule, the
  claimed position, reach), damage (weapon, Sharpness, Strength and Weakness, critical hits) and
  the extra push of a sprint or Knockback. The hurt path gains Protection, Resistance, the
  knockback resistance of netherite, the velocity a push keeps, and a knock and hurt event for a
  hit that exceeds the hurt window.
- Attacks on a player are routed from the mob-action queue to the hit function, with the
  attacker's state before its first input of the tick (BDS applies an attack when it consumes
  the attacker's next input, before that input's move).

Out of scope: shields, thrown tridents, the mace's smash attack, the strict entity interaction
mode (its facing check), the order BDS applies different players' packets in within a tick
(decision 0045's schedule), totems.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `server-authority`: attacks on players are checked against server state (alive, not spectator,
  immune victim, the `pvp` rule, held slot and item, claimed position, reach), and a reported
  velocity must be finite. Which vanilla rules the checks reproduce stays in the parity rows.

## Impact

- Crates: `unastar` (`entity/player_life/{pvp,hurt}.rs`, `entity/mob/interaction/actions.rs`,
  `server/game/movement.rs`, `world/rules.rs`, `item/enchant.rs`), `unastar-data`
  (`armour.rs`), `bds-sweep` (lockstep), `tools/bds-sweep` (`pvp` job, items, fixture).

### Parity rows

| Domain | Row | Target |
|---|---|---|
| systems | `player.combat` | stays `partial` (shields, tridents, the mace's smash missing); PvP melee `verified` in the gaps text against `bds-1.26.51-pvp.json` |
| systems | `crafting.enchantments` | stays `partial`; Sharpness, Knockback and Protection act |
| systems | `player.status_effects` | stays `partial`; Resistance acts on melee |
| systems | `player.damage_sources` | stays `partial`; the excess hit's knockback and hurt event |
