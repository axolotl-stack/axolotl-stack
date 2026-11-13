# BDS extraction lane (T1 captures, T2 sweeps)

Measures BDS 1.26.51 on loopback so unastar can copy it (decisions D3/D4):
packet captures with `bds-extractor` (T1) and built-in facts with an Endstone
sweep harness (T2). Everything here needs a BDS runtime: the shared one
(`C:/Users/jvigu/orca/bds-runtime`, one session at a time) or a parallel slot
(see [Parallel slots](#parallel-slots)). Never launches the Minecraft client.

| File | Role |
|---|---|
| `bds_runtime.py` | Starts/stops the BDS of one runtime, `BDS_RUNTIME` on `BDS_PORT` (plain or under Endstone): refuses when a `bedrock_server` from that runtime's `server/` runs or the port is bound, backs up and restores `server.properties`, harness-owned world names only, graceful `stop` |
| `capture.py` | T1: two `bds-extractor --capture-dir` sessions on plain BDS (member join; operator join + `/summon` of every summonable identifier) into `<runtime>/captures/1.26.51/<UTC>/`; `--commands FILE` runs one operator session of scripted commands instead (`operator-commands/command-outputs.json`, plus `console-outputs.json`: BDS's console lines by step, typed for `!console` steps; `allowlist.json`, `permissions.json` and `keys/` are restored afterwards) |
| `command-script.txt` | The vanilla command feedback script for `capture.py --commands` (`# segment: NAME` groups lines that unastar's tests replay together; every segment leaves the world as it found it; `!console COMMAND` runs on the server console, `!join NAME` / `!leave NAME` bring a second player in and out) |
| `capture.py --pair FILE` | Two or more overlapping operator sessions on one fresh world: `host/` runs FILE, guests join and leave during it (`--guests N`, one after the other or `--guest-gap S` seconds apart and online together); for what BDS sends online players when others join and leave |
| `roster_probe.py` | Player-colour reuse probe: a guest leaves before the next one joins (schedule in its docstring) |
| `commit_roster_fixtures.py` | Copies the roster packets of `--pair`/probe runs into `crates/bds-extractor/captures/1.26.51/roster/` |
| `command_digest.py` | `command-outputs.json` to a readable per-command `digest.md` (CommandOutput fields, key fields of every other packet) |
| `commit_fixtures.py` | Copies the committed subset of a capture into `crates/bds-extractor/captures/1.26.51/` (see the crate README); `--commands CAPTURE` commits a command session's `feedback.json` |
| `scripts/command-order.txt` | A `bds-sweep-client --script` script: block commands sent right before (or one input before) the input that starts breaking the block, on plain BDS (flat world, survival, operator) |
| `command_order_fixture.py` | Cuts a capture of `scripts/command-order.txt` into `crates/unastar/src/server/game/testdata/bds-command-order.jsonl` (commands, inputs and crack starts of four scenarios) |
| `player_script.py` | Plain-BDS scripted player captures: a timed script of console commands and client orders with every client's packets recorded (see [Movement](#movement)) |
| `player_fixture.py` | Distils `player_script.py` captures into a value-only test fixture: per phase the subject's inputs, the packets one client got about the subject, and the subject's attributes before the phase |
| `block_names.py` | `script OUT` writes a `/testforblock` script over every palette block; `table CAPTURE` turns its capture into `crates/unastar-data/data/measured/block_name_keys.json` |
| `sweep.py` | T2: one sweep job under Endstone with `bds-sweep-client` as the player; output in `<runtime>/sweeps/1.26.51/<kind>-<UTC>/` |
| `endstone-sweep/` | The Endstone Python plugin that runs jobs server-side (inert unless `BDS_SWEEP_JOB` is set) |
| `summarize.py` | Raw sweep results to `crates/unastar-data/data/measured/` + `manifest.json` |
| `knockback_fixture.py` | `knockback` sweeps (run with `--client-capture`) to the committed knockback fixture `crates/unastar/src/entity/mob/combat/testdata/bds-1.26.51-knockback.json`: per-tick mob positions, knockback events, and the mob's position and motion packets |
| `replication_fixture.py` | `knockback --items cadence` / `cadence2` / `cadence3` sweeps (run with `--observer --client-capture`) to the committed replication fixture `crates/unastar/src/entity/mob/net/testdata/bds-1.26.51-replication.json`: per-tick actor state, where each viewer stood, and each viewer's `AddActor`, `MoveActorDelta` and `SetActorMotion` about the actor (a vehicle's deltas matched with its wire offset) |
| `range_fixture.py` | `range` sweeps (one per view / simulation distance) to `crates/unastar/src/entity/testdata/bds-1.26.51-send-range.json`: each marker's kind and position, the observer's steps, and the step and tick of every add and remove it got |
| `player_relay_fixture.py` | `walk` sweeps to `crates/unastar/src/server/testdata/bds-1.26.51-player-relay.json`: for every input of the sweep player, the input it sent and the `MovePlayer` the observer got for it |
| `spawn_bonus_fixture.py` | `bonus` sweeps (with `--keep-world`; their actor records dumped by `cargo run -p unastar --example bds_actor_records`) to `crates/unastar/src/entity/mob/persistence/testdata/bds-1.26.51-spawn-bonus.json`: per kind, how many were saved and every `RandomSpawnBonus` amount |
| `script.py` | Plays a `bds-sweep-client --script` scenario against plain BDS (no Endstone) and records the client's packets; the same script runs against unastar with `bds-sweep-client --addr <unastar> --script ...` |
| `scripts/breaking.txt` | Survival block breaking scenarios: by hand, tools, instant, abort, a frozen client, Haste, a block above, client predictions on time and early, `StopDestroyBlock`, blocks put back where a break goes on, a burst of inputs |
| `breaking_fixture.py` | Cuts the committed breaking fixture (`crates/unastar/src/server/game/blocks/testdata/bds-breaking.jsonl`) out of a `script.py` capture of `scripts/breaking.txt` |
| `native.py` | Readers for protocolgen's CC0 native data (block palette, mining classes, items, item tags) the jobs are built from |
| `scenarios.py` | Block scenario families for the `scenarios` job (stairs, connections, wall_above, gate_in_wall, support, surfaces, soils, junctions, halves; round 2: stairs_blocks, connections_blocks, wall_heights_blocks, multiface_support, rails, soils_more, coral_death) |
| `circuits.py` | Round 3: redstone circuit families for the `scenarios` job (`redstone_*`, `MEASURED`; the `circuits_pilot*` families are the pilots): a circuit built in a 22 x 6 cell, a timeline of inputs at known ticks and a read of every position every tick |
| `random_ticks.py` | Round 3: random tick families (`random_*`): 16 x 16 fields of crops, sugar cane, cacti, copper, leaves and farmland at a set `randomtickspeed`, read every 100 ticks |
| `coral.py` | Round 3: coral death families (`coral_water`, `coral_timing`; round 2's `coral_death` also runs on this layout): cells 16 blocks apart so the water a cell brings reaches no other, coral set, `/setblock`ed, clicked into place or losing its water, read every 10 ticks or every tick |
| `placements.py` | The survival clicks of the `placements` job in case groups (`GROUPS`: round 1's `blocks`, then signs, door hinges, uses, coral, skulls, directions, mushrooms, bells, multi-face blocks, candles, crops, plants and stacks, block actors, follow-up; round 3's `leftovers`: flower pot plants, cave vine ages, cake slices, string): blocks set around the click, the stack in hand, the player's feet and look, `before` commands, the click, follow-up steps, the positions read back |
| `subchunk_fixture.py` | Sub-chunk records BDS saves: builds every palette shape in a flat world (and generates a seed-12345 world), reloads each and checks cells with `/testforblock`, then writes `crates/unastar/tests/fixtures/bds_1_26_51_subchunks.json` (steps `build`, `probe`, `natural`, `natural-probe`, `fixture`) |
| `subchunk_roundtrip.py` | Sub-chunk records between BDS and unastar: one BDS-saved world edited by the same plan from BDS's console (chunks then unloaded) and through unastar's provider (`cargo run -p unastar --example subchunk_roundtrip`), every record of the edited columns compared byte for byte; BDS reopens the world unastar edited and one unastar generated and checks cells with `/testforblock`; writes `crates/unastar/tests/fixtures/bds_1_26_51_subchunk_edits.json` (steps `source`, `plan`, `bds-edit`, `unastar-edit`, `compare`, `bds-verify`, `generate`, `generated-verify`, `fixture`) |
| `leveldb_read.py` | Read-only LevelDB reader (tables and logs, raw-deflate blocks) for BDS world copies, independent of unastar |
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

Each `bds-extractor` debug process needs a few hundred MB: about 20 guests at
once ran this machine out of memory (2026-11-11), so keep `--guests` with
`--guest-gap` around 12 or fewer.

## Scripted client controls

The plugin sends these chat orders to `bds-sweep-client` (plain BDS scripts
send the same text with `tellraw <player> {"rawtext":[{"text":"bds-sweep:..."}]}`):

| Order | Behaviour |
|---|---|
| `bds-sweep:mine X Y Z FACE PREDICT` | Start/continue breaking; optional predicted break after `PREDICT` ticks |
| `bds-sweep:stop` | Abort the current mining action |
| `bds-sweep:pause TICKS` | Send no `PlayerAuthInput` for that many ticks (a frozen client), then carry on |
| `bds-sweep:burst INPUTS` | Send that many more `PlayerAuthInput`s right after the next one, at once (a client running faster than real time) |
| `bds-sweep:release` | Release the button: `StopDestroyBlock` with the default position, and no more mining |
| `bds-sweep:attack RUNTIME_ID` | Attack using the synchronized held stack |
| `bds-sweep:drop hotbar SLOT COUNT` | Closed-screen Drop from hotbar index 0..8; count 1 is Q, a full stack is ctrl-Q |
| `bds-sweep:drop inventory SLOT COUNT` | Open the inventory, wait for its server-allocated window ID, then Drop from native inventory index 0..35 |
| `bds-sweep:close-inventory` | Close the tracked native inventory window |
| `bds-sweep:walk DX DY DZ DYAW TICKS FLAGS [DPITCH]` | For the next `TICKS` inputs, move the reported position by (`DX`, `DY`, `DZ`) and turn yaw and head yaw by `DYAW` (pitch by `DPITCH`), all in thousandths of a block or degree per input; `FLAGS` bit 0 adds forward movement (`Up`, move vector (0, 1)), bit 1 `VerticalCollision`; the body moves by the steps unsimulated (no velocity), and the simulated movement resumes after the walk |
| `bds-sweep:press FLAG...` | Add these `PlayerAuthInput` input flags (lower-case names: `startsneaking`, `sneaking`, `startsprinting`, `startswimming`, `startcrawling`, ...) to the next input only |
| `bds-sweep:hold FLAG...` / `bds-sweep:release FLAG...` | Add the flags to every input from the next one on, until released (`sneaking sneakdown` while a sneak lasts) |
| `bds-sweep:use` | Use the held item in the air (`InventoryTransaction` item use); BDS rejects it for armour and resends the whole inventory, which refreshes stack net IDs |
| `bds-sweep:equip-head SLOT` / `bds-sweep:equip-feet SLOT` | Open the inventory and move one item from hotbar index SLOT to the head or feet armour slot (`ItemStackRequest` place) |
| `bds-sweep:quit` | Close the connection and exit (BDS still keeps the player about 10 s, until its connection times out) |
| `bds-sweep:move F S` | Hold the movement keys: forward `F` (W positive) and strafe `S` (A positive), each -1..1, until the next `move` (`move 0 0` lets go) |
| `bds-sweep:look YAW PITCH` | Turn to this yaw and pitch (degrees; pitch -90..90) |
| `bds-sweep:sprint on\|off` | Press or release the sprint key; a sprint starts while moving forward and lasts until forward movement stops or a sneak starts, as in vanilla |
| `bds-sweep:sneak on\|off` | Press or release the sneak key |
| `bds-sweep:jump [TICKS]` | Hold the jump key for TICKS ticks (default 1); held, the player jumps again on landing |
| `bds-sweep:glide` | Open the elytra: a jump press in the air (needs an elytra in the chest slot) |
| `bds-sweep:use X Y Z FACE CX CY CZ TARGET` | Click block face `FACE` of `X Y Z` with the selected hotbar stack (an `ItemUse` place transaction; click position `C*`/1000 inside the block; `TARGET` the clicked block's runtime id), as a survival player's right click |
| `bds-sweep:sign X Y Z FRONT HEX` | Edit the sign at `X Y Z`: the block-actor data the server last sent for it, with the `Text` of the front (`FRONT` 1) or back side replaced by the UTF-8 text `HEX` (`-` for empty), sent as `BlockActorData` the way a client submits an edit |

`bds-sweep-client --script FILE` plays timed orders without the plugin: one `TICK ORDER` line per
step at that client input tick, an order above without its `bds-sweep:` prefix, a slash command
(sent as `CommandRequest`; the player must be an operator) or `exit`.

### Movement

The client simulates its own movement like a vanilla client and reports it in
every `PlayerAuthInput`, so BDS's server-authoritative movement accepts it:
gravity and drag, ground friction and acceleration (sprint speed 0.13,
air speed 0.02/0.026), jumps (0.42, sprint-jump push 0.2, 10-tick delay),
sneaking (BDS applies the 0.3 itself, so the move vector is sent unscaled)
and its edge stop, collisions with step-up, slime and bed bounces, cobwebs and
powder snow, water (drag 0.8 or 0.9 sprinting, sink 0.005 unless sprinting,
swimming toward the look pitch, the climb-out lift) and elytra gliding. The
blocks come from the world BDS sends: the client asks for the sub-chunks of
the chunks around it (`SubChunkRequest`, BDS's request mode) and follows
`UpdateBlock`/`UpdateSubChunkBlocks`; block states, collision boxes and
friction come from `unastar-data`. Until the blocks around it have arrived the
player stays put.

Input flags follow the keys like a vanilla client's (`Up`, `Sprintdown`,
`Sprinting` with `StartSprinting`/`StopSprinting`, the sneak and jump flags
with their raw-key companions, `StartSwimming`, `StartGliding`, the collision
flags, `HandledTeleport`). The wire position is the feet plus 1.62001 in
every pose, `pos_delta` the velocity after the move. Server packets set the
state: a teleport (`MovePlayer`) leaves the player resting at its destination
through the next input, as BDS does, `SetActorMotion` sets the velocity,
`UpdateAttributes` the movement speed, `MobEffect` jump boost and slow
falling. A `CorrectPlayerMovePrediction` rewinds to BDS's state at its tick
and replays the inputs sent since; the client logs each one that disagrees
with its own prediction (`BDS corrected the movement`) and the total on exit.
With these rules, walking, strafing, sprinting, sneaking, jumps, falls,
swimming and gliding run without a correction (2026-11-11). Still corrected:
knockback landings (the client applies `SetActorMotion` when it arrives, a tick
after BDS's copy takes it) and the entry into powder snow; after a sprint-swim
stop the client keeps the sprint's water drag a tick too long (inside the
threshold, uncorrected); lava is not simulated.

Tooling for such runs:

- `player_script.py SCRIPT` runs a timed script on plain BDS (`launch`,
  `wait`, `cmd`, `order`, `mark`, `expect`, `quit` steps; see its docstring)
  with each client's capture, `bds.log` and `phases.json` in
  `<runtime>/captures/<label>-<UTC>/`.
- `cargo run -p bds-sweep --bin bds-sweep-dump -- CAPTURE/packets.jsonl
  --name PlayerAuthInput --from MS --to MS` prints decoded packets.
- `player_fixture.py SPEC.json OUT.json --captures DIR` writes a fixture from
  such captures (needs `bds-sweep-dump` built): each phase names a capture, the
  recording client, the subject and its client tick range; the packets kept are
  those between the first and last packet about the subject stamped in that
  range. `unastar`'s movement tests replay these
  (`crates/unastar/src/entity/player_life/testdata/bds-1.26.51-movement.json`).
- A second client standing near the first receives BDS's own simulation of it
  (`MovePlayer` every tick); while the first client disagrees, BDS moves its
  copy towards the client's position by up to about 0.005 blocks a tick, so
  calibrate against runs without corrections.

Drop references come only from server `InventoryContent`/`InventorySlot`
updates; missing/empty/idless slots and invalid counts fail rather than invent
a stack net ID. `/enchant` updates a slot through a legacy transaction the
client does not track, so send `use` (or another full resync) before an
`equip-head` of an enchanted item. `--uuid UUID` logs in with a fixed identity,
so a player that leaves and rejoins gets its saved data back. Requests use negative odd IDs. `--capture-dir DIR` records
both directions, including unhandled actor packets, to `DIR/packets.jsonl`;
records are flushed individually and the recorder closes on normal/error exit.
Windows debug startup uses the same enlarged codec stack as `bds-extractor`.
The entity probe's `player-drop` mode drives the bounded two-client experiment;
see `crates/unastar/README.md` for its setup and retained native evidence.


## Jobs

| Kind | What one item measures | Arena / method |
|---|---|---|
| `drops` | `samples` x `/loot spawn … mine … mainhand` per (block state, tool) | block at (3,-60,0) set without physics; tools: hand, every tier of each correct digger, one wrong digger, shears, silk touch and fortune I-III on the diamond correct tool |
| `mining` | break ticks per (block state, tool, effect/environment), 3 trials (`--trials` for the held and effect sets), and the held stack's durability damage after every break | player (survival) at (0.5,-60,0.5) facing the west face of (1,-59,0); client sends Start/ContinueDestroyBlock, BDS breaks the block itself; ticks from the LEFT_CLICK_BLOCK event to the BlockBreakEvent (±1 tick). `--variants tools` (default): the tool slice; `held`: `--items` held on `--blocks` (default every non-tool item the parity catalog gives a `digger` facet, on dirt, netherrack and oak leaves, plus every tool on dirt and netherrack for its wear); `effects`: Haste I-III by hand on stone, Mining Fatigue I-II by hand on dirt |
| `armor-slots` | `/replaceitem` accept per slot (head, chest, legs, feet, offhand) | read back with Endstone `PlayerInventory` |
| `armor-values` | health lost per `/damage <player> D entity_attack` wearing one item | max health 2000, hits repeated for sub-point precision; needs the full `armor-slots` result for slot assignment |
| `armor-wear` | durability one `/damage <player> D entity_attack` hit costs a worn piece, per D (1, 3, 4, 5, 7, 8, 12, 16, 40) | one breakable piece in its `armor_slots.json` slot, read back through Endstone before and after every hit |
| `attack` | melee damage per held item, warm-up + 3 trials, and the held stack's durability damage after every trial | a fresh pig at (2,-60,0.5); the client sends an InventoryTransaction attack; Endstone `ActorDamageEvent` value. `--gamemode survival` lets hits wear the item; `--items spears` adds every spear tier, `--gap` ticks apart |
| `fuel` | furnace burn ticks per item | pool of 32 furnaces, 1 cobblestone input + the item as fuel |
| `knockback` | one mob's motion, tick by tick, around the sweep player's hits (or under its AI) | a fresh cow or zombie, frozen with Slowness 255 or left to its AI; before each hit the player is put 2 blocks west (or south-west) of it, facing east (or west); Endstone samples position, rotation, velocity (the last tick's position change) and ground contact every tick, and records each `ActorKnockbackEvent` (its `knockback` is the velocity change applied). `--items slice` (hits), `air` (tempted cows jumping a step, mobs hit while falling, a panicking cow), `far` (cows falling at 4-48 blocks, for the send rate by distance), `cadence` and `cadence2` (two viewers per actor, see below) |
| `knockback-reload` | whether a zombie's knockback response survives a save and load | `--items spawn --keep-world`: three named, frozen zombies 200 blocks from the world spawn, each hit once and left in the world; then `--items reload --world-from <spawn run>/world`: the loaded zombies (found by name) hit again; the server starts at easy, as peaceful would remove them |
| `range` | which actors BDS sends a viewer, by where it stands (needs `--observer`) | still markers (cows, armor stand, item, arrow, minecart, xp orb, and the sweep player) around chunk (0, 0); the observer walks away and back along +x, the diagonal and -x in one-block steps of ten ticks, then jumps between 40 and 112 blocks; a `bds-sweep:mark` chat line every tick puts each observer packet on a tick. Run once per `--view-distance` / `--tick-distance` |
| `bonus` | the random spawn bonuses BDS saves for each kind (`--keep-world`, `--difficulty easy\|hard`) | 870 mobs of 13 kinds (no villagers, golems, wolves or illagers, which fight the others) frozen on a grid around the sweep player and left in the world; read their records from the kept world |
| `walk` | how BDS relays a player's movement to another player (needs `--observer`) | the sweep player driven by `bds-sweep:walk` (standing, turning, walking, stepping up, tilting) with the observer 8 blocks south and a cow spawned at yaw 33.3 for its `AddActor` rotation |
| `scenarios` | block states BDS stores after a scripted sequence of block sets (with or without the block update a placement triggers), waits and reads | items from `scenarios.py` families run in waves of up to 64 cells (6 blocks apart, east of the player); one wait covers a wave; touched cells are cleared without updates and the superflat layers restored after each wave |
| `scenarios` (round 3) | step kinds `build` (several sets in order), `cmd` (a console command; `{x} {y} {z}`, `{cx} {cz}` and `{player}` are filled in), `timeline` (events at given ticks, every listed position read once per tick; the trace keeps the first read and each change) with events `set`, `setblock`, `cmd`, `drop` (an item actor) and `use` (a click by the scripted player, standing where a `cmd` tp put it; such an item runs alone) | layouts per family (`scenarios.LAYOUTS`): the 8 x 8 cells above, `circuits` (22 x 6 cells, tick distance 8, a whole-cell refill after each wave, and a witness: a redstone block and dust west of the cells whose lighting tick gives each wave's redstone parity) and `fields` (16 x 16 fields, 20 apart); a `cmd` step is part of the wave signature, so waves with different game rules run apart |
| `placements` | what BDS stores and sends when the survival player clicks a block face with a stack (blocks read back, the held stack, every packet both ways), and what a second scripted player watching sees | per item: the arena box around (4,-60,4) is cleared, its blocks set with block updates, the stack put in hotbar slot 0 with `/replaceitem` (the inventory API does not reach the client), the player teleported with its look; chat markers `bds-sweep:mark <key>` / `bds-sweep:mark end` cut the client capture (`--capture-dir`, on automatically) per item |
| `item-blocks` | the block state BDS describes a stack of each item with | `/replaceitem` of every registry item into hotbar slot 0, one per two ticks behind a chat marker; slot 0 of the next main-inventory `InventoryContent` in the client capture |

```powershell
& $py tools/bds-sweep/sweep.py drops --blocks all --samples 400   # first palette state of every block
& $py tools/bds-sweep/sweep.py drops --blocks minecraft:wheat --states all
& $py tools/bds-sweep/sweep.py mining --blocks slice
& $py tools/bds-sweep/sweep.py mining --variants held --trials 1      # non-tool items and tool wear
& $py tools/bds-sweep/sweep.py mining --variants effects --mine-ticks 1200
& $py tools/bds-sweep/sweep.py attack --items spears --gap 20 --gamemode survival --client-capture
& $py tools/bds-sweep/sweep.py mining --variants capture --client-capture   # breaking packet flow
python tools/bds-sweep/script.py tools/bds-sweep/scripts/breaking.txt --out <dir>   # plain BDS, no plugin
& $py tools/bds-sweep/sweep.py armor-slots --items all
& $py tools/bds-sweep/sweep.py armor-wear
& $py tools/bds-sweep/sweep.py fuel --items all
& $py tools/bds-sweep/sweep.py scenarios --family stairs,connections,wall_above,gate_in_wall,support
& $py tools/bds-sweep/sweep.py placements      # ~24 min for 719 clicks with the watcher
& $py tools/bds-sweep/sweep.py placements --cases signs,uses   # case groups of placements.py
& $py tools/bds-sweep/sweep.py item-blocks     # ~4 min for 2076 items
& $py tools/bds-sweep/summarize.py drops <dir> [<repeat dir> ...]
& $py tools/bds-sweep/summarize.py mining <slice dir> <held dir> <effects dir>   # merged, first wins
& $py tools/bds-sweep/summarize.py scenarios <dir>
& $py tools/bds-sweep/sweep.py scenarios --family redstone_dust,redstone_repeaters   # circuits.MEASURED, ~1-2 min per 500 circuits
& $py tools/bds-sweep/sweep.py scenarios --family random_crops,random_copper          # random_ticks.FAMILIES, ~3 min per wave
& $py tools/bds-sweep/summarize.py circuits <dir> [<dir> ...]   # -> data/measured/redstone/ and redstone/blocks.json
& $py tools/bds-sweep/summarize.py random-ticks <dir>           # -> data/measured/random_ticks/
& $py tools/bds-sweep/summarize.py placements <dir>   # -> crates/unastar/src/server/game/testdata/bds-placements.json
& $py tools/bds-sweep/summarize.py item-blocks <dir>  # -> data/measured/item_blocks.json
& $py tools/bds-sweep/sweep.py knockback --items slice --client-capture   # also air, far
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
```

The `placements` job runs a second scripted player, `BDSWatch`, unless
`--no-watcher` is given: it joins once the sweep player has spawned (BDS
refuses a second login from the same address right after the first, "ip
recently connected", so it retries), stands outside the arena (or at a
case's `watcher` spot) and records its own capture
(`<out>/watcher-capture`), cut by the same chat markers. A case may follow
its click with `after` steps (`wait`, `hand` to swap a player's stack with
`/replaceitem`, another `use`, a `sign` edit by either player) and a
`read_later`; the plugin records the tick of each step (`timeline`) so the
replay test can route each captured packet at the same tick.
`summarize.py placements` writes one fixture per case group
(`crates/unastar/src/server/game/testdata/bds-placements[-<group>].json`).

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

`--client-capture` records the scripted client's packets (both directions) to
`<out>/client-capture/packets.jsonl`, outside the repository like every raw result.
The BDS process gets this checkout's `endstone-sweep/src` first on `PYTHONPATH`, so
a worktree runs its own jobs even though the shared venv's editable install points
at another checkout; the plugin logs `jobs loaded from <path>` at startup.

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
| `mining --variants held --trials 1` (2026-11-10) | 409 | 10 min |
| `mining --variants effects` (2026-11-10) | 5 | 2 min |
| `drops --blocks all --states all --samples 64` | 287,231 cells | ~50 min over 7 attempts (5 crashing states skipped, 3 handovers) |
