# Design: the silverfish, and the creeper's fuse sensor

## Infested blocks are measured data

BDS keeps the host ↔ infested pairing in code. The pairing is measured
(`sweep.py silverfish --items blocks`: a silverfish in 1 × 1 pits of 14 kinds of block) into
`crates/unastar-data/data/measured/infested_blocks.json` and resolved once against the block
registry (`InfestedBlocks::vanilla`), failing with a typed error on an unknown name. The
alternative, deriving the infested name by prefixing `infested_` (the prior art's table and
string building), is what the code-shape rules forbid, and cobbled deepslate, which a prefix
rule would accept, is not a host on BDS. States are carried by property, so deepslate's axis
survives a merge and a wake (decision 0059).

## The silverfish goals

The merge goal draws one roll per tick it may start (no target, no path, mobGriefing on),
then one of the six faces of the feet block; a host block on that face is merged into,
otherwise the goal strolls with the published `behavior.random_stroll` defaults (the goal
authors none of them). The wake-up goal starts once the mob was hurt by an actor and scans
every 20 ticks from then on, in the measured order, stopping after each block on a fair coin.
The world work (block changes, the actor event, spawning the released silverfish) runs after
the AI's resource scopes, like the other goals' deferred work, now grouped in one `Deferred`
struct instead of five parallel buffers.

## The target-nearby sensor

The sensor is edge-triggered: an event fires when the target moves into another zone, never
while it stays. Ranges are measured between the two collision boxes. The forced exploding
groups author an empty sensor to switch it off; an empty trigger compiles to no event. The
swell goal runs while the mob is `IGNITED`, so it needs no handle on the explode component
blocks round 4 adds: the creeper becomes whole when both changes are in.

## Risks

- The merge roll is fitted from 1 × 1 pits measured on a quiet machine; runs taken while the
  machine was compiling waited about twice as long (see the research note), so the fit is
  provisional.
- The sensor's gap was measured on level ground only.
