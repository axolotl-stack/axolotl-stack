---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Sync time with 26.x world clocks, not SetTime (WD5)

## Context and Problem Statement

Older servers drive the client clock with `SetTime`. Captures of BDS 1.26.51 showed 0 `SetTime`
against 54 `SyncWorldClocks` packets across 14 sessions.

## Considered Options

- Legacy `SetTime` loop
- 26.x world clocks (`SyncWorldClocksPacket`), as BDS does

## Decision Outcome

Chosen option: 26.x world clocks, as BDS 1.26.51 does; no legacy `SetTime` loop unless a capture
shows BDS sending it.

### Consequences

- Good, because the wire traffic matches BDS (`SyncWorldClocksPacket` verified in parity).
- Overrides the parked survival `WorldClock`, which sent `SetTime` every 200 ticks.

## More Information

Quirk row "World / time" in [docs/quirks/bds.md](../quirks/bds.md). Code: `crates/unastar/src/world/clock/`.
