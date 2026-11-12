# Silverfish goals on BDS 1.26.51 (first measurements)

What `sweep.py silverfish` showed (entities round 4, raw runs `ent4-silverfish-merge-normal` and
`ent4-silverfish-wake-normal`, normal difficulty, not yet distilled into a fixture). The silverfish
is not spawnable yet; this is the starting point for `behavior.silverfish_merge_with_stone` and
`behavior.silverfish_wake_up_friends`.

## Merging into stone (`--items merge`, 10 runs)

A silverfish with no target in a 3 × 3 pit dug into stone always merged, 12 to 177 ticks after it
spawned (12, 13, 18, 22, 35, 40 while still on its spawn block; 80 to 177 after it had walked to a
corner). One `UpdateBlock` turned a stone block into infested stone in the same batch as the
silverfish's `RemoveActor`. The block was always a neighbour of the block at its feet: the floor
below while it stood in the middle of the pit, the floor or a wall from a corner, never the block
above. So the goal picks a direction from the feet block and merges when that neighbour is
stone; how often it draws, and whether it strolls in between, needs a run in a 1 × 1 pit (five
stone neighbours, nowhere to walk) with about 20 repeats.

## Waking friends (`--items wake`, 3 runs)

A frozen silverfish among infested stone (12 blocks along +x, 6 along +z, 5 above) took the
protected player as its target and turned angry (`mob.warning` one batch later). It was hurt by
the player on tick 40. From tick 60, and every 20 ticks after, infested blocks broke: a
`LevelEvent` 2001 (block destroy, data the infested block's network id) at the block's centre,
the new silverfish's `AddActor` one tick later at the block's corner minus 0.2 along x
(`(x − 0.2, y, z)`, its spawn sound at `(x, y + 0.255, z)`), sometimes without an `UpdateBlock`
for the broken block. Several blocks break on the same tick, nearest first, and the released
silverfish (angry through `broadcast_anger`) break more on later 20-tick steps: by tick 300 the
blocks 12 along +x and 5 above had broken in two of the three runs. The order is random between
runs. Releasing a silverfish when an infested block breaks is also what players' block breaking
lacks (`docs/parity/blocks.toml`, the infested rows).

## Prior art

The unmerged branch `Jviguy/stab-ent-m5` (`503bf672`) has a `components/silverfish.rs` that
models the merge goal as a random stroll that may instead merge; check it against these runs
before reusing it.
