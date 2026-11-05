---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Ingest Bedrock-style feature data from a local BDS install (WD1)

## Context and Problem Statement

The Bedrock-style generator needs BDS's features and feature rules, which ship only inside BDS
(Minecraft EULA).

## Considered Options

- Use Java `placed_feature` / `configured_feature` data for both generators
- Ingest BDS `definitions/` and versioned `.brarchive` files from a local install for Bedrock-style

## Decision Outcome

Chosen option: ingest from the shared BDS install for the Bedrock-style generator; Java placed and
configured features drive the Java-style generator only. It is read via `UNASTAR_BDS_DIR` behind the
off-by-default `bedrock-worldgen` feature.

### Consequences

- Good, because Bedrock decoration comes from Bedrock's own data.
- Bad, because the Bedrock generator needs a local BDS install to build (0024).
