---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Port BDS surface builders for Bedrock-style; keep Java surface rules for Java-style (WD2)

## Context and Problem Statement

Bedrock surfaces are produced by surface builders parameterised by biome JSON, not by Java's
surface-rule trees.

## Considered Options

- Use Java surface rules for both generators
- Port the BDS surface builders for Bedrock-style, decided per builder by fixtures

## Decision Outcome

Chosen option: port the BDS surface builders, parameterised by biome JSON, after biomes are right;
the fixture decides each builder. Java-style keeps Java surface rules with seeding fixed.

### Consequences

- Good, because each generator matches its own truth source (0019).
- Bad, because surface work is gated on biome correctness first.
