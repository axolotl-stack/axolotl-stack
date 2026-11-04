# Contributing

Thanks for helping. Read [AGENTS.md](AGENTS.md) for the workspace layout, hard rules and
validation commands; it is written for coding agents and humans alike. Crates with their own
rules have a nested `AGENTS.md`, and every crate has a README.

## How work is planned and recorded

Non-trivial changes go through [OpenSpec](https://openspec.dev): a proposal in
`openspec/changes/<name>/`, implemented task by task, then archived so its delta merges into
`openspec/specs/`. Finishing a change also records what it produced: a decision in
`docs/decisions/`, a copied BDS quirk in `docs/quirks/`, a lesson in `docs/solutions/`.
[docs/README.md](docs/README.md) explains where each kind of knowledge lives. Small fixes
skip OpenSpec. Run `tools/agents/setup.sh` once to install the pinned CLI (with Bun) with its
telemetry turned off.

## Finding work

[`docs/parity/PARITY.md`](docs/parity/PARITY.md) lists everything vanilla Bedrock 1.26.51
has and how far `unastar` is with each thing. Its
[Where to help](docs/parity/PARITY.md#where-to-help) section shows, per domain, the free
families with the most missing or partial rows and the areas already claimed. Pick a free
`missing` or `partial` row; its `gaps` say what is left.

## Claiming an area

Add a `[[claim]]` to [`docs/parity/claims.toml`](docs/parity/claims.toml) in your first pull
request for an area, with your `@github-handle` as `owner`, today's date as `since`, the
`domain`, and the `families`, `ids` or id `prefixes` you are taking (fields are explained in
[docs/parity/README.md](docs/parity/README.md#claims)). Remove the claim when you stop
working on it. `parity check` rejects claims that overlap an existing one; if you want part
of a claimed area, ask its owner in an issue first. If the work is an OpenSpec change, set
`change = "<change-name>"` on the claim and list the target rows in the proposal; the claim is
removed in the commit that archives the change (see
[docs/README.md](docs/README.md#parity-and-openspec)).

## The parity rule

**A feature is not finished until its parity rows are updated in the same commit.**

1. Update the affected rows in `docs/parity/<domain>.toml`: `status`, `owner`, `tests`,
   `evidence`, `gaps` (see [docs/parity/README.md](docs/parity/README.md)).
2. `done` needs a real test in the workspace; `verified` also needs committed native
   evidence (a BDS or Java capture or fixture) that the test compares against.
3. Run `cargo run -p parity -- generate`, then `cargo run -p parity -- check`.

## Before opening a pull request

Run what CI runs (`.github/workflows/test.yml`):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo run -p parity -- check
openspec validate --all --strict        # if openspec/ changed
bun tools/agents/check-links.ts         # if docs changed
tools/agents/sync-skills.sh --check     # if .agents/skills changed
```

plus the `cargo test` commands for the crates you touched.

## Writing about vanilla behaviour

Comments, docs, test names, fixtures and commit messages describe the behaviour our code
implements. Do not write Bedrock server or client internal class, function or symbol names,
addresses or offsets. Black-box provenance ("matches the BDS 1.26.51 capture",
"measured on BDS 1.26.51") is fine.
