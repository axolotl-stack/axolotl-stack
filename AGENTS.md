# AGENTS.md

Instructions for coding agents (Claude Code, Codex) and humans working in `axolotl-stack`:
a Rust workspace for Minecraft: Bedrock Edition servers, proxies and tooling. The reference
behaviour is **BDS 1.26.51, protocol 2193**. Product overview: [README.md](README.md).

## Hard rules

1. **Parity rule.** A gameplay feature is not finished until its rows in `docs/parity/<domain>.toml`
   are updated in the same commit, then `cargo run -p parity -- generate` and `-- check` pass.
   Use the `parity-update` skill. Details: [docs/parity/README.md](docs/parity/README.md).
2. **Provenance rule.** Never write Bedrock server or client internal class, function or symbol
   names, addresses or offsets in code, comments, docs, tests, fixtures or commit messages.
   Black-box provenance ("matches the BDS 1.26.51 capture") is fine. Names BDS sends on the wire
   are redacted from captures too; `bun tools/agents/check-redactions.ts` (CI) enforces the list.
3. **Copy BDS exactly**, quirks included, and record every odd behaviour you copy in
   [docs/quirks/bds.md](docs/quirks/bds.md) (Java worldgen: [docs/quirks/java.md](docs/quirks/java.md)).
   Built-in facts BDS doesn't ship as data are measured from BDS, not taken from wikis.
   See [docs/decisions/](docs/decisions/).
4. **Generated code is not edited by hand.** `crates/valentine/bedrock_versions/v1_26_51` and
   `crates/valentine/src/bedrock` come from `valentine_gen`; change the generator and regenerate
   (`regen-protocol` skill). CI regenerates and fails on any diff.
5. **Fail fast.** Don't hide protocol or data quirks in fallback logic or guessed wire types.
6. **Keep layers separate.** Transport, protocol, auth and server logic are distinct crates.
   Shared codec runtime → `bedrock_core`; generated protocol → `bedrock_versions`; generator
   logic → `valentine_gen`; Bevy-free rules/packs → `unastar-rules` / `unastar-packs`.

## Toolchain and validation

- Rust `nightly-2026-03-05` (pinned in `rust-toolchain.toml`, uses `portable_simd`), edition 2024,
  `tokio`, `bevy_ecs 0.17`. Clone with `--recurse-submodules`.
- Native deps for `bleveldb-sys`: Linux `zlib1g-dev libsnappy-dev`; Windows vcpkg `zlib` + `snappy`
  (`x64-windows-static-md`) with `VCPKG_ROOT` set.
- Before you call work done, run what CI runs (`.github/workflows/test.yml`) for what you touched:

```sh
cargo fmt --all -- --check
cargo clippy -p <crate> --all-targets -- -D warnings     # then --workspace if shared code moved
cargo test -p <crate>
cargo run -p parity -- check
openspec validate --all --strict                          # when openspec/ changed
```

- Generator output changed? `cargo run -p valentine_gen -- --latest`, `cargo fmt --all`, then
  `cargo test -p valentine`.
- jolyne feature combinations are linted separately in CI; run them when touching jolyne features.

## Workspace map

| Path | What |
|---|---|
| `crates/valentine` (+ `bedrock_core`, `bedrock_versions/v1_26_51`) | Protocol surface, codec runtime, generated packets. Import via `valentine::bedrock::version::v1_26_51`. |
| `crates/valentine_gen` | Protocol generator. **Read [its AGENTS.md](crates/valentine_gen/AGENTS.md) before editing.** |
| `crates/jolyne` | Protocol engine: login, encryption, compression, batching, state machine. |
| `crates/tokio-raknet`, `crates/tokio-nethernet` | Async RakNet and NetherNet (WebRTC) transports. |
| `crates/axolotl-xbl` | Xbox Live / Minecraft services auth. |
| `crates/axelerator` | Friend-broadcast gateway (NetherNet + Xbox sessions, then transfer). |
| `crates/unastar` | The server. **Read [its AGENTS.md](crates/unastar/AGENTS.md) before editing.** |
| `crates/unastar-api`, `unastar-api-macros`, `example-plugin` | WASM plugin API (WIT world), macros, reference plugin. |
| `crates/unastar-data` (+ `datagen`, `codegen`) | Source-attributed game data embedded into the server. |
| `crates/unastar-rules`, `crates/unastar-packs` | Bevy-free filters/events/Molang/loot; behaviour-pack ingestion. |
| `crates/unastar_noise`, `crates/unastar_bedrock_codegen` | Build-time AOT toolchains for the Java-style and Bedrock-style world generators. |
| `crates/bds-extractor`, `crates/bds-sweep`, `crates/client-diff`, `crates/bench-compare` | BDS capture/extraction, scripted sweeps, wire diffing, benchmarks. Harnesses in `tools/`. What each one measures or checks: [docs/guides/tools.md](docs/guides/tools.md). |
| `crates/parity` | Generates and checks the vanilla-parity catalog in `docs/parity/`. |

## Where knowledge lives

Read the relevant entries **before** planning; write back **after** finishing. Overview:
[docs/README.md](docs/README.md).

| Question | Location | Write it with |
|---|---|---|
| What do our systems guarantee? | `openspec/specs/<capability>/spec.md` | OpenSpec change → archive |
| What are we changing right now, and why? | `openspec/changes/<name>/` | `openspec-propose` skill (`/opsx:propose`) |
| Why is it built this way? | `docs/decisions/NNNN-*.md` (MADR) | `record-decision` skill |
| What does vanilla do, and how far are we? | `docs/parity/` | `parity-update` skill |
| Which odd BDS/Java behaviour do we copy? | `docs/quirks/` | `log-quirk` skill |
| What did we learn the hard way? | `docs/solutions/<area>/*.md` | `capture-solution` skill |
| Long-form background research | `docs/research/` | by hand |

Specs describe **our** contracts (codec bounds, transport limits, plugin event order, generator
invariants). Vanilla behaviour belongs in parity and quirks; specs link to it, never copy it.

### Workflow for non-trivial work

1. **Explore / propose**: `/opsx:propose` (Codex: `$openspec-propose`). Search `docs/solutions/`,
   `docs/decisions/` and the affected specs first. Small fixes (typo, one-line bug) skip OpenSpec.
2. **Apply**: `/opsx:apply`. Work task by task; run the validation above.
3. **Review**: `review-change` skill on the diff before opening a PR.
4. **Archive and compound**: `/opsx:archive` merges the delta into `openspec/specs/`; then run
   `capture-solution` for anything surprising, `record-decision` for any choice between
   alternatives, `log-quirk` for copied BDS oddities. Skipping this step is how docs rot.

Research questions about vanilla behaviour: use the `bedrock-research` skill (source order and
provenance rules).

## Conventions

- Match the style of the crate you're in. Prefer small helpers over mixed traversal/emission logic.
- Doc comments where they help: protocol layout, generator phases, non-obvious data transforms.
- Update the nearby `README.md` when behaviour, workflow or architecture changes.
- Search with `rg`. Never commit tokens (`token*.json`, `mpsd_token*.json` are gitignored).
- Tooling setup: `tools/agents/setup.sh` installs the pinned OpenSpec CLI with telemetry off.
  Use `bun`/`bunx` for JS tooling, never `npm`/`npx`.
- Agent skills live in `.agents/skills/` (canonical) and are mirrored to `.claude/skills/` by
  `tools/agents/sync-skills.sh`; CI fails if they differ. `openspec-*` skills are generated by
  `openspec update` — don't edit them; put project rules in `openspec/config.yaml`.
- Commits: conventional style (`feat(scope):`, `fix(scope):`, `docs(scope):`).
