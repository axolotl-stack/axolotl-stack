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

Clarified 2026-11-05 (review of PR #65):

- **Allowed:** names of files and schemas Mojang publishes as data or documentation, e.g. a
  bedrock-samples schema file such as `FilterSubject.json`, a vanilla pack path, or a packet or
  field name from the public protocol docs.
- **Not allowed:** a scope-qualified engine name (`Outer::Inner`) or an engine-style function name
  (`doThing`). These are banned even when the name has leaked into a public changelog. Describe the
  thing instead ("the enchantment id enum in Mojang's public bedrock-protocol-docs").
- **Examples in tests and docs** use made-up names.

### Consequences

- Good, because every claim is reproducible from outside.
- Bad, because some behaviour takes more measurement to pin down.

### Confirmation

`parity check` rejects obvious patterns; `review-change` checks every diff; rule 2 in `AGENTS.md`.
