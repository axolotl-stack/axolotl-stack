# Silverfish goals on BDS 1.26.51

What `sweep.py silverfish` showed, normal difficulty. Round 4 took the first runs
(`ent4-silverfish-merge-normal`, `ent4-silverfish-wake-normal`); round 5 added 1 × 1 pits
(`pit`, `blocks`, `ab`), mobGriefing off (`griefing`, `wake-nogrief`) and distilled the runs
into `crates/unastar/src/entity/mob/ai/testdata/bds-1.26.51-silverfish.json`
(`tools/bds-sweep/silverfish_fixture.py`). unastar implements both goals from these runs
(`mob/ai/silverfish.rs`, `mob/life/infested.rs`); the replays are in
`mob/tests/silverfish.rs`.

## Merging into a block

A silverfish with no target merges into a block next to its feet block. The merge shows as
the mob's `SilverfishMergeAnim` actor event on one tick and, one tick later, an
`UpdateBlock` turning the block into its infested form in the same batch as the mob's
`RemoveActor`. The block is any of the six neighbours of the feet block that is a host: in a
1 × 1 pit with five host neighbours BDS merged into all five, never the open block above
(20 stone pits: +x 7, floor 5, -x 3, +z 3, -z 2).

Hosts (`--items blocks`, a 1 × 1 pit cut into a 3 × 3 × 4 block of one kind, three runs each):
stone, cobblestone, stone bricks, mossy, cracked and chiseled stone bricks, and deepslate
(either axis; infested deepslate keeps the axis). Never merged: cobbled deepslate, andesite,
granite, diorite, tuff, smooth stone, infested stone. The pairs are
`crates/unastar-data/data/measured/infested_blocks.json` (decision 0059). With mobGriefing off
(`griefing`, three runs of 400 ticks) a silverfish never merged.

How soon: the tick of the `SilverfishMergeAnim`, counted from the spawn tick, behaves as a
per-tick chance (several runs merged on the spawn tick). On a quiet machine the 1 × 1 pits
waited 7.0 ticks on average (`blocks`, 21 runs over 7 hosts), 8.5 and 10.4 (`ab` and `ab2`,
80 runs, stone and cobblestone interleaved, the same for both blocks): 8.9 over all 101 runs.
With the face draw (five host faces of six) the most likely roll is one in 8 per tick, one in
9 nearly as likely, one in 10 about two standard errors off (`SILVERFISH_MERGE_ROLL`). Two
earlier stone sweeps (`pit`, `pit2`, 40 runs) waited 16.3 and 14.6 ticks on average; they ran
while the machine was compiling the server, and stone interleaved with cobblestone showed no
difference between the blocks, so the load is the suspected cause
(`docs/solutions/tooling/measure-random-goals-on-a-quiet-machine-and-interleave-the-cases.md`).

In the round 4 3 × 3 pit the silverfish sometimes walked to a corner first (after 30 to 160
ticks), then merged: the goal strolls when the roll does not merge, with the published
`behavior.random_stroll` defaults.

## Waking friends

A frozen silverfish among infested stone (12 blocks along +x, 6 along +z, 5 above), hurt by
the player on tick 40 (`/damage … entity_attack entity <player>`, which also knocks it back
0.3 blocks), breaks infested blocks every 20 ticks from tick 60 for as long as the run lasts
(300 ticks). Each round scans from its feet block in the order y, x, z, each offset in the
order 0, 1, -1, 2, -2, …: first the +z column, then the +x row, then the blocks above. Blocks
11 and 12 along +x and 6 above never broke: the scan reaches 10 sideways and 5 up. After each
broken block the round stops with even odds (57 blocks over 31 rounds; 17 rounds broke one).
The silverfish it lets out do not scan: none of them was hurt.

A broken block shows its destroy particles (level event 2001 at the block centre, data the
infested block's network id); one tick later comes the `UpdateBlock` to air (bundled in
`UpdateSubChunkBlocks` when several broke) and the released silverfish's `AddActor` at the
block's corner moved 0.2 along -x, `(x - 0.2, y, z)`, its spawn sound at `(x, y + 0.255, z)`.

With mobGriefing off (`wake-nogrief`, three runs) the rounds scan the same way, but each
woken block turns back into stone (an `UpdateBlock`, no particles) and no silverfish comes out.

## Prior art

The unmerged branch `Jviguy/stab-ent-m5` (`503bf672`) modelled the merge goal as a random
stroll that may instead merge (kept: it matches the 3 × 3 runs) and named the infested blocks
in a hand-written table that also released a persistent silverfish at the block's corner (not
kept: the table is measured, and BDS puts the silverfish 0.2 off the corner).
