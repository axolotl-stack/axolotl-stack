---
name: bedrock-research
description: Research how vanilla Minecraft Bedrock (BDS 1.26.51) or Java Edition worldgen actually behaves before implementing it — protocol fields, gameplay rules, data values, timings. Use when the answer isn't already in docs/parity, docs/quirks, docs/research, docs/solutions or the specs, when sources disagree, or when designing a feature that consumes vanilla data.
---

# Research vanilla behaviour

## Check the repo first

```sh
rg -n -i '<topic>' docs/quirks docs/solutions docs/decisions docs/research openspec/specs
rg -n '<id>' docs/parity/*.toml
```

## Source order (most to least authoritative)

1. **Measured BDS 1.26.51 behaviour**: our captures and sweeps (`crates/bds-extractor`,
   `tools/bds-sweep`, `tools/client-diff`, `crates/unastar-data/data/measured/`, test fixtures).
   What each tool measures and needs: `docs/guides/tools.md`.
   If the question matters and no measurement exists, propose one rather than guessing.
2. **Data BDS ships**: protocolgen's pinned native inputs and manifest
   (`crates/valentine_gen/protocolgen/`), the vanilla behaviour packs, Mojang's published
   protocol docs and changelog.
3. **Open-source implementations** as hints, never as proof: gophertunnel / dragonfly (Go),
   CloudburstMC / Nukkit (Java), PrismarineJS `minecraft-data`. They are often behind or wrong
   for 1.26.51; confirm against 1 or 2.
4. **Wikis and forum posts**: leads only. Built-in values (armour, tool tiers, fuels, drops) are
   measured, not copied from wiki tables.

For the Java-style generator, the reference is Java Edition 1.21.11: its public worldgen data
(`crates/unastar_noise/worldgen_data`), reference worlds and open-source ports such as cubiomes.

## Provenance rule

Black-box only. Never use, quote or write down Bedrock server/client internal class, function or
symbol names, addresses or offsets, and don't recommend disassembly. Describe behaviour as observed.

## Output

Say what the behaviour is, which tier of source established it (with paths/links), what is still
uncertain, and how to measure it if needed. Then route what you found:
odd-but-copied behaviour → `log-quirk`; a lesson → `capture-solution`; long background →
`docs/research/<YYYY-MM-DD>-<topic>.md`.
