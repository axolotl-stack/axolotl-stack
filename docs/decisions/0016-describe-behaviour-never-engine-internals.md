---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Describe observed behaviour; never name BDS or client internals

## Context and Problem Statement

Matching BDS exactly invites working from its internals. Committed text that names them creates
legal and provenance risk for the project.

## Considered Options

- Allow internal names where they help explain behaviour
- Black-box only: describe behaviour and cite captures, public Mojang documentation, public data
  sets or open-source code

## Decision Outcome

Chosen option: black-box only. Nothing committed names the internal implementation of BDS or the
Bedrock client: no class, function or symbol names, addresses or memory layouts. Repository text
describes behaviour and cites observed captures, public Mojang documentation, public data sets or
open-source code.

### Consequences

- Good, because every claim is reproducible from outside.
- Bad, because some behaviour takes more measurement to pin down.

### Confirmation

`parity check` rejects obvious patterns; `review-change` checks every diff; rule 2 in `AGENTS.md`.
