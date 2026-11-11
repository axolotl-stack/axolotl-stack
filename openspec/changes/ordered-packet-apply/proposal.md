# Proposal: apply client packets in BDS's order

## Why

PvP has to come out exactly as on BDS: whoever hits first, with which sprint, fall and facing
state, and what the victim receives when. unastar routes each player's packets into per-domain
queues drained by separate systems, so within a tick packets are applied grouped by domain.
[Decision 0036](../../../docs/decisions/0036-run-a-command-before-its-players-later-inputs.md)
fixed one pair (a command before its player's first input) and named what it left out.

The BDS 1.26.51 race captures (`tools/bds-sweep/pvp_race.py`, three scripted players in one
process) show that BDS does not apply packets in arrival order either. It runs a network pass
that applies commands, chat and swings at once, queues movement inputs and holds attacks and
item uses for the player's next input. Then it runs a player pass that consumes the queued
inputs. Every result in the PvP race matrix follows from that order. Arrival order, and
today's domain grouping, each get some of them wrong. The measured order is in
[design.md](design.md#measured-bds-order); the copied oddities are in
[docs/quirks/bds.md](../../../docs/quirks/bds.md).

Prior work: decision 0036 and
[per-domain-queues-lose-a-players-packet-order.md](../../../docs/solutions/server/per-domain-queues-lose-a-players-packet-order.md)
(the command-order capture, which this order also explains);
[bds-holds-attacks-until-the-next-input.md](../../../docs/solutions/server/bds-holds-attacks-until-the-next-input.md)
(this measurement); `openspec/specs/server-authority` (packets reach state only through domain
queues) and `openspec/specs/plugin-host` (events dispatched in buffer order, block intents
committed after dispatch). No earlier decision covers order across players.

## What Changes

- A per-player packet-order queue (`server/game/packet_order.rs`, prototyped here, not wired):
  each packet is applied on arrival, queued as a movement input, or held for the next input.
- One exclusive system replaces the domain systems' packet draining. It runs the network pass
  over the tick's routed packets in arrival order per session, then the player pass over every
  player in a fixed order, and calls each domain's handler as a function over `&mut World` at
  the moment its packet applies. Systems that do not consume client packets stay parallel.
- Held attacks resolve before their input's flags and movement, through the PvP lane's
  single-hit function (decision 0046), so a hit sees the state BDS uses.
- `CommandOrder` and `run_early_commands` go: a command applies in the network pass, which is
  before every input of its tick, as on BDS.
- The order across players in a pass and the per-tick inbound budget follow the measurement
  (design.md).
- Outbound timing (a hit's sound one tick after its hurt, actor data two ticks after) is
  recorded, not changed here; see Out of scope.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `server-authority`: how client packets reach game state (ordered passes, not per-domain
  queues), and the order and determinism guarantees.

## Impact

- Crates: `unastar` (server/game routing and packet systems, network inbound queues, schedule);
  `bds-sweep` (`order_probe`) and `tools/bds-sweep` (captures) as evidence tooling.
- Plugins: event order in the `EventBuffer` becomes packet-apply order. The plugin-host
  contract (buffer order, intents committed after dispatch) is unchanged.
- Performance: the packet stage becomes one exclusive system. Today it is already almost all
  exclusive systems; see design.md for the benchmark.

### Parity rows

No behaviour changes in this proposal's commit: it adds evidence, a model and a design. When
the change is built, it touches the ordering side of `systems: player.combat` (owner
`entities`, PvP lane 0046) and `systems: player.movement`. Those rows stay with their owners;
this change adds tests to them instead of claiming them.

## Out of scope

- Combat numbers: damage, knockback, hurt window, crits and reach are the PvP lane's
  (decision 0046). This change only decides when a hit is applied and with which state.
- Outbound batching and the one- and two-tick lag of a hit's consequences (sound, actor data).
  These are measured and recorded here (design.md, quirks) but belong to a separate change on
  the send side.
- Item release, use on a block, block actions inside `PlayerAuthInput`, and other packet
  kinds not measured here. They keep their current timing until measured, as listed in
  design.md.
