---
title: Multi-client BDS captures — what online players see, and the limits of the harness
area: tooling
tags: [bds-sweep, captures, multiplayer, slots, player-list]
date: 2026-11-11
origin: OpenSpec change protocol-join-roster
status: current
---

## Context

A single `bds-extractor` session only shows what BDS sends the joining player. What players
already online receive when someone joins or leaves (player list, sleeping counts, colours) needs
several clients at once.

## What we learned

- `capture.py --pair FILE` runs a host that executes FILE while guests join and leave. The host's
  `command-outputs.json` attributes every packet to the filler command running at the time, so a
  script of `/list` lines works as a clock; `--guest-gap` keeps guests online together.
- `base_properties` caps `max-players` at 2. More guests are refused with `Serverfull`; `--pair`
  raises the cap to the number of players.
- Each debug `bds-extractor` needs a few hundred MB. With about 20 at once the machine ran out of
  memory and five guests died within one second (`os error 8`), which BDS reports as ordinary
  leaves. Keep runs to about 12 overlapping guests, or account for the early leaves in the
  analysis (`roster/twenty-one-players`, README there).
- A full `PlayerList` from BDS is not in a stable order, so fixtures that compare it must match
  entries, not positions, unless the run has only two players.
- The generated `PlayerList` codec puts the add/remove marker on every entry; it round-trips the
  captured 1- to 21-entry packets byte for byte, so build packets with it instead of hand-writing the
  layout.
- Slot locks: never chain `rm -rf <slot>/agent.lock` after a command that may run without the
  lock. On 2026-11-11 a probe refused to start on a slot another agent had locked, and the cleanup
  after it deleted that agent's lock. Take the lock with `if mkdir <slot>/agent.lock; then ...; fi`
  and release it only inside that branch.

## Where it lives

`tools/bds-sweep/capture.py` (`pair`), `roster_probe.py`, `commit_roster_fixtures.py`;
fixtures in `crates/bds-extractor/captures/1.26.51/roster/`; tests in
`crates/unastar/src/server/game/roster/tests.rs`.
