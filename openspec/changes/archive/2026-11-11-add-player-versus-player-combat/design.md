# Design: player-versus-player melee like BDS

## Measurement

Three `bds-sweep-client` players in one process (`--also`), driven tick by tick by the sweep
plugin (`sweep.py pvp`): 710 trials over three runs (`numbers`, `reach`, `hurt` three times,
`reach` twice with strict entity interactions, `extra` three times). Timing cannot place a packet
in a chosen server tick (BDS handles packets late in the tick, ticks are irregular, it sometimes
consumes two queued inputs of a player at once), so every tick line turns each client 0.01
degrees and the sampled yaw says which input BDS consumed; Endstone records packets in and out,
damage and knockback events on one sequence counter. `pvp_fixture.py` distils each trial into the
attacks in the tick and order BDS applied them, with both players' state at that moment.

## Decisions

1. **One hit function with the attacker's state passed in** (decision 0046, proposed). Options:
   read the world after the tick's inputs; pass a `Swing` taken from before the attacker's first
   input of the tick; ship nothing until decision 0045's ordered schedule. Chosen: the snapshot,
   because it matches BDS for the order vanilla clients send (attack, then input) and the ordered
   path can call the same function with the state at consumption.
2. **The hurt window counts from the arrival tick, passed in.** BDS ignored a hit applied 10 ticks
   after the previous one exactly when its packet had arrived a tick early. unastar applies an
   attack in its arrival tick today, so the caller passes the current tick; the ordered path will
   pass the arrival tick.
3. **The victim's velocity is the one its client reported** (`PlayerInput::velocity`, from
   `pos_delta`, or a push since). BDS keeps half its own simulation's velocity; a vanilla client
   reports the same, and unastar has no server-side player simulation (the movement lane's work).
   Options: start every push from rest (the old code: wrong under netherite and in the air), the
   report, a server simulation (out of scope). Chosen: the report.
4. **Victim-side changes apply to every player hurt, not only PvP** (excess hit pushes and
   restarts the window, armour before the window, motion to viewers): BDS's hurt rules belong to
   the victim. The mob knockback replication rows already showed motion going to every viewer.
5. **Replay tests feed BDS's state** (positions, the velocity BDS had, the order of hits) to the
   hit function; packet-path tests cover what the current schedule reproduces; ignored tests name
   what waits for 0045.

## Risks

- A mob's hit on a player now also pushes on an excess hit and subtracts after armour; measured on
  player attackers only.
- Players who joined before this change have `InputStart` only after their next spawn (inserted at
  spawn); the hit falls back to the current state.
