# BDS extraction lane (T1 captures, T2 sweeps)

Measures BDS 1.26.51 on loopback so unastar can copy it (decisions D3/D4):
packet captures with `bds-extractor` (T1) and built-in facts with an Endstone
sweep harness (T2). Everything here needs the **exclusive** shared BDS slot
(`C:/Users/jvigu/orca/bds-runtime`): announce long runs to the coordinator and
the impl workers first. Never launches the Minecraft client.

| File | Role |
|---|---|
| `bds_runtime.py` | Starts/stops the shared BDS (plain or under Endstone): refuses when a `bedrock_server` runs or the port is bound, backs up and restores `server.properties`, harness-owned world names only, graceful `stop` |
| `capture.py` | T1: two `bds-extractor --capture-dir` sessions on plain BDS (member join; operator join + `/summon` of every summonable identifier) into `<runtime>/captures/1.26.51/<UTC>/` |
| `commit_fixtures.py` | Copies the committed subset of a capture into `crates/bds-extractor/captures/1.26.51/` (see the crate README) |
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

## Jobs

| Kind | What one item measures | Arena / method |
|---|---|---|
| `drops` | `samples` x `/loot spawn … mine … mainhand` per (block state, tool) | block at (3,-60,0) set without physics; tools: hand, every tier of each correct digger, one wrong digger, shears, silk touch and fortune I-III on the diamond correct tool |
| `mining` | break ticks per (block state, tool, effect/environment), 3 trials | player (survival) at (0.5,-60,0.5) facing the west face of (1,-59,0); client sends Start/ContinueDestroyBlock, BDS breaks the block itself; ticks from the LEFT_CLICK_BLOCK event to the BlockBreakEvent (±1 tick) |
| `armor-slots` | `/replaceitem` accept per slot (head, chest, legs, feet, offhand) | read back with Endstone `PlayerInventory` |
| `armor-values` | health lost per `/damage <player> D entity_attack` wearing one item | max health 2000, hits repeated for sub-point precision; needs the full `armor-slots` result for slot assignment |
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
`server.properties`.

## Runtimes observed (2026-10-25, turbo boost off; compare only like with like)

| Run | Items | Time |
|---|---|---|
| `capture.py` (both sessions, 112 summons) | 2 sessions | ~4 min |
| `drops` slice, 10 blocks x 7-14 tools x 200 samples | 126 | 6-15 s of sweep work (~4,000 samples/s), ~40 s with BDS start/stop |
| `armor-slots` slice | 10 | 17 s |
| `fuel` slice (lava bucket burns 20,000 ticks) | 8 | 17 min |
| `mining`, 2 blocks x 17 variants x 3 trials | 34 | 2.6 min |

Repeatability: two independent `drops` slice runs agreed on all 89
single-outcome cells and stayed inside the sampling bound on all 37 random
cells (`summarize.py compare-drops`; recorded in the measured manifest).

## Licensing

Captures are Mojang-derived and stay in the untracked runtime cache except
for the small committed fixtures and manifests (D13). Sweep outputs are
measurements of BDS behaviour; only the summarised data goes into
`crates/unastar-data/data/measured/`, raw logs stay in the runtime cache.
