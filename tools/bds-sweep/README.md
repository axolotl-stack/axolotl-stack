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
| `player_script.py` | Plain-BDS scripted player captures: a timed script of console commands and client orders with every client's packets recorded (see [Movement](#movement)) |
| `player_fixture.py` | Distils `player_script.py` captures into a value-only test fixture: per phase the subject's inputs, the packets one client got about the subject, and the subject's attributes before the phase |
| `sweep.py` | T2: one sweep job under Endstone with `bds-sweep-client` as the player; output in `<runtime>/sweeps/1.26.51/<kind>-<UTC>/` |
| `endstone-sweep/` | The Endstone Python plugin that runs jobs server-side (inert unless `BDS_SWEEP_JOB` is set) |
| `summarize.py` | Raw sweep results to `crates/unastar-data/data/measured/` + `manifest.json` |
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

The plugin sends these chat orders to `bds-sweep-client` (plain BDS scripts
send the same text with `tellraw <player> {"rawtext":[{"text":"bds-sweep:..."}]}`):

| Order | Behaviour |
|---|---|
| `bds-sweep:mine X Y Z FACE PREDICT` | Start/continue breaking; optional predicted break after `PREDICT` ticks |
| `bds-sweep:stop` | Abort the current mining action |
| `bds-sweep:attack RUNTIME_ID` | Attack using the synchronized held stack |
| `bds-sweep:drop hotbar SLOT COUNT` | Closed-screen Drop from hotbar index 0..8; count 1 is Q, a full stack is ctrl-Q |
| `bds-sweep:drop inventory SLOT COUNT` | Open the inventory, wait for its server-allocated window ID, then Drop from native inventory index 0..35 |
| `bds-sweep:close-inventory` | Close the tracked native inventory window |
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
| `mining` | break ticks per (block state, tool, effect/environment), 3 trials | player (survival) at (0.5,-60,0.5) facing the west face of (1,-59,0); client sends Start/ContinueDestroyBlock, BDS breaks the block itself; ticks from the LEFT_CLICK_BLOCK event to the BlockBreakEvent (±1 tick) |
| `armor-slots` | `/replaceitem` accept per slot (head, chest, legs, feet, offhand) | read back with Endstone `PlayerInventory` |
| `armor-values` | health lost per `/damage <player> D entity_attack` wearing one item | max health 2000, hits repeated for sub-point precision; needs the full `armor-slots` result for slot assignment |
| `attack` | melee damage per held item, warm-up + 3 trials | a fresh pig at (2,-60,0.5); the client sends an InventoryTransaction attack; Endstone `ActorDamageEvent` value |
| `fuel` | furnace burn ticks per item | pool of 32 furnaces, 1 cobblestone input + the item as fuel |

```powershell
& $py tools/bds-sweep/sweep.py drops --blocks all --samples 400   # first palette state of every block
& $py tools/bds-sweep/sweep.py drops --blocks minecraft:wheat --states all
& $py tools/bds-sweep/sweep.py mining --blocks slice
& $py tools/bds-sweep/sweep.py armor-slots --items all
& $py tools/bds-sweep/sweep.py fuel --items all
& $py tools/bds-sweep/summarize.py drops <dir> [<repeat dir> ...]
```

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
