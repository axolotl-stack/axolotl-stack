# Design: the slime and the magma cube

## The hop control replaces path following

A mob with `minecraft:movement.jump` never paths. Its goals write two things per tick into a
`HopState`: the yaw a facing goal wants (`slime_random_direction`, `slime_attack`) and the
speed multiplier a moving goal asks for (`slime_keep_on_jumping`, `slime_float`). The control
then turns the body by at most `max_turn`, and on the ground counts the hop delay down and
jumps; in the air it keeps the speed. The travel step is the ordinary living one: BDS's hop
matched the jump impulse, ground acceleration on the jump tick and air acceleration after it
to the thousandth of a block. The alternative, a jump flag on the path follower, would have
moved slimes between hops and turned them in the air, which BDS does not do.

The delays are the authored seconds cut to whole ticks per draw: BDS's calm slimes waited 10
to 29 ground ticks (`[0.5, 1.5]` s) and magma cubes 43 to 119 (`[2, 6]` s).

## Splitting is a native class trait

BDS splits only the slime and magma cube classes, so `NativeTraits::slime` marks them. The
child's size group is found in the definition (the group whose `minecraft:variant` is half
the parent's) instead of a hand-written name table.

## Slime chunks

The chunk test is a 32-bit Mersenne Twister seeded by the chunk coordinates alone. The
generator already existed for the Bedrock world generator behind a feature; it moves to
`world::bedrock_random` so spawning can use it in every build (the generator keeps its
`rng` path through a re-export).

## Risks

- The slime-chunk height bound is not measured (the sweep's floor is far below it).
- Contact damage cadence comes from four attack runs.
