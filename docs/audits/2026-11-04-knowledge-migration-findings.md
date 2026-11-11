# Findings from the knowledge-system migration (2026-11-04)

While the old notes folder (removed) was triaged and the first OpenSpec specs were written from the code, reviewers
read the code against the docs and found the problems below. None were fixed in that migration,
which only changed documentation. Each item is open for anyone to pick up: fix it, then delete its
row here (or point it at the OpenSpec change that fixes it).

Specs describe the code as it behaves today, so where code and docs disagreed the specs follow the
code and the gap is listed here.

## Status (2026-11-05)

Pull requests #65–#70 fixed most of this file, and the specs in `openspec/specs/` were synced to
the merged code afterwards. Fixed rows have been deleted:

- #65: data, rules, packs, the parity tool, worldgen tooling tests, CI and provenance.
- #66: axelerator (friend gateway) and tokio-nethernet.
- #67: jolyne, axolotl-xbl, valentine, valentine_gen and tokio-raknet.
- #68: unastar world bounds, movement checks, inbound packet budgets, strict config and logging.
- #69: unastar storage and player persistence.
- #70: the plugin host.

What remains is backlog: items no PR was assigned, items a decision record settles, and new items
found while fixing and merging those PRs. Rows settled by a decision record say so.

## Bugs

| Area | Finding | Where | Tracked by |
|---|---|---|---|
| Plugins | Only the `filesystem` capability does anything; `network`, `write-player-data` and `process` are parsed and ignored | `crates/unastar/src/plugin/manager.rs` | change `plugin-async-tasks` (network) |
| Plugins | After a client disconnect, `on-player-quit` sees a player that is already despawned, so its getters return defaults (documented in `docs/guides/plugins.md`, not changed in #70) | `crates/unastar/src/server/runtime.rs`, `plugin/manager.rs` | |
| Storage | Subchunk palettes are saved as runtime palette positions | `crates/unastar/src/world/chunk.rs` | change `persist-block-states-by-name`, decision 0033 |
| Storage | A column loaded from storage always gets the full overworld bounds (`Chunk::new`), not the generator's `world_bounds()`. In a Java-style Nether or End world, players can place blocks outside the dimension's height in loaded chunks but not in fresh ones. Found in #69, deferred to the next storage change | `crates/unastar/src/storage/leveldb_world.rs`, `storage/blazedb.rs`, `world/chunk.rs` | |
| Storage | BlazeDB takes no lock on its directory, so two server instances can open the same BlazeDB world (LevelDB's lock makes the second one fail startup) | `crates/unastar/src/storage/blazedb.rs` | |
| Storage | A chunk held in memory after a failed unload save retries with a blocking write on the tick thread (`block_on_storage`). #69's backoff limits how often, not the blocking | `crates/unastar/src/world/ecs/systems.rs` (`process_chunk_unloads`) | |
| Worldgen | Java-style output is not a pure function of (seed, chunk): it decorates in request order through one shared native session, and the pool hands it chunks in nondeterministic order. **Accepted trade-off:** decision 0031 | `crates/unastar/src/world/generator/pipeline.rs`, `pipeline/native.rs` | decision 0031 (accepted) |
| Worldgen | The Java native session's chunk map is never evicted (grows for the server's lifetime) and one mutex serialises all Java decoration. A naive eviction hits the "completed native origin has no source chunk" panic | `crates/unastar/src/world/generator/pipeline/native.rs` | decision 0034 (proposed) |
| jolyne / RakNet | While writing #68's flood test, the server reported `batch checksum mismatch on encrypted batch 168` when an encrypted client sent about 1,500 single-packet batches back to back. Not reproduced or explained yet | `crates/jolyne/src/`, `crates/tokio-raknet/src/` | |
| axolotl-xbl | Built on its own, the crate gets `reqwest` with only the `json` feature and no TLS backend; inside the workspace, feature unification hides this. Needs an owner decision on which TLS feature to declare | `crates/axolotl-xbl/Cargo.toml` | |
| Packs | `ActorParser::register_component` accepts a duplicate registration. Left on purpose in #65; revisit when the addon loader exists | `crates/unastar-packs/src/actor.rs` | change `data-driven-gameplay` |
| Plugins (decided) | A plugin that traps is disabled for the rest of the run instead of being re-instantiated. **Accepted:** decision 0035; re-instantiating needs a state-reset contract | `crates/unastar/src/plugin/manager.rs` | decision 0035 (accepted) |

## Flaky tests

| Test | Finding | Next step |
|---|---|---|
| `crates/tokio-raknet/tests/capture_diff.rs::baseline_session_matches_bds` | Fails intermittently on CI: the server occasionally ACKs after about 1.2 ms where the check requires at least 9 ms (the 10 ms SYN). It failed on three PRs on 2026-11-05 | Investigate whether there is a real early-ACK path before loosening anything |
| `crates/unastar/tests/item_use_loopback.rs::eat_mine_and_shoot_over_loopback` | Times out at "cow is hurt" under load (Windows CI, and in a full local suite run). The summoned cow can wander before the arrow lands | Make it deterministic, e.g. a mob that can't move, or aim at its updated position |
| `crates/unastar/tests/mob_e2e.rs::summoned_cow_is_hurt_killed_and_drops_loot` (`#[ignore]`) | Failed once in a full-suite run during #68 (no `AddItemActor` within 3 s of the cow's death); passed in every later run and on `main`. The review found it unrelated to #68 | Track with the row above; likely the same moving-target problem |

## Docs that disagree with the code

| Doc | Says | Code does |
|---|---|---|
| `crates/valentine/README.md` | example `--versions …,1.26.30` cross-version dedup | the default protocolgen source rejects a `--versions` list without the manifest's version; it needs `--source prismarine` |
| `crates/unastar/src/entity/systems/lifecycle.rs`, `server/game/items.rs` | item pickup reach and merge radius "to be confirmed" | confirmed by capture (entities research, deviation X6) |
| `docs/audits/2026-09-maintenance-audit.md` (AUTH-01) | jolyne trusts Mojang-rooted legacy chains | legacy chains are accepted only in offline mode |
| `docs/audits/2026-09-maintenance-audit.md` (JOLYNE-AUTH) | two open items (unencrypted online replay, JWKS fetch cancellation) | both handled and tested |
| `docs/audits/2026-09-maintenance-audit.md` (NETHERNET-02) | callbacks honoured | the current API has no callbacks |
| `crates/parity/src/check.rs` (git errors), `crates/valentine_gen/src/parser/protocolgen.rs` (reused type id error) | one-line error messages | a line break inside each string literal has no `\` continuation, so the message carries a run of stray spaces (the same nit #68 fixed in the bounds error) |

## Gaps in test coverage and CI

- No `ActorKindRegistry` exists yet although decision 0025 calls for one.
- Feature-gated worldgen gates and the `worldgen_bench --ratchet` allocation ceilings need local
  installs, so CI doesn't run them.
- No order-independence test for Java-style generation. Decision 0031 accepts that the output
  depends on request order, so such a test would have to pin the order or compare per-session.
- The axelerator loops that call Xbox Live (friend sync, session upkeep, the transfer server
  against the real franchise service) have no end-to-end tests; they would need the network. #66
  covered the rest of the crate.
- The jolyne spawn timeout for a client that sends `SetLocalPlayerAsInitialized` and never sends
  `RequestChunkRadius` (`Timeout(Spawn)`) is checked from the code, not by a direct test.
