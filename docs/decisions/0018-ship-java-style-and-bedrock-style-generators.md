---
status: accepted
date: 2026-10-24
decision-makers: [project owner]
---

# Ship two overworld generators: Java-style and Bedrock-style (D7)

## Context and Problem Statement

World research §2.4 measured that BDS terrain behaves like Java's noise router, but Bedrock's
seeding, biomes, surface builders, features and structures differ. Earlier plans targeted Java byte
parity only; the world research reframed the truth as BDS.

## Considered Options

- Java parity only
- Bedrock parity only
- Two generators sharing one core, chosen by world config

## Decision Outcome

Chosen option: two generators. **Java-style**: Java Edition parity, the `unastar_noise` design
finished to 100% (terrain, biomes, surface rules, carvers, features, structures). **Bedrock-style**:
BDS 1.26.51 parity, the shared Java noise core with Bedrock seeding and biomes where they differ,
plus Bedrock surface builders, feature rules, features, carvers and structures from the BDS install.
Both share one generator trait, chunk pipeline, AOT toolchain and caches; the world config picks one.
Finishing the Java-style generator comes first.

### Consequences

- Good, because each generator has a precise, testable target (0019).
- Bad, because two pipelines must be maintained; Bedrock data cannot be committed (0024).
- Supersedes the Java byte-parity-only target of the 2025-12-29 parity plan.

## More Information

`world.generator.kind` in `crates/unastar/src/config/mod.rs`; `crates/unastar/src/world/generator/bedrock/`.
Progress: `docs/parity/worldgen.toml`.
