---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Copy BDS behaviour exactly, quirks included, and log every quirk (D3)

## Context and Problem Statement

BDS 1.26.51 is sometimes more permissive than other servers (placing into a furnace result slot is
allowed; merges skip per-slot rules) and sometimes stricter (request ids that are not negative-odd
get no response; each screen allows only its own containers; `item_lock`). An implementer has to
choose which to follow.

## Considered Options

- Copy BDS exactly
- Be stricter than BDS where that is safe

## Decision Outcome

Chosen option: copy BDS exactly, including permissive or quirky behaviour. Every odd or edge-case
behaviour we copy is recorded in [docs/quirks/bds.md](../quirks/bds.md) (what BDS does, how it was
observed, why it looks odd). They are refined later, one decision at a time.

### Consequences

- Good, because clients and tools behave exactly as they do against BDS.
- Bad, because some permissive behaviour is reproduced on purpose; the quirks log is mandatory so
  each one can be revisited (`log-quirk` skill).
- Supersedes the premise "behaviour-pack JSON is a patch on code defaults" (BDS replaces whole files
  by identifier) and the "vanilla-ish baseline" goal of 0007's source document.

### Confirmation

`review-change` checks that copied oddities have a quirks row.
