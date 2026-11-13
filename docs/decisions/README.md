# Decisions

Why the project is built the way it is. One [MADR 4](https://adr.github.io/madr/) record per
decision; copy [template.md](template.md) or use the `record-decision` skill. Records are never
deleted: a changed decision gets a new record, and the old one's status becomes
`superseded by NNNN`.

Old ids (D1–D13, WD1–WD5, entities Q1–Q5) come from the 2026-10-24 decisions and research
documents and still appear in code comments and research docs; the table maps them.

| # | Decision | Date | Status | Old id |
|---|---|---|---|---|
| [0000](0000-record-decisions-as-madr.md) | Record decisions as MADR files in docs/decisions | 2026-11-04 | accepted | |
| [0001](0001-compile-density-functions-to-rust-at-build-time.md) | Compile density functions to Rust at build time | 2024-12-30 | accepted | |
| [0002](0002-generate-worldgen-code-in-build-script.md) | Generate worldgen code in build.rs, not committed | 2024-12-30 | accepted | |
| [0003](0003-drive-java-worldgen-from-pinned-data.md) | Drive Java-style worldgen from the pinned Java data | 2025-01-02 | accepted | |
| [0004](0004-compile-surface-rules-with-build-time-block-states.md) | Compile surface rules with build-time block states | 2025-01-02 | accepted | |
| [0005](0005-build-biome-source-from-java-parameter-points.md) | Biome source from Java's parameter points and R-tree | 2025-01-02 | accepted | |
| [0006](0006-generate-protocol-take-game-data-from-bds-sources.md) | Generate protocol; game data from BDS-native sources | 2025-12-27 | accepted | |
| [0007](0007-unastar-is-a-server-product-with-wasm-plugins.md) | Unastar is a server product with WASM-first plugins | 2025-12-26 | accepted | |
| [0008](0008-blocks-are-data-rows-with-hashed-network-ids.md) | Blocks are data rows with hashed network ids | 2026-01-04 | accepted | |
| [0009](0009-plugin-calls-are-synchronous-and-time-bounded.md) | Plugin calls are synchronous and time-bounded | 2026-02-12 | accepted | |
| [0010](0010-game-state-in-ecs-resources-plugins-outside-the-world.md) | Game state in ECS resources; plugins outside the World | 2026-02-13 | accepted | |
| [0011](0011-pin-vanilla-packs-to-bedrock-samples-1-26-50.md) | Pin vanilla packs to bedrock-samples 1.26.50+ | 2026-10-24 | accepted | D1 |
| [0012](0012-codegen-schemas-decode-vanilla-data-at-startup.md) | Codegen schemas; decode vanilla data at startup | 2026-10-24 | accepted | D2 |
| [0013](0013-copy-bds-behaviour-exactly.md) | Copy BDS behaviour exactly, quirks included | 2026-10-24 | accepted | D3 |
| [0014](0014-measure-built-in-facts-from-bds.md) | Measure built-in facts from BDS, not wikis | 2026-10-24 | accepted | D4 |
| [0015](0015-vanilla-scope-with-extensible-registries.md) | Vanilla scope with extensible registries | 2026-10-24 | accepted | D5 |
| [0016](0016-describe-behaviour-never-engine-internals.md) | Describe behaviour; never name engine internals | 2026-10-24 | accepted | evidence rule |
| [0017](0017-compile-worldgen-ahead-of-time-end-to-end.md) | Compile worldgen ahead of time, end to end | 2026-10-24 | accepted | D6 |
| [0018](0018-ship-java-style-and-bedrock-style-generators.md) | Ship Java-style and Bedrock-style generators | 2026-10-24 | accepted | D7 |
| [0019](0019-gate-generators-with-fixtures-and-ratchets.md) | Gate generators with fixtures and ratchets | 2026-10-24 | accepted | D8 |
| [0020](0020-entity-data-from-bedrock-samples-checked-against-bds.md) | Entity data from bedrock-samples, checked against BDS | 2026-10-24 | accepted | D9 / O1 / Q1 |
| [0021](0021-shared-rules-engines-in-bevy-free-crate.md) | Shared rules engines in a bevy-free crate | 2026-10-24 | accepted | D10 / O2 / Q2 |
| [0022](0022-first-mobs-by-gameplay-value.md) | First mobs by gameplay value | 2026-10-24 | accepted | D11 / O3 / Q4 |
| [0023](0023-land-parked-branches-with-recorded-deviations.md) | Land parked branches with recorded deviations | 2026-10-24 | accepted | D12 / O4 / O5 / Q5 |
| [0024](0024-do-not-commit-eula-derived-artefacts.md) | Don't commit EULA-derived artefacts | 2026-10-24 | accepted | D13 / O6 |
| [0025](0025-one-rust-implementation-per-component-kind.md) | One Rust implementation per component kind; BDS-exact loader | 2026-10-24 | accepted | entities Q3, §4.7 |
| [0026](0026-bedrock-features-from-the-bds-install.md) | Bedrock features from the BDS install | 2026-10-24 | accepted | WD1 |
| [0027](0027-port-bds-surface-builders.md) | Port BDS surface builders | 2026-10-24 | accepted | WD2 |
| [0028](0028-decode-worldgen-data-at-startup.md) | Decode worldgen data at startup | 2026-10-24 | superseded by 0017 | WD3 |
| [0029](0029-statistical-decoration-first.md) | Statistical decoration first | 2026-10-24 | accepted | WD4 |
| [0030](0030-use-26x-world-clocks.md) | Sync time with 26.x world clocks | 2026-10-24 | accepted | WD5 |
| [0031](0031-parallel-chunk-generation-is-allowed.md) | Parallel chunk generation is allowed | 2026-10-24 | accepted | world §1.3 |
| [0032](0032-two-worldgen-numeric-backends.md) | Two worldgen numeric backends | 2026-10-27 | accepted | |
| [0033](0033-persist-block-states-by-name.md) | Save block states on disk by name and states | 2026-11-04 | proposed | |
| [0034](0034-bound-the-java-native-session.md) | Bound the Java-style native session without changing its output | 2026-11-05 | proposed | |
| [0035](0035-disable-a-plugin-after-it-traps.md) | Disable a plugin after it traps | 2026-11-05 | accepted | |
| [0040](0040-judge-player-movement-with-a-shared-simulation.md) | Judge player movement with a simulation shared with the measuring client | 2026-11-11 | proposed | |
| [0054](0054-measure-bds-movement-through-a-far-reporting-client.md) | Measure BDS's own movement with a client that reports a far position | 2026-11-12 | proposed | |
| [0060](0060-validate-the-movement-harness-against-bds-itself.md) | Validate the movement harness against BDS itself | 2026-11-12 | proposed | |
| [0067](0067-find-movement-rules-from-random-seeds-and-probes.md) | Find movement rules from random seeds and targeted probes, and lay the fixtures out a tick a line | 2026-11-12 | proposed | |

Proposed records await the owner's approval; their work is an open OpenSpec change anyone can pick up.
