---
status: superseded by 0017
date: 2026-10-24
decision-makers: [project owner]
---

# Decode worldgen data at startup (WD3)

## Context and Problem Statement

The world research applied the gameplay-data rule (0012) to worldgen: decode surface, feature and
structure data at startup instead of generating per-entry code.

## Considered Options

- Decode at startup
- Compile to straight-line Rust at build time

## Decision Outcome

Chosen option at the time: decode at startup. Superseded the same day by 0017: worldgen data is AOT
compiled end to end; only a later addon/WASM overlay may be interpreted. Spawn rules (gameplay) keep 0012.
