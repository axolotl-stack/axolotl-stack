---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Generate pack-derived artefacts at build time; commit only manifests (D13)

## Context and Problem Statement

Data derived from bedrock-samples and BDS falls under the Minecraft EULA; committed repository data
must be CC0, Apache-2.0 or MIT. Earlier rule P1 committed golden KDL output.

## Considered Options

- Commit generated pack data (golden files)
- Generate it at build time and commit only manifests (hashes, counts)

## Decision Outcome

Chosen option: artefacts derived from bedrock-samples are generated at build time, not committed;
only manifests are committed. The Java worldgen data (`crates/unastar_noise/worldgen_data`) stays
committed so default builds work without a local jar.

### Consequences

- Good, because the repository stays redistributable.
- Bad, because some regressions show up as manifest hash changes rather than readable diffs.
- Modifies P1 for pack-derived families.

## More Information

`crates/unastar-data/output/pack-manifest.json`. Origin: O6.
