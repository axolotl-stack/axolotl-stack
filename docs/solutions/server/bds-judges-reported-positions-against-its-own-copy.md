---
title: BDS judges reported positions against its own copy of the player; watch that copy through a second client
area: server
tags: [movement, authority, correction, teleport, player-auth-input, bds-sweep, viewer, fixture]
date: 2026-11-11
origin: openspec/changes/server-authoritative-movement
status: current
---

## Context

To copy BDS 1.26.51's movement authority, unastar had to know when BDS corrects a client, and
what BDS does when it doesn't. The first guess was "correct when a reported position is more than
`player-position-acceptance-threshold` (0.5) off". Nearly every rule turned out to be something
else. The client being judged cannot see most of what BDS does.

## What we learned

- **Corrections show only a fraction of what happens.** Within the threshold, BDS moves its copy
  instead of correcting. It takes the client's position when every axis is within 0.1, or adds a
  pull of at most 0.005 to the copy's velocity. The judged client sees none of this. A second
  client nearby sees BDS's copy after every input in the `MovePlayer` relays.
- **Probe with a client that misreports on purpose.** The sweep client's `offset` and `drift`
  orders shift what it reports, and `corrections ignore` keeps the offset after a correction. A
  viewer client records the copy throughout. Constant offsets of 0.09, 0.11, 0.2, 0.4 (diagonal),
  0.45, 0.55 and 1.0, followed and ignored, pinned the whole cadence:
  - the threshold compares the squared distance, so 0.55 is not corrected;
  - the pull is 0.005 along the gap;
  - the eleventh disagreement is corrected;
  - after a correction, six inputs pass with no pull;
  - threshold corrections come at most every sixth input.

  The rules are in `docs/quirks/bds.md` (Entities / player movement authority, pull).
- **Fit the model offline before coding it.** A Python model replayed against the viewer series
  reproduced every input of the constant-offset and teleport runs exactly before any Rust was
  written. Drifts (0.05 and 0.095 a tick) and hovering 0.09 up never fit: BDS takes about every
  other input there. They are pinned as known gaps, not tuned around.
- **Teleports are not acknowledged.** Neither `HandledTeleport` nor anything else in an input
  acknowledges a teleport. BDS does not simulate the next input: its copy rests at the
  destination, and the input is corrected there if it is far off. A short teleport (under about
  0.7 blocks) can be pulled back by a client that ignores it. A respawn is not a teleport: the
  next input is simulated from the spawn point.
- **Every server-side move must reach the copy.** Commands, plugins, ender pearls, chorus fruit,
  respawns and knockback (as velocity) all have to update it. So do test harness helpers that set
  `Position` directly: a stale copy corrects the client back on the next input.
- **Positions on the wire are not server positions.** Clients report the feet plus 1.62001.
  Converting with the server's 1.62 puts the copy 1e-5 inside the floor. Collision ignores boxes
  the body already overlaps, so the copy falls through.
- **Make the zero-corrections property a test.** Replaying a capture is not enough: assert that
  the server sends exactly BDS's corrections (same inputs, position and velocity) and none that
  BDS did not send. Near a block the simulation does not cover yet, trust the client. Allow a
  missed correction only within 20 inputs of a trusted one.
- **A replay covers only what was captured.** The first version judged every situation the
  probes never visited. That meant correcting players eating while walking, sprinting at low
  food, in waterlogged blocks or currents, on ice, with depth strider, or crawling. List what
  vanilla play does and trust each case until a run covers it.
- **A trusted input must not move the copy before the reach check.** The first version did,
  which let a trusted input move the copy 1000 blocks; the next input was then in sync there.
  A trusted client still sets state: it must leave the copy's carried state (sprint speed,
  jump delay) consistent, or the first judged input after it is corrected.
- **Trust only what the server can check.** A survival client's `StartFlying`, or a spin without
  a riptide trident in hand, would otherwise make the client trusted.

## Where it lives

`crates/unastar-rules/src/movement/authority.rs`, `crates/unastar/src/entity/player_life/authority.rs`,
`crates/unastar/src/entity/player_life/tests/authority.rs`,
`crates/unastar/src/entity/player_life/testdata/bds-1.26.51-authority.json`,
`tools/bds-sweep/authority_fixture.py`, the probe orders in `crates/bds-sweep/src/main.rs`.
