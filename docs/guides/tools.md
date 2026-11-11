# Measurement and verification tools

How we find out what BDS does, and how we check our code against it. Specs in
`openspec/specs/` name the tool that verifies a contract on their `Verified by:` lines; parity
rows cite the captures and fixtures these tools produce as `evidence`. Each tool's README has the
full instructions. Several need a local BDS or Java server and are run by hand, not in CI.

## Measuring vanilla

| Tool | What it does | Produces | Needs |
|---|---|---|---|
| [`crates/bds-extractor`](../../crates/bds-extractor/README.md) | Joins a local BDS as a temporary client and records login and post-spawn packets | validated capture JSON, the input to `unastar-data`'s datagen (`crates/bds-extractor/captures/1.26.51`) | local BDS 1.26.51 in offline mode |
| [`tools/bds-sweep`](../../tools/bds-sweep/README.md) + [`crates/bds-sweep`](../../crates/bds-sweep) | Capture sessions (T1) and scripted sweeps of built-in facts under Endstone (T2): armour, melee, fuel, drops, mining, command output; sub-chunk records BDS saves, checked with `/testforblock` (`subchunk_fixture.py`) | measured tables in `crates/unastar-data/data/measured/`; `crates/unastar/tests/fixtures/bds_1_26_51_subchunks.json` | local BDS (+ Endstone for sweeps); one session per BDS slot |
| [`tools/bds-worldgen-controls`](../../tools/bds-worldgen-controls) | Drives BDS (console, ticking areas) to generate reference worlds and controlled worldgen probes (structures, aquatic) | worldgen fixtures under `crates/unastar/tests/fixtures/` | local BDS |
| [`tools/client-diff`](../../tools/client-diff/README.md) + [`crates/client-diff`](../../crates/client-diff) | Runs the gophertunnel fork and jolyne against vanilla BDS and diffs their traffic; `-Mode conformance` checks captured wire layouts | `tools/client-diff/conformance-report.md`, corrections in `crates/valentine_gen/PROTOCOLGEN_ISSUES.md` | local BDS + Endstone with the capture plugin, Go |

## Checking our code

| Tool | What it checks | Where it runs |
|---|---|---|
| [`tools/wire-conformance`](../../tools/wire-conformance/README.md) | Generated protocol puts the same bytes on the wire as gophertunnel (the oracle pinned by protocolgen); checked-in Valentine output regenerates without drift and compiles | CI: `.github/workflows/wire-conformance.yml` on protocol changes |
| `cargo run -p parity -- check` ([`crates/parity`](../parity/README.md)) | The vanilla-parity catalog: every row has a status, `done` rows name existing tests, claims are valid | CI: `test` job |
| `openspec validate --all --strict` | Specs and changes are well-formed | CI: `knowledge` job |
| `cargo mutants` (`mutants.toml`) | Mutation testing of the diff for tokio-raknet, valentine, jolyne | CI: `mutation-test` job on PRs (advisory) |
| Worldgen parity and output-hash tests (`crates/unastar/tests/worldgen_parity.rs`, `worldgen_output_hash.rs`) | Generated chunks against BDS and Java fixtures; parity metrics only ratchet up | CI: `cargo test -p unastar` |

## Performance

| Tool | Measures | Report |
|---|---|---|
| [`tools/bench-compare`](../../tools/bench-compare/README.md) + [`crates/bench-compare`](../../crates/bench-compare/README.md) | Valentine codecs, jolyne batch framing and tokio-raknet against Cinnabar and the Go implementations | `tools/bench-compare/report.md` |
| [`tools/worldgen-perf`](../../tools/worldgen-perf/README.md) | Per-stage worldgen time and allocations on the 68 BDS fixture positions | `tools/worldgen-perf/report.md` |
| `cargo bench` in `crates/tokio-raknet`, `crates/tokio-nethernet`, `crates/valentine` | Transport latency and codec microbenchmarks | `crates/tokio-raknet/LATENCY_REPORT.md` |

## Keeping inputs current

| Workflow | Does |
|---|---|
| `.github/workflows/update-protocolgen.yml` | Daily: checks for a newer protocolgen manifest and proposes a pin bump |
| `.github/workflows/update-prismarine-data.yml` | Manual: refreshes the PrismarineJS reference data (cross-check only; see decision 0006) |

Rules that apply to all of them: captures and fixtures describe observed behaviour only (no BDS
internals; decision 0016); Mojang assets, BDS binaries and EULA-derived data are never committed
(decision 0024), so raw captures and caches live under the gitignored `/.cache/`.
