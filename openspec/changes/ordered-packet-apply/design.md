# Design: apply client packets in BDS's order

## Measured BDS order

All measurements are black-box. Three scripted players run in one process
(`crates/bds-sweep/src/bin/order_probe.rs`) against plain BDS 1.26.51 on a flat survival
world, driven by `tools/bds-sweep/pvp_race.py`. One task sends for all three players, so the
order of their packets within a 50 ms client tick, and which packets share a game batch, are
the script's. Every case was repeated: 10 trials per variant unless noted, and 3 per step for
crits and the reach sweep. `tools/bds-sweep/pvp_race_summary.py` groups the trials and prints
each variant's outcome distribution. Trials during which BDS stalled for more than 150 ms are
excluded and marked. Captures cut into fixtures: `crates/unastar/src/server/game/testdata/pvp-race-*.jsonl`.
The first line of each fixture holds every variant's outcome distribution.

The main observable is the input tick BDS stamps on `SetActorMotion`, `UpdateAttributes`,
`SetActorData` and movement corrections. It is the stamped player's last applied
`PlayerAuthInput`, so it tells which inputs had been applied when a hit resolved. Two more
signals: whether health changed, and the client tick at which each packet arrived.

### One server update

```
network pass:  for each connection, its packets oldest first
                 command, chat, swing, open inventory, ...   -> apply now
                 PlayerAuthInput                              -> queue for the player
                 attack, item use in the air                  -> hold until the player's next input,
                                                                 then queue with it
player pass:   for each player, every queued input oldest first
                 held actions   -> resolve with the player's state from BEFORE this input
                 the input      -> flags (sprint, sneak, ...), rotation, movement
flush:         one batch per connection
```

| What was sent (A attacks B) | BDS result | Evidence |
|---|---|---|
| attack, then A silent for 5 ticks | hurt, knockback and health at +7, after A's next input at +6 (10/10) | `pvp-race-queue` `attack-no-input` |
| `[attack, input]` in one batch / `[input]` then `[attack]` / attack then next tick's input | hit at +1 / +2 / +2: an attack joins the first input after it | `queue` `attack-input-batch`, `turn-then-attack`, `attack-then-turn` |
| attack, chat, swing, A silent | chat echo and swing relayed at +1, hurt at +7 | `queue` `attack-chat-no-input`, `swing-attack-no-input` |
| attack, chat, input in one tick | chat echo, then the hurt, in the same batch (10/10) | `pvp-race-model` `attack-chat-input` |
| attack, then `/effect resistance` from A or from B | no health lost, in both orders: the command is in place for the hit | `queue` `attack-then-resist`, `attack-then-victim-resist` |
| snowball thrown, A silent | chat at +1, snowball spawned at +9, after A's next input (10/10) | `model` `use-no-input` |
| `Interact` open inventory, A silent | `ContainerOpen` at +1 while A sends nothing (10/10): applied on arrival | `model` `interact-no-input` |
| stop-sprint input, then attack (separate or one batch) | plain knockback 0.4/0.4 | `pvp-race-sprint` |
| attack, then stop-sprint input (separate or one batch) | sprint knockback 0.6/0.32 | `sprint` |
| start-sprint input, then attack / attack, then start-sprint input | sprint / plain | `sprint` |
| sprint held, a second hit 12 ticks later | sprint knockback again (0.606): BDS does not reset sprint on a hit | `sprint` `two-sprint-hits` |
| A dropped from 1.5 blocks; attack k ticks later, that tick's input before or after the attack | crit for k=2..6 when the input comes first, k=3..7 when it comes after (3/3 each): crit is judged before the following input | `pvp-race-crit` |
| strict entity interactions on: attack facing B, then an input turning away | hit (10/10) | `pvp-race-model-strict` |
| strict on: facing away, attack, then an input facing B | miss (10/10): the held attack uses the facing from before its input | `model-strict` |
| strict off: same facing variants | every hit lands; no facing check | `model` |
| B's input before or after A's attack in the tick | same result; knockback stamped with B's input of that tick, applied in B's next move (first correction stamped B+2) | `pvp-race-victim-input` |
| five attacks in one tick, separate or one batch, or over five ticks | one hurt, one knockback | `pvp-race-burst` |
| A silent 12 ticks, then 12 inputs at once with an attack after the 1st and the 11th | one hit (20/20): the backlog is consumed in one update, so both attacks resolve in one server tick | `pvp-race-lag` |
| second hit sent 9 / 10 / 11 ticks after the first | 0/10, 6/9, 9/9 land: a 10-server-tick window between resolutions | `pvp-race-window` |
| B teleported by a command, then A's attack in the same tick | knockback from A to B's new position: the command ran first | `pvp-race-pending` |
| B claims a position half a block off | knockback from BDS's own position for B | `pending` |
| A's transaction claims a position in reach while A is 7.75 blocks away | miss (10/10): reach uses BDS's position, not the claim | `pvp-race-reach-order` `spoofed-from` |
| A 6.75 blocks away: input claiming a 0.1 step toward B, then attack / attack, then that input | hit / miss (10/10 each): reach is checked from A's position before the input after the attack | `reach-order` `claim-then-hit`, `hit-then-claim` |
| B 6.68 blocks away: B's input claiming a 0.1 step back, then A's attack / A's attack, then B's step | miss / **miss** (10/10 each): B's step of the same tick is applied before A's attack resolves with A's next input | `reach-order` `victim-claims-leave-*` |

Combat numbers in these rows are recorded for the PvP lane (decision 0046), not decided here:
knockback 0.4/0.4 and sprint 0.6/0.32, crit damage 1.5, reach edge 6.7 to 6.8 blocks feet to
feet, and the hurt window.

### Order across players

Two players' held attacks resolving in the same player pass do **not** resolve in arrival
order:

- Both players at 1 health hit each other in the same tick, 15 trials per variant (5 in the
  later sessions), in seven sessions. Exactly one dies, and a dead player's held attack is
  dropped. Across the variants (whose attack arrived first, whose next input arrived first,
  batched or not), the winner stays the same within a session. The **second player to join**
  won in 172 of 210 trials (82%), with both join orders tried (`pvp-race-both-low`).
- The losing outcomes coincide with the next inputs arriving within about 20 ms of a server
  update. Most likely the network pass was already running when they arrived, so the later
  input waited for the next pass.
- The pair sessions (`pvp-race-pairs`, every pair of three players, four join orders) give
  the full order: see [Pair order](#pair-order).

### Pair order

Every pair of the three players, both at 1 health, hits each other in the same tick, in both
arrival orders, separate and batched (4 trials per variant, 16 per pair and session). There
were six sessions in four join orders. A trial is decided by the first death within 4 ticks.

| Join order | A–B first | A–C first | B–C first | Resolution order |
|---|---|---|---|---|
| A, B, C | B 16/16 | C 15/16 | C 13/16 | C, B, A |
| A, B, C | B 15/16 | C 15/16 | C 15/16 | C, B, A |
| C, B, A | A 11/14 | A 11/13 | B 11/13 | A, B, C |
| C, B, A | A 14/16 | A 15/16 | B 16/16 | A, B, C |
| B, C, A | A 15/16 | A 14/16 | C 15/16 | A, C, B |
| A, C, B | B 12/16 | C 15/16 | B 13/16 | B, C, A |

**Within a pass, players resolve in reverse join order: the latest joiner first.** The later
joiner went first in 251 of 280 decided trials (90%); 8 were undecided. Runtime ids do not
explain it: C was id 4 in one A, B, C session and id 7 in the other, with the same order. The
first C, B, A session also showed damage the script did not cause (later deaths, health
dropping in steps of 4). Its decided races still follow the rule. An earlier two-attackers
reading (A's knockback over C's on the same victim) does not decide order: the bystander saw only
one knockback for the two hits of that tick, so it does not show which hit resolved first.

### Output timing

Relative to the batch that carries a hit's hurt event, as seen by the bystander (10/10 per
variant, `pvp_race_summary.py` "output bursts"):

- Same batch: `ActorEvent` hurt, `SetActorMotion` and the target's health update.
- Next update: the `attack.strong` sound.
- Two updates later: the attacker's `SetActorData`.

Other lanes saw the same pattern. Entities lane (PR #89): BDS sends a player-caused
consequence one tick after its direct reply. Items lane (PR #91): a tick's block changes go
out before the next inputs are read, and unastar's `set_block` sends each change to viewers
twice. unastar sends everything in the tick that caused it. Changing that is a send-side
change, out of scope here.

### Not measured or unclear

- Sneak at hit time: BDS never reported the scripted attacker as sneaking, so no conclusion.
- Item release, use on a block, block actions inside `PlayerAuthInput`, `MobEquipment` and
  item stack requests relative to a held attack. Not measured: they keep their current timing,
  and each needs a capture before it moves.
- An attack while the attacker's own teleport is unacknowledged (the scripted client
  acknowledges at once), and the cap on queued inputs.
- Reach with movement from the move vector alone: one walking input from standstill did not
  cross the edge in either order, because the scripted player claims a stationary position.
  The claimed-step rows above settle reach by order.

## Decisions

### 1. Two ordered passes in one exclusive system (decision 0045, proposed)

Options and reasons are in
[docs/decisions/0045](../../../docs/decisions/0045-apply-client-packets-in-bds-network-and-player-passes.md).
Arrival order is refuted by the sprint, crit, resistance and cross-player rows above. Per-player
chained system sets cannot express the order across players without a global order. Keeping
0036 fails every held-attack row.

### 2. Handlers are functions over `&mut World`, called in order

Every domain system that drains a packet queue today becomes a handler function for one packet:
`fn(&mut World, Entity, Packet)`. One exclusive system, `apply_client_packets`, calls them in
pass order. One-shot systems (`SystemId`, `run_system_with`) were considered. They would keep
`Query`/`Res` parameters, but each call flushes commands and checks access, which adds work per
packet, and most handlers are already `&mut World` code (inventory, blocks, mob actions, item
use, commands). Plain functions it is, with `world.run_system_cached_with` where a handler
really wants system parameters.

### 3. Systems that do not consume client packets stay parallel

Physics, entity logic, chunks and sending keep their sets and parallel scheduling. Only
`PacketApplySet` turns into the one exclusive system (plus the chunk-request handler, which has
no ordering effect and can stay a parallel system after it).

### 4. Player order across players

Each pass visits players in reverse join order: the player who joined last goes first. This
copies the pair sessions. A player keeps their place for the session, and a player who
rejoins counts as the latest joiner. unastar's network pass sees the whole tick's packets at
once, so it always gives the measured majority. BDS gives the other result in about 10% of
races, which coincides with input that arrives during its own pass. That minority is not
copied: it depends on network timing, not on packet order.

The order lives in one place (`PlayerOrder`, a list of entities appended on join), so a
measurement that changes it changes one function.

### 5. Plugin events keep buffer order

Handlers push events to the `EventBuffer` as they run, so the buffer is in apply order. The
plugin-host contract (dispatch in buffer order, block intents committed after dispatch) does
not change. A held attack's plugin event is raised when it resolves, not when it arrives.

## Cost

PvP correctness decides the design. The benchmark only checks that the cost stays small.
`crates/unastar/src/server/game/packet_order_bench.rs` (ignored test, release build) routes one
`PlayerAuthInput` and one `MobEquipment` per player per tick and measures the median tick and
the median packet stage:

One run on the development machine (2026-11-11, release, 200 measured ticks after 40 of
warm-up, other BDS slots running on the same machine):

| Players | Schedule | Median tick (µs) | Median packet stage (µs) |
|---|---|---|---|
| 10 | today | 347 | 32 |
| 10 | serial | 99 | 5 |
| 10 | model | 340 | 31 |
| 50 | today | 689 | 57 |
| 50 | serial | 218 | 9 |
| 50 | model | 701 | 56 |
| 200 | today | 2,057 | 150 |
| 200 | serial | 1,752 | 70 |
| 200 | model | 2,312 | 173 |

- Running the whole schedule on one thread is *faster* than today's multi-threaded executor
  at every size: the packet stage takes about half the time and the tick less. The domain
  systems are short, and today's packet stage is already mostly exclusive systems, so
  thread hand-off costs more than the parallelism gains.
- The model's bookkeeping (`model`: every packet through a `PlayerQueue`, released in pass
  order) adds up to about 0.25 ms per tick at 200 players.
- So one exclusive packet system costs nothing measurable at these loads. The parallel
  executor stays for the rest of the schedule. Whether it pays off there is a separate
  question; this benchmark only covers the packet stage.

## Migration plan

The model is prototyped and tested here (`packet_order.rs`); nothing is wired. Wiring touches
the files these lanes are changing now: player movement (PlayerAuthInput), items
(InventoryTransaction), blocks, entities and protocol. So it is scheduled after they merge, in
this order:

1. **Routing records, it does not queue.** `packets.rs` `route_packet` appends
   `(session, entity, packet)` to one `ClientPacketLog` resource, in arrival order. The
   per-domain queue pushes go away.
2. **Handlers.** Turn each domain system's per-packet body into a handler. These are the files
   and systems:
   - `server/game/movement.rs` `apply_movement` (→ `apply_auth_input`, `apply_player_action`)
   - `server/game/blocks.rs` `apply_block_actions` (→ `apply_block_actions_of_input`, `apply_block_click`)
   - `server/game/inventory/mod.rs` `apply_inventory` (→ one handler per `InventoryAction`)
   - `server/game/item_use/mod.rs` `apply_item_use` and `entity/projectile/launch.rs` `apply_throws`
   - `entity/mob/interaction/actions.rs` `apply_mob_actions`, and the PvP lane's player hit
     (decision 0046)
   - `server/game/commands.rs` `apply_chat_and_commands`, `run_early_commands`, `run_commands`
     (console lines and `/schedule` stay in `run_commands`)
   - `entity/player_life/respawn.rs` `apply_respawn_requests`
   - `server/game/chunks.rs` `apply_chunk_requests` (may stay a system; no ordering effect)
3. **The passes.** `apply_client_packets` (exclusive, `PacketApplySet`) runs the network pass
   over the log with a `PlayerQueue` per player, then the player pass in the player order of
   decision 4.
4. **Schedule.** `server/game/mod.rs` registers the one system in place of the domain systems
   and drops the `before`/`after` edges between them. `packet_queues.rs` and
   `packet_domains.rs` shrink to the action enums. `consolidate_domain_events` goes, because
   handlers push to the `EventBuffer` directly.
5. **Inbound.** `network/inbound.rs` keeps its per-session budget. It visits sessions in the
   player order of decision 4 instead of rotating the first session each tick. `server/runtime.rs`
   needs no change.
6. **Remove 0036's mechanism.** Delete `CommandOrder`, `run_early_commands` and
   `applied_before_commands`. Keep `command_order_tests.rs`, which must still pass, because
   commands now apply in the network pass.
7. **Tests.** Replay every `pvp-race-*` fixture through `GameServer::route_packet`. Until step 3
   lands, these are ignored tests that point at this change.
8. **Specs and docs.** Archive into `server-authority`. Decision 0045 is accepted by the owner,
   and 0036 is marked superseded.

## Risks

- A handler that was written as a system may rely on `Commands` being applied at the end of
  its system. A handler must call `world.flush()` where it spawns or despawns.
- The order across players rests on the second-joiner result and the pair sessions. If later
  sessions disagree, decision 4 changes. The model keeps the order in one place
  (`PlayerOrder`).
- Inputs that arrive while a pass runs. BDS shows about 15% of tie outcomes flipping with
  arrival timing. unastar's network pass sees a whole tick's routed packets at once, so it
  always takes the measured majority.
