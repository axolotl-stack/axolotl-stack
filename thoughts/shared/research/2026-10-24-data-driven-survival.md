---
date: 2026-10-24
author: VANILLA-SURVIVAL worker
target: BDS 1.26.51 (protocol 2193)
status: research; parked WIP implementation on branch `stab-vanilla-survival` (commits in the appendix)
scope: player health and attributes, hunger and food, damage, death and respawn, commands, game rules and difficulty, time and weather, player list and skins
---

# Data-driven survival: how BDS does it and how unastar should

## How to read this

**Sources.** Statements cite the public or measured source they rest on:

| Tag | Source |
|---|---|
| **(BP)** | bedrock-samples behaviour pack: `crates/unastar-data/data/vanilla_bp/behavior_pack`. Counts here were taken on the old pin 1.21.130.3; D1 has since bumped main to 1.26.50.4 |
| **(RP)** | The same bedrock-samples resource pack, language files `resource_pack/texts/*.lang` |
| **(N)** | Native BDS 1.26.51 measurement tables, CC0: `crates/valentine_gen/protocolgen/data/source/inputs/1.26.51/native/` |
| **(PD)** | Mojang's public protocol docs: `crates/valentine_gen/bedrock-protocol-docs/json` |
| **(V)** | Generated protocol types: `crates/valentine/bedrock_versions/v1_26_51/src/{proto,types}.rs` |
| **[join]** | BDS 1.26.51 join capture (packet names only): `crates/jolyne/src/proto/testdata/bds-1.26.51-join-s2c.txt` |
| **[gophertunnel]** | The pinned gophertunnel fork (`tools/client-diff`), protocol constants and field semantics |
| **[dragonfly]** | df-mc/dragonfly (MIT) |
| **[wiki]** | Minecraft Wiki (behaviour descriptions only) |

Statements with no tag describe vanilla behaviour without a public source. Per the
evidence rule they must be confirmed by a capture or a BDS measurement before they are
treated as final; §5 lists the fixtures that do it.

**Data kinds** (same meaning as the items research):

| Tag | Meaning |
|---|---|
| **(a)** | Data-driven from pack JSON or language files |
| **(b)** | Native/built in, but measurable as a table (D4) |
| **(c)** | Hard-coded behaviour that unastar writes in Rust |

## TL;DR

1. **Most survival numbers are already data (a).** `player.json` (BP) carries every
   per-player constant the hunger, air and lava systems need: `minecraft:exhaustion_values`
   (heal 6, jump 0.05, sprint_jump 0.2, mine 0.005, attack 0.1, damage 0.1, walk 0,
   sprint 0.1, swim 0.01, lunge 4), `minecraft:breathable` (15 s supply, 3.75 s inhale,
   suffocate −1 s), `minecraft:health` 20, `minecraft:attack` 1,
   `minecraft:hurt_on_condition` (lava 4/tick), `minecraft:player.saturation` 5/20,
   `minecraft:player.exhaustion` 0/20, `minecraft:experience_reward` (Molang). The WIP
   branch already reads `exhaustion_values` through `unastar-data` (generated entity
   bundles now implement `Default` with the pack values).
2. **Food is an item component (a)**: 40 vanilla foods carry `minecraft:food` in (BP),
   legacy and new formats mixed (named saturation strings, `effects`,
   `using_converts_to`, `can_always_eat`). The WIP branch has a food family, but its
   codegen emits a per-entry static table; under D2 it becomes schema-only codegen plus
   embedded data decoded at startup, together with items and blocks.
3. **The rules are code (c), the parameters are data.** Exhaustion → saturation → food,
   natural regeneration every 80 ticks at ≥ 18 food, the per-difficulty starvation floor,
   the 10-tick hurt cooldown, fall damage `ceil(d − 3 − jump boost)`, armour reduction,
   effect intervals: all are fixed rules parameterised by the data above.
4. **D4 facts to measure, not port**: melee damage per tool tier, armour points/toughness
   per material and slot, knockback force/height (and sprint bonus), fall-damage
   multipliers per landing block, drowning damage and interval, void depth and damage,
   game-rule defaults and order. The WIP branch uses dragonfly (MIT) values for the
   first three and wiki values for the block multipliers; they stay only as a
   cross-check for the BDS sweep.
5. **Commands should be data from BDS (b).** BDS sends its whole command syntax in
   `AvailableCommandsPacket` [join]. Capturing it once gives every command name, alias
   enum, overload, parameter type and enum value list; only execution is code (c).
   Feedback is translation keys from (RP) (`commands.*`, `death.attack.*`).
6. **The join sequence shows four sync gaps** in the WIP branch [join]: BDS sends
   `SyncWorldClocksPacket` (not `SetTime`), `SetCommandsEnabledPacket`, three
   `RespawnPacket`s during join and a late `SetHealthPacket`; the WIP sends `SetTime` and
   none of the others. Payloads are not in the fixture yet (§5).

---

## 1. State of the parked WIP branch

Branch `stab-vanilla-survival` (merged with `main` 9dbfbb6). Everything below compiles,
is wired into the tick, and has ECS-level tests unless marked otherwise.

| Area | WIP implementation | Tests |
|---|---|---|
| Damage pipeline | `entity::DamageRequest` message → `apply_damage_requests` for any entity with `Health`: game-mode protection (void and `/kill` bypass), `fallDamage`/`drowningDamage`/`fireDamage` rules, fire resistance, 10-tick hurt cooldown (excess only), armour, resistance, `ActorEvent::Hurt` with the cause, knockback (`SetActorMotion` to players, `Velocity` for mobs), crit animation, `Dead` marker + `EntityDied` | `entity::systems::damage::tests::*` (6), `server::game::tests::damage_is_synced_as_a_health_only_attribute_update` |
| Attributes | Join sends the full list; `sync_player_attributes` sends only changed attributes once per tick; air supply as actor data | `attributes::tests::diff_sends_only_changed_attributes_once` |
| Hunger | Exhaustion step, peaceful refill, regeneration, starvation per difficulty; values from `player.json` | `entity::systems::hunger::tests::*` (7) |
| Movement survival | `on_ground` from `VerticalCollision` (fixes the inverted flag), fall distance and fall damage with landing multipliers, sprint/swim/jump exhaustion | `server::game::movement::tests::*` (5) |
| Environment | Void, drowning/air, lava, burning, regeneration/poison/wither via `DamageRequest`, effect expiry sends `MobEffect` remove | `entity::systems::effects::tests::*` (4) |
| Melee | `ItemUseOnActor` Attack: held-item damage, strength/weakness, crits, `pvp` rule, reach, attack exhaustion | `combat::tests::*` (4), `server::game::tests::melee_hit_between_players_damages_and_knocks_back` |
| Level rules | `LevelRules {difficulty, game_rules, weather}`, StartGame difficulty and rule list, `SetDifficulty`/`GameRulesChanged`/weather `LevelEvent` broadcast, natural weather cycle, `server.difficulty` config | `level_rules::tests::*` (5) |
| Clock and spawn | `WorldClock` (daylight cycle, `SetTime` on change and every 200 ticks), `WorldSpawn`/`SpawnPoint` with `SetSpawnPosition` | `world_clock::tests::*` (2) |
| Commands | Framework with world access, permission levels, overload syntax, `AvailableCommandsPacket` at join, `CommandOutputPacket` with (RP) keys; `/gamemode /time /weather /give /tp /kill /difficulty /gamerule /say /list /effect /setworldspawn /spawnpoint`; selectors `@s @a @p @r @e`, names, `~` | `command::*::tests` (10), `server::game::commands::tests` (5) |
| Player list | `PlayerList` add (with skins) before `AddPlayer`, removal + `RemoveActor` on quit, `AddPlayer` with held item, actor data, platform, armour for existing players | `server::game::tests::joining_players_see_the_player_list_before_add_player` |
| Skins | `ClientData` → `SerializedSkinRef` (gophertunnel semantics; invalid image sizes disconnect with `Skinissue`) | `skin::tests::*` (5) |
| Relays | Swing (`Animate`) + plugin `on-player-swing`, client sounds, emotes (server-side flag), `PlayerSkin` | covered by the player-list test only |
| Food data | `unastar_data::items::food()` from (BP), `output/foods.kdl` with provenance | `unastar-data/tests/foods.rs` (5) |
| Plugin API | `get_game_mode` reads the component (was hard-coded) | `plugin::manager::tests::get_game_mode_reads_player_component` |
| Housekeeping | `packet_domains.rs` deleted; all `Messages` buffers registered and rotated each tick (they grew without bound before) | — |

**Not finished (parked):**
- Death and respawn: `server/game/death.rs` is written (death message, `DeathInfo`,
  `ActorEvent::Death`, drops unless `keepInventory`, `Respawn` ReadyToSpawn, respawn
  reset, re-add for viewers) but **not declared as a module**: it calls VANILLA-ENTITIES'
  `entity::item_drop::spawn_item_drop`, which is not on `main` yet. `RespawnPacket`
  (ClientReadyToSpawn) and `PlayerAction::Respawn` are already routed into
  `RespawnPacketQueue`.
- `/give` reports items that do not fit instead of dropping them (same dependency).
- Eating: the food data and `Hunger::eat` exist; the consume flow (use start/finish,
  duration check, item decrement, `using_converts_to`, food effects) belongs to the
  VANILLA-ITEMS consume path and is not wired.
- Mining exhaustion (`exhaustion_values.mine` per survival block break) needs one hook in
  the block-break commit (VANILLA-ENTITIES file).
- `MobEquipment`/`MobArmorEquipment` rebroadcast on change (VANILLA-ITEMS).

---

## 2. How BDS does it

### 2.1 Player attributes and health sync

- Players have server-side attributes: health, absorption, movement, hunger, saturation,
  exhaustion, level, experience. Ranges and defaults come from `player.json` (a)
  (`minecraft:health` 20; `player.saturation` value 5, max 20; `player.exhaustion` 0/20;
  `player.level` max 24791; `player.experience` 0/1).
- The client is told about changes with `UpdateAttributesPacket` carrying only the
  attributes that changed. The join sequence sends `UpdateAttributesPacket` twice, the
  second after the first chunks [join].
- BDS also sends a `SetHealthPacket` once at the end of join [join]. The WIP never sends
  it; its payload and whether BDS sends it again on later health changes need a capture
  (§5, fixture F1).
- Open: the WIP sends exhaustion with max 5 at join (inherited) while the pack says max
  20; the wire min/max/default triplets per attribute must come from F1.

### 2.2 Hunger and exhaustion (rules (c), parameters (a))

Parameters: `minecraft:exhaustion_values` (a). Rules:

- Creative and spectator players are skipped.
- When exhaustion reaches 4, 4 is removed and one saturation point is drained, or one
  food point if saturation is empty — **never food on peaceful**. At most one such step
  happens per tick; extra exhaustion waits for the next tick.
- Peaceful refills food by 1 every 10 ticks.
- With `naturalRegeneration` on, food ≥ 18 and health below max, the player heals 1
  every 80 ticks and gains `exhaustion_values.heal` exhaustion. There is no
  saturation-based fast regeneration (unlike Java Edition).
- At food 0 the player takes 1 starvation damage every 80 ticks while health is above
  10 on easy, above 1 on normal, and always on hard [wiki].
- When neither applies, the 80-tick timer resets every tick, so partial progress is lost.
- Exhaustion sources: per horizontal metre `sprint`/`swim`/`walk`; per jump
  `jump`/`sprint_jump`; per survival block break `mine`; per attack `attack`; per hit
  taken `damage` (only for damage that armour can reduce).

### 2.3 Food (a)

- 40 vanilla items carry `minecraft:food` (BP). The schema mixes the legacy format
  (named saturation `poor`…`supernatural`, `effects`, `remove_effects`,
  `using_converts_to`, `cooldown_*`, `on_use_*`) and the new one (numeric
  `saturation_modifier`, `can_always_eat`). Seven legacy files use old identifiers
  (`appleEnchanted`, `fish`, `muttonRaw`, …) that resolve through the native
  `descriptionId` (N).
- Use duration is the native `maxUseDuration` (N), matching `minecraft:use_modifiers`/
  `minecraft:use_duration` for every vanilla food.
- Eating adds `nutrition` and `nutrition × saturation_modifier × 2` saturation, capped at
  the food level (`minecraft:food` documentation in the BP schemas).
- A full player cannot start eating unless the food is `can_always_eat`, except on
  peaceful and in creative, where any food can be eaten. (Behaviour statement; confirm
  with fixture F4.)
- **D2 design**: the food family becomes a schema type generated at build time
  (`FoodData`, `FoodEffect`) plus the (BP)-derived rows embedded in the binary and
  decoded once at startup into a registry keyed by identifier, so addon packs (D5) can
  add or override rows before the registry is frozen. This replaces the WIP's generated
  `ALL_FOODS` table and should land together with the item and block families.

### 2.4 Damage

| Fact | Kind | Source / status |
|---|---|---|
| Damage cause ordinals (`EntityAttack` 2, `Fall` 5, `Void` 12, `Starve` 16, …) | (c) wire constants | (V), [gophertunnel] `DamageCause*` |
| `ActorEvent::Hurt` carries the cause as data; broadcast to all viewers including the victim | (c) | confirm payload with F2 |
| 10-tick hurt cooldown; a stronger hit inside it applies only the difference, with no hurt effect and no knockback | (c) | F2 |
| `Override` (and `SelfDestruct`) damage ignores the cooldown; `/kill` uses `Override` | (c) | F2 |
| Creative/spectator immune except void and `/kill` | (c) | F2 |
| Armour reduction `clamp(armor − dmg/(2 + toughness/4), armor/5, 20)/25` | (c) | [wiki] Armor |
| Armour points/toughness per material and slot, netherite knockback resistance | **(b) D4** | measure (F3); WIP uses [dragonfly] values |
| Resistance −20% per level | (c) | [wiki] |
| Fall damage `ceil((distance − 3 − jump boost level) × multiplier)` | (c) | [wiki] Fall damage |
| Landing multipliers (slime 0 unless sneaking, hay/honey 0.2, beds 0.5) and blocks that reset fall distance (water, ladders, vines, scaffolding, cobweb, powder snow) | **(b) D4** | measure (F5); WIP uses [wiki] values |
| Per-entity sensors: `minecraft:damage_sensor` (immunities, multipliers), `minecraft:hurt_on_condition` (player: lava 4/tick) | (a) | (BP) entity JSON; implement both generically for all entities |
| Melee damage per held item (tool tier + tool kind, trident, mace) | **(b) D4** | measure (F3); WIP uses [dragonfly] tiers |
| Strength +3/level, weakness −4/level, critical ×1.5 when falling, airborne, not flying/swimming/blind | (c) | [wiki] |
| Knockback force/height, sprint-attack bonus, knockback resistance | **(b) D4** | measure (F3); WIP uses [dragonfly] 0.45/0.3608 |
| Player knockback is pushed with `SetActorMotion` because player movement is client-predicted | (c) | F2 |

### 2.5 Environment

- Air (a): `minecraft:breathable` gives 300 ticks of air, refilled at 300/75 = 4 per
  tick out of water. Under water air drops 1 per tick; at −20 (`suffocate_time` −1 s) it
  resets to 0 and the player takes drowning damage (2 [dragonfly]; **measure**, F5).
- Air is shown through actor data (air supply key, `BREATHING` flag) — confirm with F5.
- Void: damage starts 64 blocks below the dimension floor, 4 every 10 ticks
  [dragonfly]; **measure** (F5).
- Lava: `hurt_on_condition` 4 per tick (a) and 15 s of burning; burning deals 1 every
  second; water extinguishes.

### 2.6 Death and respawn

- Death message: translation key per cause from (RP) `death.attack.*` /
  `death.fell.*` with the victim (and killer) names, broadcast when
  `showDeathMessages` is on. The victim also gets `DeathInfoPacket` (V).
- Other viewers see `ActorEvent::Death`.
- Unless `keepInventory`, inventory, armour, offhand and cursor drop as item entities;
  XP drops per `minecraft:experience_reward.on_death` = `Math.Min(query.player_level *
  7, 100)` (a, Molang; needs the Molang evaluator and XP orbs).
- Respawn: BDS uses `RespawnPacket` states (searching → ready → client ready). The join
  sequence itself already sends three `RespawnPacket`s [join]; the death-time sequence
  and payloads need F6. The client answers with `RespawnPacket` ClientReadyToSpawn and
  `PlayerAction::Respawn`.
- After respawn the player has full health, default food (20/5/0), no effects, full air,
  no fire, no fall distance, at its `/spawnpoint` or the world spawn.

### 2.7 Game rules and difficulty

- Difficulty defaults to easy (BDS `server.properties`), wire values 0–3 (V).
- Game rules: StartGame carries the full rule list and `GameRulesChangedPacket` follows
  in the join [join]. The WIP's default table (38 rules, lowercase names, values from
  vanilla defaults) is a **(b)** fact: replace it with the list captured from BDS (order,
  names, defaults, editable flags; F1).
- `doDaylightCycle`, `doWeatherCycle`, `keepInventory`, `naturalRegeneration`,
  `fallDamage`, `drowningDamage`, `fireDamage`, `pvp`, `showDeathMessages`,
  `doImmediateRespawn`, `sendCommandFeedback` gate the survival systems.

### 2.8 Commands

- BDS sends `AvailableCommandsPacket` at the end of the join [join], filtered by the
  player's command permission level. Its content — command names, descriptions
  (translation keys), aliases, overloads, parameter parse symbols, enum values and soft
  enums — is BDS data **(b)**: capture it once per permission level (F7) and replay it,
  instead of the WIP's hand-written overloads. Only the executors are code (c).
- Parse symbols and permission strings in 26.51 are the ones [gophertunnel] encodes
  (`CommandArgType*`, `"any"`/`"gamedirectors"`/…); the capture settles them.
- `SetCommandsEnabledPacket` is part of the join [join]; the WIP does not send it.
- Feedback: `CommandOutputPacket` (output type `alloutput`, success count, messages with
  (RP) `commands.*` keys and parameters). Announcements (`/say`) are translated
  `chat.type.announcement` text.
- Permission: operators run `GameDirectors` commands; `/list` and other `Any` commands are
  open. unastar grants operator through the server-owned `Operator` marker.

### 2.9 Time and weather

- A day is 24 000 ticks; `/time set` presets day 1000, noon 6000, sunset 12000, night
  13000, midnight 18000, sunrise 23000 [wiki].
- BDS 1.26.51 synchronises time with `SyncWorldClocksPacket` (sent twice in the join
  [join]); the WIP uses the older `SetTimePacket`. The clock payload and when BDS
  resends it need F8.
- Weather: `LevelEvent` start/stop rain (3001/3003) and thunder (3002/3004) (PD,
  [gophertunnel]); natural cycle durations from [wiki]; `doWeatherCycle` gates it.

### 2.10 Player list, skins and presence

- The join sends `PlayerListPacket` twice [join] (once before StartGame, once after the
  game rules); which entries each carries needs F1.
  Entries carry UUID, unique id, name, XUID, platform online id, build platform, skin,
  teacher/host/sub-client flags and a player colour (V).
- Skins come from the login client data; the conversion follows [gophertunnel]
  semantics. Unknown persona piece types map to the protocol's `Unsupported` value.
- `AddPlayer` carries the held item, game type, abilities and actor data (flags, name
  tag, air, bounding box, scale); the exact actor-data set BDS sends needs F2.
- The client predicts arm swings, attack/eat sounds and emotes; the server relays them
  to other viewers (emotes with the server-side flag).

---

## 3. Proposed design under D1–D5

1. **Player component values (a)**: keep reading `player.json` through the generated
   entity bundle `Default` (already on the branch). After the D2 rework, the entity
   family is decoded at startup like everything else; addon packs can override
   `minecraft:exhaustion_values`, `breathable`, `health` and `hurt_on_condition` (D5).
2. **Food (a)**: schema codegen + embedded rows, registry keyed by identifier, frozen
   after packs load (§2.3). The eat flow is a generic `minecraft:food` component
   behaviour owned by the items consume path; it calls `Hunger::eat` and applies
   `effects`/`remove_effects`/`using_converts_to`.
3. **Generic entity damage components (a)**: `damage_sensor` and `hurt_on_condition`
   implemented once for every entity (mobs reuse the same `DamageRequest` pipeline).
4. **Built-in combat tables (b, D4)**: tool-tier melee damage, armour points/toughness/
   knockback resistance, knockback force/height and sprint bonus, landing multipliers,
   drowning damage, void depth/damage — produced by the shared BDS extraction/sweep
   tooling (with VANILLA-ITEMS and VANILLA-ENTITIES) into `unastar-data`; dragonfly/wiki
   numbers only as a cross-check in tests.
5. **Commands (b)**: a captured `AvailableCommandsPacket` per permission level becomes a
   data artifact; the registry maps command names to Rust executors and appends plugin
   commands (D5). The WIP framework (`Command`, `CommandContext`, selectors, executors,
   `CommandOutputPacket`) stays; only the overload declarations are replaced by data.
6. **Game rules (b)**: captured rule list/defaults as data; `LevelRules` keeps the
   typed accessors.
7. **Language keys (a)**: death messages and command feedback reference (RP) keys; a
   test can assert every key the server emits exists in `en_US.lang`.
8. **Time (cross-area contract, architecture doc)**: WORLD owns `WorldClock` and its
   `SyncWorldClocksPacket` sync (world W10); the WIP `WorldClock` that sends `SetTime`
   is replaced by it when the branch lands. SURVIVAL keeps `LevelRules` (game rules,
   difficulty, weather) and the `/time` executor, which calls WORLD's
   `set_time`/`add_time`.
9. **Food data source (cross-area contract, architecture doc)**: gameplay rows come from
   the pack's `minecraft:food` components (current since D1), parsed in both legacy and
   new formats the way BDS parses them; `ItemRegistryPacket` sends protocolgen's
   `items.nbt` component data verbatim; a test checks both agree for every vanilla food.

## 4. Quirks we copy (logged in `thoughts/shared/research/bds-quirks.md`)

- Exhaustion converts at most one 4-point step per tick.
- Peaceful drains saturation but never food, and refills food every 10 ticks.
- The regeneration/starvation timer resets every tick it is not running.
- Starvation stops at 10 health on easy and 1 on normal, kills on hard.
- A stronger hit inside the hurt cooldown applies only the difference, silently.
- Creative players still die to the void and `/kill`.

## 5. Fixtures to capture or measure

All use the shared BDS runtime through `tools/client-diff` (one worker at a time).

| Id | What | How |
|---|---|---|
| F1 | Join payloads: `UpdateAttributes` (both), `SetHealth`, StartGame rule list, `GameRulesChanged`, `SetDifficulty`, `SetCommandsEnabled`, `SyncWorldClocks`, `Respawn` ×3 | extend the join fixture from names to decoded payloads |
| F2 | A hit on the scripted client: `ActorEvent` hurt payload, `SetActorMotion`, `UpdateAttributes`/`SetHealth` after damage, `AddPlayer`/`SetActorData` actor data | second scripted client attacks the first; BDS `/damage` for causes |
| F3 | Melee damage per item, armour reduction per material/slot, knockback per attack and sprint attack | sweep: `/give` + `/replaceitem` + scripted attacks, read health/motion |
| F4 | Eating: `ItemUse` start/finish packets, refusal when full, peaceful/creative exceptions | scripted eat with full and partial food |
| F5 | Fall damage per height and landing block, drowning interval/damage, void depth/damage, air actor data | `/tp` drops over each block; submerge; fall into the void |
| F6 | Death and respawn sequence: death message, `DeathInfo`, `Respawn` states, drops, post-respawn sync | `/kill` the scripted client, respawn |
| F7 | `AvailableCommandsPacket` for member and operator | join twice, once opped |
| F8 | Time: `SyncWorldClocks` payload and cadence, `/time set` response | observe idle, then `/time set` |

## 6. Open questions (answered by fixtures, not owner decisions)

- Does BDS send `SetHealthPacket` on every health change or only at join (F1/F2)?
- Exhaustion attribute max on the wire: 5 (WIP join) or 20 (`player.json`) (F1)?
- ~~Is `SetTimePacket` still sent at all next to `SyncWorldClocksPacket` (F8)?~~ Answered:
  0 `SetTime` against 54 `SyncWorldClocks` across 14 captured BDS 1.26.51 sessions
  (`tools/client-diff/conformance-report.md:121`, world research). The re-sync cadence
  with the daylight cycle running still needs F8.
- Which player actor-data keys does `AddPlayer` carry (F2)?

## Appendix: parked WIP commits on `stab-vanilla-survival`

| Commit | Content |
|---|---|
| `8326aa7` | Plugin `get_game_mode` reads the component; dead `packet_domains.rs` removed |
| `5cbabea` | Damage pipeline, hunger, fall/void/drowning/lava, `on_ground` fix, attribute sync, `LevelRules`, message rotation; `unastar-data` bundles implement `Default` |
| `8052ff9` | Melee combat, player list with skins, relays, food data family |
| `bd95e0e` | Command framework, 13 vanilla commands, world clock, spawn points, effect packets |
| `a2cea17` | Comments describe behaviour without non-public sources; WIP `death.rs` (not declared) |
| `5c154f2` | `main` 9dbfbb6 merged |
| this doc | Research doc and the survival rows of `bds-quirks.md` |
