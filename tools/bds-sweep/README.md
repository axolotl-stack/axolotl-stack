# BDS extraction lane (T1 captures, T2 sweeps)

Measures BDS 1.26.51 on loopback so unastar can copy it (decisions D3/D4):
packet captures with `bds-extractor` (T1) and built-in facts with an Endstone
sweep harness (T2). Everything here needs a BDS runtime: the shared one
(`C:/Users/jvigu/orca/bds-runtime`, one session at a time) or a parallel slot
(see [Parallel slots](#parallel-slots)). Never launches the Minecraft client.

| File | Role |
|---|---|
| `bds_runtime.py` | Starts/stops the BDS of one runtime, `BDS_RUNTIME` on `BDS_PORT` (plain or under Endstone): refuses when a `bedrock_server` from that runtime's `server/` runs or the port is bound, backs up and restores `server.properties`, harness-owned world names only, graceful `stop` |
| `capture.py` | T1: two `bds-extractor --capture-dir` sessions on plain BDS (member join; operator join + `/summon` of every summonable identifier) into `<runtime>/captures/1.26.51/<UTC>/`; `--commands FILE` runs one operator session of scripted commands instead (`operator-commands/command-outputs.json`) |
| `command-script.txt` | The vanilla command feedback script for `capture.py --commands` |
| `command_digest.py` | `command-outputs.json` to a readable per-command `digest.md` (CommandOutput fields, key fields of every other packet) |
| `commit_fixtures.py` | Copies the committed subset of a capture into `crates/bds-extractor/captures/1.26.51/` (see the crate README) |
| `sweep.py` | T2: one sweep job under Endstone with `bds-sweep-client` as the player; output in `<runtime>/sweeps/1.26.51/<kind>-<UTC>/` |
| `endstone-sweep/` | The Endstone Python plugin that runs jobs server-side (inert unless `BDS_SWEEP_JOB` is set) |
| `summarize.py` | Raw sweep results to `crates/unastar-data/data/measured/` + `manifest.json` |
| `knockback_fixture.py` | `knockback` sweeps (run with `--client-capture`) to the committed knockback fixture `crates/unastar/src/entity/mob/combat/testdata/bds-1.26.51-knockback.json`: per-tick mob positions, knockback events, and the mob's position and motion packets |
| `replication_fixture.py` | `knockback --items cadence` / `cadence2` / `cadence3` sweeps (run with `--observer --client-capture`) to the committed replication fixture `crates/unastar/src/entity/mob/net/testdata/bds-1.26.51-replication.json`: per-tick actor state, where each viewer stood, and each viewer's `AddActor`, `MoveActorDelta` and `SetActorMotion` about the actor (a vehicle's deltas matched with its wire offset) |
| `range_fixture.py` | `range` sweeps (one per view / simulation distance) to `crates/unastar/src/entity/testdata/bds-1.26.51-send-range.json`: each marker's kind and position, the observer's steps, and the step and tick of every add and remove it got |
| `player_relay_fixture.py` | `walk` sweeps to `crates/unastar/src/server/testdata/bds-1.26.51-player-relay.json`: for every input of the sweep player, the input it sent and the `MovePlayer` the observer got for it |
| `spawn_bonus_fixture.py` | `bonus` sweeps (with `--keep-world`; their actor records dumped by `cargo run -p unastar --example bds_actor_records`) to `crates/unastar/src/entity/mob/persistence/testdata/bds-1.26.51-spawn-bonus.json`: per kind, how many were saved and every `RandomSpawnBonus` amount |
| `effects_fixture.py` | `effects` sweeps (run with `--client-capture`) to the committed effects fixture `crates/unastar/src/entity/effects/testdata/bds-1.26.51-effects.json`: per item the commands and their ticks, every damage event, the mob's position and health per tick and every `MobEffect`, effect actor data, `UpdateAttributes` and `ActorEvent` about the mob or the player, each on the tick of the `bds-sweep:mark` before it (decoded with `bds-extractor --decode`) |
| `mooshroom_fixture.py` | `mooshroom` sweeps (`--items basic` and `--items geometry`, run with `--client-capture`) to `crates/unastar/src/entity/mob/interaction/testdata/bds-1.26.51-mooshroom.json`: per item the mooshroom's feet, facing and events, and per interaction BDS let through the player's eyes and held item, the particle level event, the held slot afterwards, the sounds, the mark variant, the dropped items and the actor it turned into, each with its tick |
| `ranged_fixture.py` | `ranged` sweeps (every item set, run with `--client-capture`) to `crates/unastar/src/entity/mob/combat/testdata/bds-1.26.51-ranged.json`: per item the difficulty, the mob, the player and the setup blocks, the target / facing / shaking flag changes, every arrow (sample, AddActor position, velocity, rotation, owner, aux, the batch's sounds, the player's feet, when it stuck and was taken), damage events, the player's `MobEffect`s, and the mob's positions when it moved |
| `silverfish_fixture.py` | `silverfish` sweeps (run with `--client-capture`) to `crates/unastar/src/entity/mob/ai/testdata/bds-1.26.51-silverfish.json`: per merge item the pit's block, mobGriefing, the spawn feet, the `SilverfishMergeAnim` tick, the block that changed (position, network id) and the `RemoveActor` tick; per wake item the hurt tick, the infested cells and per tick the destroy particles, the bare block updates and where each released silverfish's `AddActor` stood |
| `creeper_fixture.py` | `creeper` sweeps (run with `--client-capture`) to `crates/unastar/src/entity/mob/ai/testdata/bds-1.26.51-creeper.json`: per item the creeper's and the player's feet every tick, whether the player was a target, the actions, and the creeper's `TARGET`, `IGNITED` / `POWERED` / `MOVING` flag changes, `FUSE_TIME` values, sounds, explosion level event and `RemoveActor`, each with its tick; a later sweep's item replaces an earlier one of the same name |
| `slime_fixture.py` | `slime` sweeps (run with `--client-capture`) and `slime-spawn` runs to `crates/unastar/src/entity/mob/ai/testdata/bds-1.26.51-slime.json`: per item the mob, its size, its first 120 ticks, every tick its ground contact or yaw changed and its hops; for `attack` the player's hits; for `split` where the body was last seen and each child's `AddActor` and `born` sound; and every slime `slime-spawn` saw spawn |
| `drowned_fixture.py` | `drowned` sweeps (run with `--client-capture`) to `crates/unastar/src/entity/mob/ai/testdata/bds-1.26.51-drowned.json`: per item the player's feet, the summoned mob's ticks (`sink` and `hunt` items), every damage the player took, each thrown trident's first sample, for `convert` items the tick the mob turned and what it held, and the equipment of the 200 `equipment` drowned |
| `mob_fixture.py` | The generic fixture of the tracked-mob sweeps (`bat`, `ghast`, `polar-bear`, `tadpole`, `vex`, run with `--client-capture`; `--only` picks item kinds, the key without its last `-` part), e.g. `crates/unastar/src/entity/mob/ai/testdata/bds-1.26.51-polar_bear.json`: per item the player's feet whenever they moved, the setup and actions, every damage event, and per tracked actor its ticks (feet, yaw, pitch, velocity, ground contact, health), its actor flags at `AddActor` (taken from anywhere in the capture, as an item's `AddActor` can come before its first mark) and every flag that changed, its `ActorEvent`s and its sounds, each with its tick |
| `native.py` | Readers for protocolgen's CC0 native data (block palette, mining classes, items, item tags) the jobs are built from |
| `test_bds_sweep.py` | Tests for the BDS-free parts: `python -m unittest discover -s tools/bds-sweep -p "test_*.py"` |

## Setup (once)

```powershell
$py = 'C:/Users/jvigu/orca/bds-runtime/venv/Scripts/python.exe'
& $py -m pip install --no-deps -e tools/bds-sweep/endstone-sweep
cargo build -p bds-extractor -p bds-sweep
```

## Parallel slots

The shared runtime runs one BDS at a time. To run several captures or sweeps at
once, use slot runtimes: copies of the server folder, each on its own port. The
launcher only refuses a `bedrock_server` started from its own `server/` folder,
so slots never block each other. A slot directory holds `server/` (a copy of the
shared server without its worlds, logs or crash reports), `world-template/`,
`captures/`, `sweeps/` and `venv`, a directory junction to the shared venv, so
the editable `endstone-bds-sweep` install is shared.

| Slot | `BDS_RUNTIME` | `BDS_PORT` |
|---|---|---|
| 1 | `C:/Users/jvigu/orca/bds-slots/slot1` | 19150 |
| 2 | `C:/Users/jvigu/orca/bds-slots/slot2` | 19160 |
| 3 | `C:/Users/jvigu/orca/bds-slots/slot3` | 19170 |
| 4 | `C:/Users/jvigu/orca/bds-slots/slot4` | 19180 |

Set both variables before running `capture.py` or `sweep.py`; the harness passes
`127.0.0.1:<BDS_PORT>` to `bds-extractor` and `bds-sweep-client`. The IPv6 port
is `BDS_PORT + 1`. Keep slot ports off 19132, where unastar listens. Agents
sharing the machine claim a slot with `mkdir <slot>/agent.lock` (atomic), write
who and what into `agent.lock/owner.txt`, and remove the directory when the run
ends.

To create a slot, copy `bds-runtime/server` without `worlds/`, `logs/` and
`crash_reports/`, copy `world-template/`, create empty `server/worlds/`,
`captures/` and `sweeps/`, and link the venv:
`mklink /J <slot>\venv C:\Users\jvigu\orca\bds-runtime\venv`. Slots hold the
same BDS 1.26.51 build; the launcher checks `version.txt` either way.

## Scripted client controls

The plugin sends these chat orders to `bds-sweep-client`:

| Order | Behaviour |
|---|---|
| `bds-sweep:mine X Y Z FACE PREDICT` | Start/continue breaking; optional predicted break after `PREDICT` ticks |
| `bds-sweep:stop` | Abort the current mining action |
| `bds-sweep:attack RUNTIME_ID` | Attack using the synchronized held stack |
| `bds-sweep:interact RUNTIME_ID` | Interact with the actor using the synchronized held stack (the first use after the plugin changed the hand is refused and makes BDS resend the slot) |
| `bds-sweep:drop hotbar SLOT COUNT` | Closed-screen Drop from hotbar index 0..8; count 1 is Q, a full stack is ctrl-Q |
| `bds-sweep:drop inventory SLOT COUNT` | Open the inventory, wait for its server-allocated window ID, then Drop from native inventory index 0..35 |
| `bds-sweep:close-inventory` | Close the tracked native inventory window |
| `bds-sweep:walk DX DY DZ DYAW TICKS FLAGS [DPITCH]` | For the next `TICKS` inputs, move the reported position by (`DX`, `DY`, `DZ`) and turn yaw and head yaw by `DYAW` (pitch by `DPITCH`), all in thousandths of a block or degree per input; `FLAGS` bit 0 adds forward movement (`Up`, move vector (0, 1)), bit 1 `VerticalCollision` |

Drop references come only from server `InventoryContent`/`InventorySlot`
updates; missing/empty/idless slots and invalid counts fail rather than invent
a stack net ID. Requests use negative odd IDs. `--capture-dir DIR` records
both directions, including unhandled actor packets, to `DIR/packets.jsonl`;
records are flushed individually and the recorder closes on normal/error exit.
Windows debug startup uses the same enlarged codec stack as `bds-extractor`.
The entity probe's `player-drop` mode drives the bounded two-client experiment;
see `crates/unastar/README.md` for its setup and retained native evidence.


## Jobs

| Kind | What one item measures | Arena / method |
|---|---|---|
| `drops` | `samples` x `/loot spawn … mine … mainhand` per (block state, tool) | block at (3,-60,0) set without physics; tools: hand, every tier of each correct digger, one wrong digger, shears, silk touch and fortune I-III on the diamond correct tool |
| `mining` | break ticks per (block state, tool, effect/environment), 3 trials | player (survival) at (0.5,-60,0.5) facing the west face of (1,-59,0); client sends Start/ContinueDestroyBlock, BDS breaks the block itself; ticks from the LEFT_CLICK_BLOCK event to the BlockBreakEvent (±1 tick) |
| `armor-slots` | `/replaceitem` accept per slot (head, chest, legs, feet, offhand) | read back with Endstone `PlayerInventory` |
| `armor-values` | health lost per `/damage <player> D entity_attack` wearing one item | max health 2000, hits repeated for sub-point precision; needs the full `armor-slots` result for slot assignment |
| `attack` | melee damage per held item, warm-up + 3 trials | a fresh pig at (2,-60,0.5); the client sends an InventoryTransaction attack; Endstone `ActorDamageEvent` value |
| `fuel` | furnace burn ticks per item | pool of 32 furnaces, 1 cobblestone input + the item as fuel |
| `knockback` | one mob's motion, tick by tick, around the sweep player's hits (or under its AI) | a fresh cow or zombie, frozen with Slowness 255 or left to its AI; before each hit the player is put 2 blocks west (or south-west) of it, facing east (or west); Endstone samples position, rotation, velocity (the last tick's position change) and ground contact every tick, and records each `ActorKnockbackEvent` (its `knockback` is the velocity change applied). `--items slice` (hits), `air` (tempted cows jumping a step, mobs hit while falling, a panicking cow), `far` (cows falling at 4-48 blocks, for the send rate by distance), `cadence` and `cadence2` (two viewers per actor, see below) |
| `knockback-reload` | whether a zombie's knockback response survives a save and load | `--items spawn --keep-world`: three named, frozen zombies 200 blocks from the world spawn, each hit once and left in the world; then `--items reload --world-from <spawn run>/world`: the loaded zombies (found by name) hit again; the server starts at easy, as peaceful would remove them |
| `range` | which actors BDS sends a viewer, by where it stands (needs `--observer`) | still markers (cows, armor stand, item, arrow, minecart, xp orb, and the sweep player) around chunk (0, 0); the observer walks away and back along +x, the diagonal and -x in one-block steps of ten ticks, then jumps between 40 and 112 blocks; a `bds-sweep:mark` chat line every tick puts each observer packet on a tick. Run once per `--view-distance` / `--tick-distance` |
| `bonus` | the random spawn bonuses BDS saves for each kind (`--keep-world`, `--difficulty easy\|hard`) | 870 mobs of 13 kinds (no villagers, golems, wolves or illagers, which fight the others) frozen on a grid around the sweep player and left in the world; read their records from the kept world |
| `walk` | how BDS relays a player's movement to another player (needs `--observer`) | the sweep player driven by `bds-sweep:walk` (standing, turning, walking, stepping up, tilting) with the observer 8 blocks south and a cow spawned at yaw 33.3 for its `AddActor` rotation |
| `effects` | what effects do to a mob or to the sweep player, tick by tick (`--items mobs\|players\|all`) | a cow or zombie in a barrier cell three blocks east, or the survival player; `/effect` and `/damage` at fixed ticks (poison, regeneration, wither, instant effects, resistance, fire resistance, health boost, absorption, speed, slowness, invisibility, hidden particles, several at once, stacking, clearing, expiry; levitation and slow falling on a frozen cow; hunger and saturation on the player); every tick a `bds-sweep:mark` chat line puts the capture on the samples |
| `ranged` | how the skeleton family shoots (once per `--difficulty`); `--items geometry`, `aim`, `spread`, `damage`, `damage-long`, `weapon`, `filters`, `avoid`, `avoid-many` and `pickup` add the start point, the aim, the spread, the arrow damage, the ranged weapons, the `is_underground` / `in_block` filters and the trap sensor, avoidance and arrow pickup | a frozen skeleton (8 and 15 blocks), stray, bogged and parched shooting at the survival sweep player, kept alive by Resistance V and Regeneration, 300 ticks each; the other sets pin the player in a barrier pen, raise or move the shooter, replace its hand, put a stray under roofs, a skeleton in powder snow or next to a frozen wolf, or drop a dispensed arrow beside the player |
| `melee-effects` | the effect a husk's hit applies, per `--difficulty` | a husk walks up to the survival player and hits it for 200 ticks; Health Boost X, a full heal and Regeneration V keep the player alive without Resistance (which would change the hits); `--client-capture` holds each hit's hunger `MobEffect` |
| `mooshroom` | the mooshroom's interactions | the survival player holding a bowl, an allium (brown mooshroom) or shears interacts with a frozen mooshroom (`bds-sweep:interact`, twice); `--items geometry` moves the particle: the player on a block diagonally above, a baby mooshroom, and a bowl after the allium (suspicious stew). One mooshroom in 20 spawns a baby, which has no interactions, so those items grow theirs up first |
| `silverfish` | the silverfish's goals: `--items merge` (a 3 × 3 pit), `pit` (a 1 × 1 stone pit, 20 times), `blocks` (1 × 1 pits of 14 kinds of block, 3 times each), `griefing` (mobGriefing off), `ab` (stone and cobblestone pits interleaved, 20 each), `wake` and `wake-nogrief` (infested stone around a frozen silverfish hurt on tick 40) | a silverfish with no target in a pit dug into the block, or frozen among infested stone with the protected survival player 4 blocks west |
| `creeper` | the creeper's fuse (`--items slice`, or item names: `approach`, `cancel`, `sight`, `flint`, `charged`, `no-griefing`, `flint-alone`, `gap`, `gap-far`) | the survival player under Resistance V in a barrier pen, a frozen or walking creeper 2-6 blocks east; the floor the blast broke is rebuilt after every item |
| `slime` | the slime and the magma cube, summoned at a size (`summon … spawn_<size>`), every one of them sampled every tick (`track`): `--items hop` (calm, 600 ticks, three times per size), `attack` (the survival player, Health Boost X, a barrier wall behind it), `split` (killed by `/damage` or `/kill`, 80 ticks), `float` (a slime in water, a magma cube in lava) or `all` | a barrier wall at x = -1 behind the player for `attack`; the job removes the tracked mobs without killing them, so nothing splits into the next item |
| `slime-spawn` | natural slime spawning (`spawn_watch`: every new slime recorded and removed, other monsters removed) | the still creative player at the origin of the superflat world (floor y -61), normal, midnight, mob spawning on; two items of `--samples` × 100 ticks |
| `drowned` | the drowned in a glass pool (`POOL`, water x 3..19, y -60..-54), every drowned, zombie, husk and thrown trident sampled every tick: `--items sink` (no target), `hunt` (the survival player on the pool floor, on a pillar at the surface, on land), `trident` (a ranged drowned, no Resistance), `day` (noon, the player in the water or on land), `equipment` (200 drowned with their natural spawn event), `convert` (a zombie, a baby zombie and a husk under water for 1,100 ticks) or `all` | mobs summoned with a spawn event (`summon … minecraft:as_adult`); the player kept alive by Health Boost X, Regeneration V and Water Breathing |
| `witch` | the witch: `--items throw` (the player 3, 6, 9 and 10 blocks away), `drink` (under water, in fire, hurt, target moved away), `resist` (`/damage` of several causes) or `all` | a frozen witch, the survival player in a barrier pen open towards it, no Resistance; findings in `docs/research/witch-bds-1.26.51.md` |
| `bat` | the bat: `--items room` (a closed glass room, 1,200 ticks, four times), `open` (open sky, 600 ticks, twice), `cave` (a dark stone room three blocks tall, 2,400 ticks, three times), `wake` (the cave, the creative or survival player teleported into it on tick 1,500), `low` (a stone room one block tall, the bat hurt by 1 on ticks 20, 120, 220 and 320, eight times), `near` (resting in a narrower one-block room, the player outside its wall 3.85 to 6 blocks away), `unroof` (the block over the resting bat removed on tick 40), `canopy` (resting under a stone roof two blocks over the floor, the player teleported 3.75 to 4.1 blocks away in clear sight) or `all` | the bat summoned (its definition has no events) and sampled every tick, the creative player outside unless an item moves it |
| `ghast` | a ghast high over the floor (`--items open`, 600 ticks, twice): how a second float-navigating flyer drags | the creative player far away |
| `vex` | the vex: `--items idle` (in the glass room, 400 ticks, twice), `charge` (the survival player 8 blocks away, Health Boost X and Regeneration V), `decay` (summoned with `minecraft:add_damage_timer`, 2,800 ticks) or `all` | the vex summoned and sampled every tick |
| `polar-bear` | the polar bear: `--items stroll`, `attack` (the survival player hits it with `/damage` from 3 blocks), `stomp` (an angered adult frozen 1.4 to 5 blocks from the player held by barrier walls), `baby` (a baby's anger reaching an adult), `swim`, `swim-push` (hit across the glass pool) or `all` | adults summoned with `minecraft:ageable_grow_up`, babies with `minecraft:entity_born`; the player kept alive by Health Boost X and Regeneration V |
| `tadpole` | the tadpole in the glass pool: `--items swim` (600 ticks, four times), `grow` (summoned with `ageable_grow_up`, the tadpole and its frog sampled, 60 ticks, three times) or `all` | the creative player outside |
| `entity-loot` | `/loot spawn … kill <mob>` outcomes, like `drops` for blocks (`--items <mob>,...`, `--samples`) | a frozen, resistant mob; the sweep player runs the command |

```powershell
& $py tools/bds-sweep/sweep.py drops --blocks all --samples 400   # first palette state of every block
& $py tools/bds-sweep/sweep.py drops --blocks minecraft:wheat --states all
& $py tools/bds-sweep/sweep.py mining --blocks slice
& $py tools/bds-sweep/sweep.py armor-slots --items all
& $py tools/bds-sweep/sweep.py fuel --items all
& $py tools/bds-sweep/summarize.py drops <dir> [<repeat dir> ...]
& $py tools/bds-sweep/sweep.py knockback --items slice --client-capture   # also air, far, resist
python tools/bds-sweep/knockback_fixture.py <fixture.json> <sweep dir> [<sweep dir> ...]
& $py tools/bds-sweep/sweep.py knockback --items cadence --observer --client-capture --view-distance 10 --tick-distance 10
python tools/bds-sweep/replication_fixture.py <fixture.json> <cadence dir> <cadence2 dir>
& $py tools/bds-sweep/sweep.py knockback-reload --items spawn --client-capture --keep-world --out <spawn dir>
& $py tools/bds-sweep/sweep.py knockback-reload --items reload --client-capture --world-from <spawn dir>/world
& $py tools/bds-sweep/sweep.py knockback --items cadence3 --observer --client-capture --view-distance 10 --tick-distance 10
& $py tools/bds-sweep/sweep.py range --observer --client-capture --view-distance 10 --tick-distance 10   # also 6/4, 16/12
python tools/bds-sweep/range_fixture.py <fixture.json> 10,10:<dir> 6,4:<dir> 16,12:<dir>
& $py tools/bds-sweep/sweep.py walk --observer --client-capture --client <a client built from this checkout>
python tools/bds-sweep/player_relay_fixture.py <fixture.json> <walk dir>
& $py tools/bds-sweep/sweep.py bonus --difficulty easy --keep-world --client-capture   # and hard
cargo run -p unastar --example bds_actor_records -- <bonus dir>/world/db <bonus dir>/actor_records.jsonl
python tools/bds-sweep/spawn_bonus_fixture.py <fixture.json> <easy dir> <hard dir>
& $py tools/bds-sweep/sweep.py effects --client-capture   # --items mobs | players
python tools/bds-sweep/effects_fixture.py <fixture.json> <effects dir> [<effects dir> ...]
& $py tools/bds-sweep/sweep.py ranged --difficulty easy --client-capture   # and normal, hard
& $py tools/bds-sweep/sweep.py ranged --items aim --difficulty hard --client-capture   # geometry, spread, damage, ...
python tools/bds-sweep/ranged_fixture.py <fixture.json> <ranged dir> [<ranged dir> ...]
& $py tools/bds-sweep/sweep.py mooshroom --client-capture   # and --items geometry
& $py tools/bds-sweep/sweep.py melee-effects --difficulty hard --client-capture
python tools/bds-sweep/mooshroom_fixture.py <fixture.json> <basic dir> <geometry dir>
& $py tools/bds-sweep/sweep.py silverfish --items pit --client-capture   # also merge, blocks, griefing, wake, wake-nogrief
python tools/bds-sweep/silverfish_fixture.py <fixture.json> <silverfish dir> [<silverfish dir> ...]
& $py tools/bds-sweep/sweep.py creeper --client-capture   # or --items flint,charged
python tools/bds-sweep/creeper_fixture.py <fixture.json> <creeper dir> [<creeper dir> ...]
& $py tools/bds-sweep/sweep.py slime --items hop --client-capture   # also attack, split, float
& $py tools/bds-sweep/sweep.py slime-spawn --samples 120 --client-capture
python tools/bds-sweep/slime_fixture.py <fixture.json> <hop dir> <float dir> <attack dir> <split dir> <spawn dir>
& $py tools/bds-sweep/sweep.py drowned --items all --difficulty normal --client-capture
python tools/bds-sweep/drowned_fixture.py <fixture.json> <drowned dir>
& $py tools/bds-sweep/sweep.py witch --items all --difficulty normal --client-capture
& $py tools/bds-sweep/sweep.py polar-bear --items all --difficulty normal --client-capture   # and bat, tadpole
python tools/bds-sweep/mob_fixture.py --only attack,baby,swim-push,stomp <fixture.json> <polar-bear dir> [<dir> ...]
python tools/bds-sweep/mob_fixture.py --only open,low,unroof,canopy-survival,canopy-creative <bat fixture.json> <bat dirs>
& $py tools/bds-sweep/sweep.py entity-loot --items wither_skeleton --samples 800
cargo run -p bds-extractor -- --decode <dir>/client/packets.jsonl --only MobEffectPacket --output <out.jsonl>
```

`--items cadence` and `cadence2` measure how often each viewer gets a moving
actor's position: run them with `--observer --client-capture --view-distance 10
--tick-distance 10`. `--observer` starts a second `bds-sweep-client`
(`BDSOther`, recorded to `<out>/observer/packets.jsonl`) a few seconds after the
sweep player has spawned, since BDS refuses a second loopback login right
after the first ("ip recently connected"); the job waits for it and an item's
`observer` (x, y, z, yaw) places it. Items may run timed commands
(`actions: [{t, commands}]`). BDS sends an actor only to viewers about four
chunks around it whatever the view distance, and a cow more than 128 blocks
from every player despawns at once, so keep trials inside both.
`--keep-world` copies the world to `<out>/world` after BDS stops (read actor
records from its LevelDB); `--world-from DIR` starts the next run from such a
copy.

`--items cadence3` adds vehicles (a minecart and a boat resting on a pillar that
is removed in two steps) and projectiles a dispenser shoots (`spawn_commands`:
the job follows the first new actor of the item's type those commands make).
`range` and `walk` need `--observer`; `range` measured the same switches at view /
simulation distance 6 / 4, 10 / 10 and 16 / 12, so one run is enough to
recheck it. BDS simulates a player's movement itself: a `walk` without the
forward flag is corrected to a drift, and the observer sees the server's position.

`--client-capture` records the sweep player's packets (both directions) in
`<out>/client/packets.jsonl`. The BDS process gets this checkout's
`endstone-sweep/src` first on `PYTHONPATH`, so a worktree runs its own jobs
even though the shared venv's editable install points at another checkout;
the plugin logs `jobs loaded from <path>` at startup.

Sweeps are resumable: rerun with the same `--out` and finished items
(`results.jsonl`) are skipped. To hand the BDS over mid-run, create
`<out>/stop.request`; the runner stops BDS gracefully and restores
`server.properties`. To pause a queue of runs at the next job boundary,
create `<runtime>/sweeps/1.26.51/hold`: every new `sweep.py` exits without
starting BDS while it exists.

A few block states crash Endstone in `Block.set_data`. The plugin marks the
item in flight (`<out>/inflight.json`, removed on a graceful stop); when a
run dies on it, the resumed run records that state as `crashed` for every
tool and moves on, so rerunning `sweep.py` with the same `--out` until
`done.json` appears completes the sweep.

## Runtimes observed (2026-10-25, turbo boost off; compare only like with like)

| Run | Items | Time |
|---|---|---|
| `capture.py` (both sessions, 112 summons) | 2 sessions | ~4 min |
| `drops --blocks all --samples 400` (first palette state of every block) | 17,073 cells, 6.8 M samples | 15 min |
| `drops` slice, 10 blocks x 7-14 tools x 200 samples | 126 | 6-15 s of sweep work, ~40 s with BDS start/stop |
| `armor-slots --items all` | 2,075 | 13 s |
| `armor-values` (31 items, 31 hits each) | 31 | 12 min |
| `attack` (43 tools, warm-up + 3 trials) | 43 | 2 min |
| `fuel --items all` (lava bucket burns 20,000 ticks) | 2,075 | 19 min |
| `mining --blocks slice` (10 blocks, 3 trials per variant) | 176 | ~20 min in two parts |
| `drops --blocks all --states all --samples 64` | 287,231 cells | ~50 min over 7 attempts (5 crashing states skipped, 3 handovers) |
