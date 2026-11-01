# Chunk loading benchmarks

End-to-end measurements of how fast terrain reaches a client, from "stored or
generatable" to "on the client". Generation speed alone is measured by
`crates/unastar/examples/worldgen_bench.rs` (see `tools/worldgen-perf/`).

## Tools

| Tool | What it does |
|---|---|
| `crates/unastar/examples/load_bench.rs` | Real jolyne clients join over loopback RakNet, request a render radius, answer every `LevelChunk` with sub-chunk requests (batched per client tick, relative to the player's chunk, like a client), and record join, first chunk, render disk complete and streamed square complete. `--fly-seconds` then flies in a straight line at elytra speed (30 blocks/s) and samples how much of the render disk is missing. `--world generated` loads the same view once, stops the server with `stop` (chunks saved), restarts it and measures the second join, so terrain comes from storage. `--jump tp|respawn` then moves the whole view `--jump-distance` blocks (default 3000) with `/tp`, or with `/spawnpoint` + `/kill` and a client-like respawn, and records how the new view arrives (`LevelChunk`s and completed columns per 50 ms, header distances from the new centre). With `--world generated` the jump target is generated and saved in the first run too, so both views come from storage. |
| `crates/unastar/examples/stream_bench.rs` | Headless: stores a Java world in a temp directory, then joins a radius-12 player, respawns it 3000 blocks away, teleports it back and away again, ticking at the 50 ms cadence over a simulated link (`--link-kib-per-tick`, default 200). Prints per phase the ticks until the view is complete, tick p50/p99/max, the slowest tick's stages and the peak send queue; `--assert` fails on a tick over 50 ms or a queue over the 1 MiB session limit. |
| `tools/perf/load_matrix.sh` | Runs the standard matrix (radius 8/12/16, fresh and generated, flight, 1 and 8 players) for one server executable into one JSON file. |
| `tools/perf/load_summary.py` | Markdown table of one or more result files (times in ms from the RakNet connection; several players: the slowest). |
| `tools/perf/bds_reference.py` | The same client against BDS 1.26.51 on port 19140 (never 19132), fresh world then restarted on the stored world; always restores `server.properties`. Needs the native lock. |

Build:

```text
cargo build --release -p unastar --features java-worldgen,bedrock-worldgen --bin unastar --example load_bench
target/release/examples/load_bench --server-exe target/release/unastar.exe \
    --generator java --radius 12 --players 1 --world generated --json out.json
```

Options: `--generator java|bedrock`, `--radius N`, `--players N`,
`--world fresh|generated`, `--fly-seconds S`, `--fly-speed B`,
`--generation-threads N` (0 = automatic), `--compression-level 1..9` (server
deflate level; omitted = the server's default), `--label`, `--json` (appends
to the file), or `--addr host:port` for an already running server.

Server logs: `LOAD_BENCH_SERVER_LOG=<filter>` with
`LOAD_BENCH_SERVER_STDOUT=<file>` keeps the launched server's output. The
filter `warn,unastar::stream=info` adds the server's per-second streaming
summary: storage reads (`read`, `missed`), generation (`generated`, request to
column), background network encoding (`encoded`, `reencoded` for edited
chunks), `LevelChunk`s sent and sub-chunk requests answered with the tick
time they took, the acknowledgement delay (`acked`, `LevelChunk` to its
answered `SubChunkRequest`), each as a count and a mean in ms, and the largest
transport queue seen at a tick (`peak_queue_kib`). A tick over 50 ms logs
`SLOW TICK detected` with its slowest stages (`slowest_stages_ms`).

Timing runs need the native lock
(`bash C:/Users/jvigu/orca/stab-coord/native-lock.sh acquire <lane> <s> <purpose>`).

## How the server streams (what the numbers measure)

See `crates/unastar/src/world/ecs/streaming.rs`. In short: each player's view
is ordered nearest first with look-ahead in the direction of travel; columns
not in memory are read from storage on the async runtime (up to 256 reads in
flight, outside the generation pool); a storage miss goes straight to the
generation pool when one of its slots (three per worker) is free, else the
tick requests it in priority order; every loaded column is encoded for the
network once on a background thread and the encoding is shared by every
player and every sub-chunk answer (an edited chunk is re-encoded in the
background when next needed, and answered once that finished); unloaded
chunks are written in the background. The tick never reads,
generates or encodes: it hands work to the workers and spends at most 4 ms
each spawning finished columns, restoring their actors and unloading.
Sending is not capped per tick; two bounds apply per player. Columns: a send
window of columns sent but not yet answered, which starts at 128, grows with
prompt acknowledgements (up to 256) and halves when they come late. Bytes: a
sub-chunk request is answered column by column only while the bytes the
player's RakNet session still holds, plus answers queued in the last 45 ms,
stay within 512 KiB (half the session's 1 MiB limit, past which RakNet
disconnects with `QueueTooLong`); the rest of the request waits for the next
tick, and no new columns are sent while the queue is at the budget. The view
is the disk `dx² + dz² <= (r + 1)²` around the player's chunk, the column
count BDS streams. Sealed batches stay under 256 KiB and
sub-chunk answers are split between columns at 128 KiB, because a RakNet peer
drops messages of more than 512 split parts, which stalls the ordered channel
for good.

## Results

Result files are in `results/` (one JSON array of runs per file).

### 2026-10-31: respawn and teleport into a stored view (QueueTooLong fix)

Files: `2026-10-31-jump-java.json` (loopback), `2026-10-31-stream-bench.json`
(headless), `2026-10-31-bds-jump.json` (BDS).

Release builds, Java world, seed 12345, radius 12, native lock held.
`mainline3` is the playtest build that kicked the player (ec487d44),
`paced` the fix. Loopback `load_bench --world generated --jump ...`
(3000 blocks; ms from the command):

| build | jump | first chunk | view complete | most `LevelChunk`s in 50 ms | slow ticks | kicked |
|---|---|---|---|---|---|---|
| mainline3 | respawn | 158 | 453 | 256 | 89, 103 ms | no |
| mainline3 | tp | 78 | 331 | 256 | 97, 91, 60 ms | in 1 of 2 runs (`QueueTooLong`) |
| paced | respawn | 169 | 625 | 164 | none | no |
| paced | tp | 105 | 671 | 256 | none | no |

The paced build completes about 72 columns per 50 ms on loopback (512 KiB of
answers). The old build queued every requested answer at once: 256 dense
columns are about 2.5 MiB, past the session's 1 MiB limit whenever RakNet
had not sent them yet.

Headless `stream_bench --link-kib-per-tick 200` (a 4 MB/s link), tick times
in ms; `before` is the same benchmark on the server before the fix
(8bbfd47b):

| phase | before p50/p99/max | before peak queue | paced p50/p99/max | paced peak queue | paced ticks to view |
|---|---|---|---|---|---|
| join (storage) | 4.0 / 6.0 / 6.0 | 2881 KiB | 1.0 / 6.4 / 6.4 | 433 KiB | 18 |
| respawn 3000 away (storage) | 3.0 / 16.1 / 16.1 | 2555 KiB | 1.2 / 10.6 / 10.6 | 423 KiB | 16 |
| tp back (in memory) | 2.3 / 2.4 / 2.4 | 5020 KiB | 1.0 / 1.5 / 1.5 | 430 KiB | 17 |
| tp away (storage) | 4.1 / 6.9 / 6.9 | 2554 KiB | 1.1 / 13.3 / 13.3 | 422 KiB | 16 |

Normal play (`tick_bench`, 1200 ticks): one player p50 0.09 ms, p99 0.21 ms;
with 50 cows and 50 zombies nearby p50 0.35 ms, p99 0.65 ms.

BDS 1.26.51 on the same client (fresh world, the jump target generated on
demand): it sends the new view nearest first, in bursts of 20 `LevelChunk`s
(20 to 40 per 50 ms). `/tp` at radius 4: first chunk 630 ms, view 892 ms;
radius 6: 1018 ms and 1451 ms. Respawn at radius 4: 460 ms and 1088 ms. Our
order is the same (nearest first); we send more per tick, bounded by the
byte budget instead of a fixed count. Joins to BDS on a restarted (stored)
world failed intermittently in this session (the client is never placed), so
there is no stored-world BDS number for the jump.

### 2026-10-29: streaming rewrite (`2026-10-29-stream-java.json`)

Java generator, seed 12345, loopback, release builds, native lock held.
`base` is the server just before the rewrite (6e538c8b), `stream2` the
current one (two runs each). Times are ms from the RakNet connection; "after
first chunk" is the render disk time minus the first chunk's arrival. Other
lanes' builds ran on the machine meanwhile, so joins vary by ±300 ms between
runs; compare ranges, not single runs.

Stored world (`--world generated`), whole render disk:

| radius | players | base | stream2 | stream2 after first chunk |
|---|---|---|---|---|
| 4 | 1 | 737 | 569 | 115 |
| 6 | 1 | 407 | 617 | 138 |
| 8 | 1 | never completes | 737-748 | 237-267 |
| 12 | 1 | never completes | 855-866 | 368-421 |
| 16 | 1 | never completes | 1083-1132 | 508-689 |
| 12 | 8 | never completes | 968-980 | 532-588 |

`base` never completes a stored view of radius 8 or more with this client:
the stored chunks went out 32 per tick into one batch per tick, a batch
passed the 512 split parts a RakNet peer reassembles, the client dropped it
and the ordered channel stalled (stuck at 144 columns). Measured earlier with
one sub-chunk request per column (`2026-10-29-order-java.json`), `base` took
1143 ms to the radius-12 disk, 587 ms after the first chunk.

Fresh world, whole render disk (generation-bound):

| radius | players | base | stream2 |
|---|---|---|---|
| 8 | 1 | 1534-1559 | 1167-1297 |
| 12 | 1 | 2272-2416 | 1870-2002 |
| 16 | 1 | 3628-3955 | 2800-2943 |

Flying at 30 blocks/s for 20 s into fresh terrain, radius 12 (share of the
render disk missing, mean / worst; time to complete it after stopping):

| players | base | stream2 |
|---|---|---|
| 1 | 0.1% / 0.2% / 0 | 0.0% / 0.0% / 0 |
| 8, eight directions (`2026-10-29-flight-ab-java.json`, alternating runs) | 7.1-10.7% / 47-54% / 5.2-5.7 s | 7.1-11.5% / 48-55% / 5.9-6.8 s |

Eight players flying apart need about 400 new chunks/s, more than the
generation pool makes on this machine (about 270/s while serving players), so
that case is bound by generation in both builds.

Server-side stages (`unastar::stream` summary, radius 12): stored read
1-3 ms per column on the blocking pool; network encoding 0.06 ms per column
on the blocking pool; generation 130-250 ms from request to column (pool
queueing); tick time 0.003 ms per `LevelChunk` sent and 0.2 ms per sub-chunk
request answered (about 25 columns per request); acknowledgement delay about
100 ms (one client tick of batching plus one server tick). Outgoing deflate
level 4 instead of 7 brought the stored radius-12 disk from 840 to 806 ms
and the streamed square (streamed then) from about 500 to 390 ms after the
first chunk.

Steady-state tick cost is unchanged or lower (`tick_bench`, 400 ticks, mean
ms; base from `2026-10-29-tick-bench.jsonl`): 1 player 1.00 -> 0.38;
1 player with 50 cows and 50 zombies 2.21 -> 1.26; 8 players 1.67 -> 0.81;
8 players with 500 cows and 500 zombies 12.76 -> 14.78 (mob-bound, within run
to run variance).

BDS 1.26.51 reference (`2026-10-29-bds-reference-r4-r6.json`, same client,
port 19140): radius 4-6 views complete 1.3-1.6 s after the connection, joins
take 1.1-1.3 s. unastar completes them in 0.57-0.62 s. BDS stops answering
this client's sub-chunk requests at radius 8 and more
(`2026-10-29-bds-reference-r8-incomplete.json`), so larger BDS views and BDS
flight are not measured.
