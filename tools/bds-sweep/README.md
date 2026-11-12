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
| `native.py` | Readers for protocolgen's CC0 native data (block palette, mining classes, items, item tags) the jobs are built from |
| `scenarios.py` | Block scenario families for the `scenarios` job (stairs, connections, wall_above, gate_in_wall, support, surfaces, soils, junctions, halves; round 2: stairs_blocks, connections_blocks, wall_heights_blocks, multiface_support, rails, soils_more, coral_death) |
| `circuits.py` | Round 3: redstone circuit families for the `scenarios` job (`redstone_*`, `MEASURED`; the `circuits_pilot*` families are the pilots): a circuit built in a 22 x 6 cell, a timeline of inputs at known ticks and a read of every position every tick |
| `random_ticks.py` | Round 3: random tick families (`random_*`): 16 x 16 fields of crops, sugar cane, cacti, copper, leaves and farmland at a set `randomtickspeed`, read every 100 ticks |
| `coral.py` | Round 3: coral death families (`coral_water`, `coral_timing`; round 2's `coral_death` also runs on this layout): cells 16 blocks apart so the water a cell brings reaches no other, coral set, `/setblock`ed, clicked into place or losing its water, read every 10 ticks or every tick |
| `pistons.py` | Round 4: piston families for the `scenarios` job on the circuit layout (`pistons_*`): push lines, every block in front of a sticky piston, slime and honey, power, timing, chains and actors (the timeline's actor box) |
| `tnt.py` | Round 4: TNT families on their own layout (`tnt.LAYOUT`, cells 16 apart, a 13 x 10 x 13 box refilled): priming and the primed actor's path, 64 blasts per material in a 9 x 9 x 9 cube (`tnt_blast_*`), and mobs held still at known spots (`tnt_hits`) |
| `placements.py` | The survival clicks of the `placements` job in case groups (`GROUPS`: round 1's `blocks`, then signs, door hinges, uses, coral, skulls, directions, mushrooms, bells, multi-face blocks, candles, crops, plants and stacks, block actors, follow-up; round 3's `leftovers`: flower pot plants, cave vine ages, cake slices, string): blocks set around the click, the stack in hand, the player's feet and look, `before` commands, the click, follow-up steps, the positions read back |
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
| `bds-sweep:drop hotbar SLOT COUNT` | Closed-screen Drop from hotbar index 0..8; count 1 is Q, a full stack is ctrl-Q |
| `bds-sweep:drop inventory SLOT COUNT` | Open the inventory, wait for its server-allocated window ID, then Drop from native inventory index 0..35 |
| `bds-sweep:close-inventory` | Close the tracked native inventory window |
| `bds-sweep:use X Y Z FACE CX CY CZ TARGET` | Click block face `FACE` of `X Y Z` with the selected hotbar stack (an `ItemUse` place transaction; click position `C*`/1000 inside the block; `TARGET` the clicked block's runtime id), as a survival player's right click |
| `bds-sweep:sign X Y Z FRONT HEX` | Edit the sign at `X Y Z`: the block-actor data the server last sent for it, with the `Text` of the front (`FRONT` 1) or back side replaced by the UTF-8 text `HEX` (`-` for empty), sent as `BlockActorData` the way a client submits an edit |

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
| `scenarios` | block states BDS stores after a scripted sequence of block sets (with or without the block update a placement triggers), waits and reads | items from `scenarios.py` families run in waves of up to 64 cells (6 blocks apart, east of the player); one wait covers a wave; touched cells are cleared without updates and the superflat layers restored after each wave |
| `scenarios` (round 3) | step kinds `build` (several sets in order), `cmd` (a console command; `{x} {y} {z}`, `{cx} {cz}` and `{player}` are filled in), `timeline` (events at given ticks, every listed position read once per tick; the trace keeps the first read and each change); round 4: an optional actor box records the actors in the cell every tick (`actor_traces`) with events `set`, `setblock`, `cmd`, `drop` (an item actor) and `use` (a click by the scripted player, standing where a `cmd` tp put it; such an item runs alone) | layouts per family (`scenarios.LAYOUTS`): the 8 x 8 cells above, `circuits` (22 x 6 cells, tick distance 8, a whole-cell refill after each wave, and a witness: a redstone block and dust west of the cells whose lighting tick gives each wave's redstone parity) and `fields` (16 x 16 fields, 20 apart); a `cmd` step is part of the wave signature, so waves with different game rules run apart |
| `placements` | what BDS stores and sends when the survival player clicks a block face with a stack (blocks read back, the held stack, every packet both ways), and what a second scripted player watching sees | per item: the arena box around (4,-60,4) is cleared, its blocks set with block updates, the stack put in hotbar slot 0 with `/replaceitem` (the inventory API does not reach the client), the player teleported with its look; chat markers `bds-sweep:mark <key>` / `bds-sweep:mark end` cut the client capture (`--capture-dir`, on automatically) per item |
| `item-blocks` | the block state BDS describes a stack of each item with | `/replaceitem` of every registry item into hotbar slot 0, one per two ticks behind a chat marker; slot 0 of the next main-inventory `InventoryContent` in the client capture |

```powershell
& $py tools/bds-sweep/sweep.py drops --blocks all --samples 400   # first palette state of every block
& $py tools/bds-sweep/sweep.py drops --blocks minecraft:wheat --states all
& $py tools/bds-sweep/sweep.py mining --blocks slice
& $py tools/bds-sweep/sweep.py armor-slots --items all
& $py tools/bds-sweep/sweep.py fuel --items all
& $py tools/bds-sweep/sweep.py scenarios --family stairs,connections,wall_above,gate_in_wall,support
& $py tools/bds-sweep/sweep.py placements      # ~24 min for 719 clicks with the watcher
& $py tools/bds-sweep/sweep.py placements --cases signs,uses   # case groups of placements.py
& $py tools/bds-sweep/sweep.py item-blocks     # ~4 min for 2076 items
& $py tools/bds-sweep/summarize.py drops <dir> [<repeat dir> ...]
& $py tools/bds-sweep/summarize.py scenarios <dir>
& $py tools/bds-sweep/sweep.py scenarios --family redstone_dust,redstone_repeaters   # circuits.MEASURED, ~1-2 min per 500 circuits
& $py tools/bds-sweep/sweep.py scenarios --family random_crops,random_copper          # random_ticks.FAMILIES, ~3 min per wave
& $py tools/bds-sweep/summarize.py circuits <dir> [<dir> ...]   # -> data/measured/redstone/ and redstone/blocks.json
& $py tools/bds-sweep/summarize.py random-ticks <dir>           # -> data/measured/random_ticks/
& $py tools/bds-sweep/sweep.py scenarios --family pistons_push,pistons_slime,pistons_power,pistons_timing,pistons_chains,pistons_entities   # ~1 min
& $py tools/bds-sweep/sweep.py scenarios --family pistons_blocks   # 1448 blocks, ~3 min
& $py tools/bds-sweep/sweep.py scenarios --family tnt_prime,tnt_actors,tnt_hits   # ~2 min
& $py tools/bds-sweep/sweep.py scenarios --family tnt_blast_dirt,tnt_blast_oak_planks,tnt_blast_cobblestone,tnt_blast_glass   # ~2 min
& $py tools/bds-sweep/summarize.py round4 <dir> [<dir> ...]     # -> data/measured/pistons/, explosions/ and pistons/blocks.json
& $py tools/bds-sweep/summarize.py placements <dir>   # -> crates/unastar/src/server/game/testdata/bds-placements.json
& $py tools/bds-sweep/summarize.py item-blocks <dir>  # -> data/measured/item_blocks.json
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
