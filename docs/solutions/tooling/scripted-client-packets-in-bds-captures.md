---
title: Scripted client packets and custom logins in BDS captures
area: tooling
tags: [bds-sweep, bds-extractor, captures, skins, multiplayer, evidence]
date: 2026-11-11
origin: OpenSpec change protocol-client-relays
status: current
---

## Context

Many packets reach BDS only from a real client: interactions, emotes,
book edits, persona skins, a claimed blob cache. Commands cannot produce
them, so the first captures only showed what BDS sends on its own.

## What we learned

- `bds-extractor --commands` scripts take `!` lines that send client packets
  (`crates/bds-extractor/src/actions.rs`); `command-outputs.json` attributes
  server packets to them like commands. `peer` and `actor` targets are the
  last `AddPlayer`/`AddActor` the session saw. `--client-data FILE` replaces
  login ClientData fields (a persona skin), `--blob-cache` claims a chunk
  cache.
- A packet BDS refuses ends the session with a `PacketViolationWarning`
  and a close. The extractor now writes its outputs up to that point, but
  the rest of the script is lost: run risky packets last or one per session
  (`protocol_probe.py skins` and `ddui` start one short session per case on
  one server).
- To claim "BDS sends nothing back", commit the session's `timeline.json`
  (`commit_protocol_fixtures.py`): every packet name after the join, in
  order. A test then checks that only ambient traffic (actor movement,
  chunk and block updates, clock syncs) follows the client packet. Quiet the
  world first (`domobspawning`, `randomtickspeed` 0, no daylight or
  weather cycle, `kill @e[type=!player]`); otherwise spawns flood the window.
- Changes made by the server console show what the target and the others
  receive. `protocol_probe.py` drives the console from Python (`Bds.command`)
  on a timeline after `Player Spawned:` appears in the BDS log, and stores
  wall-clock times (`console.json`, `started_unix` in `session.json`) to line
  the sessions up.
- `/op` and `/deop` from an operator player fail with a permissions error.
  Only the console can run them.
- Without `--input` the extractor sends no `PlayerAuthInput`. BDS still
  relays its emotes, swings and picks. `BookEdit` changed nothing in these
  runs because the book was never opened (an item use on air), not for want
  of input: see
  [driving-a-live-client-in-bds-captures.md](driving-a-live-client-in-bds-captures.md).
- A persona with one unknown piece type (`persona_hands` is one) refuses the
  whole login, so skin variants with unknown values go in separate sessions.

## Where it lives

`crates/bds-extractor/src/actions.rs`, `tools/bds-sweep/protocol_probe.py`,
`tools/bds-sweep/commit_protocol_fixtures.py`; fixtures and the run table in
`crates/bds-extractor/captures/1.26.51/protocol/`; related:
[multi-client-bds-captures.md](multi-client-bds-captures.md).
