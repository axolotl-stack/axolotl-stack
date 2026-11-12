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
| `script.py` | Plays a `bds-sweep-client --script` scenario against plain BDS (no Endstone) and records the client's packets; the same script runs against unastar with `bds-sweep-client --addr <unastar> --script ...` |
| `scripts/breaking.txt` | Survival block breaking scenarios: by hand, tools, instant, abort, a frozen client, Haste, a block above, client predictions on time and early, `StopDestroyBlock`, blocks put back where a break goes on, a burst of inputs |
| `breaking_fixture.py` | Cuts the committed breaking fixtures (`crates/unastar/src/server/game/blocks/testdata/bds-breaking*.jsonl`) out of a `script.py` capture of `scripts/breaking.txt` (or other windows, `--windows`), with a second player's packets moved onto the first player's clock (`--observer`) |
| `scripts/breaking-2.txt`, `scripts/breaking-observer.txt` | Breaking seen by a second player (run with `script.py --observer scripts/breaking-observer.txt`), creative breaking (a start, a start and prediction in one input, a sword) and a `CreativeDestroyBlock` action, which ends the session |
| `scripts/breaking-3.txt` | Other blocks put where a break goes on: obsidian or coarse dirt on the air of a dirt break, a block swapped and a tool selected mid-break, a released break, obsidian and dirt on the air of obsidian broken with a diamond pickaxe |
| `scripts/creative-break-items.txt` | A creative start on stone with each sword, the trident, the mace, each spear and other items held: which break nothing |
| `scripts/spears.txt` | Spear scenarios with named pigs: plain hits and the attack cooldown (its last input, its category, three hits in one input), jabs without a target for every tier (also packed into the input), missed swings, holding a spear, a sword's reach |
| `scripts/enchant-spears.txt` | `/enchant` with every enchantment (and Lunge III/IV) on a fresh spear of every tier |
| `scripts/use-items.txt` | The spyglass, goat horns 0-7, potions 0-46 and ominous bottles 0-4 drunk to the end, and an elytra (see `scripts/elytra.txt`) |
| `scripts/records.txt` | All 22 music discs into a jukebox and out, a disc on a playing jukebox, creative, breaking a playing jukebox, disc 11 to its end |
| `scripts/elytra.txt`, `scripts/elytra-2.txt`, `scripts/elytra-3.txt` | Gliding with and without an elytra; potions drunk facing south, north and down (where the drink sound plays); which jump inputs start a glide; seven glides with the elytra's damage read back (an item use makes BDS resend the inventory, armour included) |
| `scripts/ore-xp.txt`, `scripts/ore-xp-2.txt` | Experience from breaking ores, sculk blocks and the spawner: 30 breaks each with an Efficiency V netherite pickaxe, Silk Touch, then the hoe, a wooden pickaxe and the hand |
| `experience_table.py` | Experience orbs per break of an `ore-xp` capture (`bds-sweep-client --decode ... --only AddActor` piped in) as the committed table `crates/unastar/src/server/game/block_drops/testdata/bds-block-experience*.json` |
| `scripts/foods.txt`, `scripts/foods-2.txt`, `food_script.py` | Every vanilla food eaten once (suspicious stew per data value) at easy difficulty, each after a Hunger drain sized so its nutrition fits; then the chance foods again, dried kelp and the Saturation effect at food 10. `food_script.py` prints them |
| `food_table.py` | Per food of a `foods` capture: the food level, saturation and exhaustion before and after the eat, the effects added and removed, `CompletedUsingItem`, slot 0 and sounds, as the committed tables `crates/unastar/src/server/game/item_use/testdata/bds-foods*.json` |
| `scripts/containers-flint.txt` | Every container block placed by a survival click (which BDS announces with actor data), then flint and steel on an unlit candle, netherrack, an unlit campfire and stone |
| `scripts/throwables.txt` | Snowballs, eggs, ender pearls (and their cooldown), experience bottles, wind charges, splash and lingering potions thrown ahead and straight down, survival and creative |
| `scripts/throwables-2.txt`, `scripts/throwables-2-observer.txt` | Poison splashed at the thrower's feet with a second player (`--observer`) 1 to 4.5 blocks away, water splashed beside fire, then every potion data value 0-46 |
| `scripts/buckets-eggs.txt` | Water, lava and powder snow buckets placed and taken (a one-block pit keeps liquids still), water into a slab, milk, the fish buckets, creative; cow, pig, chicken and zombie spawn eggs (peaceful), a side face, creative, the air and a monster spawner |
| `scripts/give.txt`, `give_script.py`, `give_table.py` | `/give` of every registry item to an empty inventory; `give_table.py` keeps the slot BDS resent for each (`crates/unastar/src/command/exec/testdata/bds-give.json`) |
| `combat_fixture.py` | Cuts replay fixtures out of a `script.py` capture: the client's inputs and actions with BDS's hurt, health, cooldown, sound and flag packets inside scenario windows (`crates/unastar/src/entity/mob/combat/hit/testdata/bds-spears.jsonl`), or the requests of one command and their outputs (`--commands enchant`, `crates/unastar/src/item/testdata/bds-enchant-spears.jsonl`) |
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
| `bds-sweep:pause TICKS` | Send no `PlayerAuthInput` for that many ticks (a frozen client), then carry on |
| `bds-sweep:burst INPUTS` | Send that many more `PlayerAuthInput`s right after the next one, at once (a client running faster than real time) |
| `bds-sweep:release` | Release the button: `StopDestroyBlock` with the default position, and no more mining |
| `bds-sweep:attack RUNTIME_ID` | Attack using the synchronized held stack |
| `bds-sweep:drop hotbar SLOT COUNT` | Closed-screen Drop from hotbar index 0..8; count 1 is Q, a full stack is ctrl-Q |
| `bds-sweep:drop inventory SLOT COUNT` | Open the inventory, wait for its server-allocated window ID, then Drop from native inventory index 0..35 |
| `bds-sweep:close-inventory` | Close the tracked native inventory window |
| `bds-sweep:attack-named NAME` | Attack the actor whose `AddActor` carried the name tag `NAME` (`/summon ... <nameTag>`) |
| `bds-sweep:jab` / `jab-input` | Item use `UseAsAttack` with the held item, on its own or packed into the next `PlayerAuthInput` (`PerformItemInteraction`) |
| `bds-sweep:swing` / `animate` / `missed-swing` | An arm swing that hits nothing: `Animate` plus the `MissedSwing` input flag, or one of the two |
| `bds-sweep:use` / `use-on X Y Z FACE` | Item use `Use` (click air; the next input flagged `StartUsingItem`) or `Place` on a block face (with the block's runtime id as last sent) |
| `bds-sweep:release-item` / `consume` | Item release `Release` / `Use` |
| `bds-sweep:select SLOT` | `MobEquipment` selecting hotbar index 0..8 |
| `bds-sweep:glide` / `unglide`, `sprint` / `unsprint` | The input flags that start or stop gliding or sprinting (`Sprinting` on every input while sprinting) |
| `bds-sweep:look YAW PITCH` | The rotation of the next inputs |
| `bds-sweep:move DX DY DZ TICKS` | Move by `(DX, DY, DZ)` on each of the next `TICKS` inputs, forward held |
| `bds-sweep:action NAME X Y Z FACE` | One raw block action with the next input (`start`, `continue`, `crack`, `predict`, `abort`, `stop`, `creative-destroy`); several on one tick share the input |

`bds-sweep-client --script FILE` plays timed orders without the plugin: one `TICK ORDER` line per
step at that client input tick, an order above without its `bds-sweep:` prefix, a slash command
(sent as `CommandRequest`; the player must be an operator) or `exit`. Every input carries the
camera orientation of its rotation, as a vanilla client sends it.

`flags A,B` and `held-flags A,B` send raw input flags by name (`Jumpdown`, `Startgliding`, ...)
on the next input or on every input until `held-flags none`. With `script.py --observer SCRIPT` a
second player, `BDSObserver`, plays its own script, recorded to `DIR/observer-capture`. It joins
first, and the player follows a few seconds after it spawned, because BDS refuses a second login
from one address right away ("ip recently connected").

`bds-sweep-client --decode DIR/packets.jsonl [--only ActorEvent,LevelSoundEvent]` prints a
recording decoded, one packet per line with the client tick of the latest input, without
connecting.

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
| `mining` | break ticks per (block state, tool, effect/environment), 3 trials (`--trials` for the held and effect sets), and the held stack's durability damage after every break | player (survival) at (0.5,-60,0.5) facing the west face of (1,-59,0); client sends Start/ContinueDestroyBlock, BDS breaks the block itself; ticks from the LEFT_CLICK_BLOCK event to the BlockBreakEvent (±1 tick). `--variants tools` (default): the tool slice; `held`: `--items` held on `--blocks` (default every non-tool item the parity catalog gives a `digger` facet, on dirt, netherrack and oak leaves, plus every tool on dirt and netherrack for its wear); `effects`: Haste I-III by hand on stone, Mining Fatigue I-II by hand on dirt |
| `armor-slots` | `/replaceitem` accept per slot (head, chest, legs, feet, offhand) | read back with Endstone `PlayerInventory` |
| `armor-values` | health lost per `/damage <player> D entity_attack` wearing one item | max health 2000, hits repeated for sub-point precision; needs the full `armor-slots` result for slot assignment |
| `armor-wear` | durability one `/damage <player> D entity_attack` hit costs a worn piece, per D (1, 3, 4, 5, 7, 8, 12, 16, 40) | one breakable piece in its `armor_slots.json` slot, read back through Endstone before and after every hit |
| `attack` | melee damage per held item, warm-up + 3 trials, and the held stack's durability damage after every trial | a fresh pig at (2,-60,0.5); the client sends an InventoryTransaction attack; Endstone `ActorDamageEvent` value. `--gamemode survival` lets hits wear the item; `--items spears` adds every spear tier, `--gap` ticks apart |
| `fuel` | furnace burn ticks per item | pool of 32 furnaces, 1 cobblestone input + the item as fuel |

```powershell
& $py tools/bds-sweep/sweep.py drops --blocks all --samples 400   # first palette state of every block
& $py tools/bds-sweep/sweep.py drops --blocks minecraft:wheat --states all
& $py tools/bds-sweep/sweep.py mining --blocks slice
& $py tools/bds-sweep/sweep.py mining --variants held --trials 1      # non-tool items and tool wear
& $py tools/bds-sweep/sweep.py mining --variants effects --mine-ticks 1200
& $py tools/bds-sweep/sweep.py attack --items spears --gap 20 --gamemode survival --client-capture
& $py tools/bds-sweep/sweep.py mining --variants capture --client-capture   # breaking packet flow
python tools/bds-sweep/script.py tools/bds-sweep/scripts/breaking.txt --out <dir>   # plain BDS, no plugin
python tools/bds-sweep/script.py tools/bds-sweep/scripts/breaking-2.txt --observer tools/bds-sweep/scripts/breaking-observer.txt --out <dir>
python tools/bds-sweep/breaking_fixture.py <dir>/client-capture/packets.jsonl <fixture> --windows 80-110,120-130,130-150,150-190,200-220,230-280,290-300,310-330,350-370,380-391,400-420,430-450,460-480 --observer <dir>/observer-capture/packets.jsonl
python tools/bds-sweep/script.py tools/bds-sweep/scripts/spears.txt --out <dir>
python tools/bds-sweep/combat_fixture.py <dir>/client-capture/packets.jsonl <fixture> --windows 40-160,170-275,280-520,528-588,588-628,628-670
& $py tools/bds-sweep/sweep.py armor-slots --items all
& $py tools/bds-sweep/sweep.py armor-wear
& $py tools/bds-sweep/sweep.py fuel --items all
& $py tools/bds-sweep/summarize.py drops <dir> [<repeat dir> ...]
& $py tools/bds-sweep/summarize.py mining <slice dir> <held dir> <effects dir>   # merged, first wins
```

`--client-capture` records the scripted client's packets (both directions) to
`<out>/client-capture/packets.jsonl`, outside the repository like every raw result.

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
