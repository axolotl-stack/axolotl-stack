# Project knowledge

How the project records what it knows. Agents and humans use the same system; the rules for
using it are in [AGENTS.md](../AGENTS.md).

| Folder | Answers | Changes how | Written with |
|---|---|---|---|
| [`../openspec/specs/`](../openspec/specs/) | What do our systems guarantee? | only by archiving an OpenSpec change | `/opsx:propose` → `/opsx:apply` → `/opsx:archive` |
| [`../openspec/changes/`](../openspec/changes/) | What is being changed now, and why? | one folder per change; archived when merged | `/opsx:propose` |
| [`decisions/`](decisions/) | Why is it built this way? | new record per decision; old ones superseded, never deleted | `record-decision` skill |
| [`parity/`](parity/) | What does vanilla have, and how far are we? | every gameplay commit | `parity-update` skill |
| [`quirks/`](quirks/) | Which odd BDS / Java behaviour do we copy on purpose? | when a quirk is copied or revisited | `log-quirk` skill |
| [`solutions/`](solutions/) | What did we learn the hard way? | after non-trivial work | `capture-solution` skill |
| [`research/`](research/) | Long-form background (algorithms, data surveys) | rarely; dated files | by hand / `bedrock-research` skill |
| [`audits/`](audits/) | Point-in-time reviews and their remediation | dated files, historical once superseded | by hand |
| [`guides/`](guides/) | How to do something (build plugins, run tooling) | when the workflow changes | by hand |

## The loop

```
propose ──► apply ──► review ──► archive ──► compound
  ▲  reads specs, decisions,              writes specs (delta merge), decisions,
  │  solutions, quirks, parity            quirks, solutions, parity rows
  └────────────────────────── next change starts from more knowledge ◄──────────┘
```

The compound step is what makes this grow: each finished change leaves behind the decision,
lesson or quirk it produced, and the next proposal reads it first.

## Parity and OpenSpec

They answer different questions and stay separate: parity rows are **generated from vanilla data**
and record how far we are with each vanilla thing; OpenSpec specs are **written by us** and record
the contracts our own systems keep. A gameplay change usually has no delta spec
(`skip_specs: true`): its parity rows are its spec. The two are linked at three points:

| Step | Parity | OpenSpec |
|---|---|---|
| Pick work | free rows in `PARITY.md` "Where to help" | proposal's Impact → "Parity rows" lists the exact ids and target statuses |
| Start | `[[claim]]` in `claims.toml` with `change = "<name>"` | first task of `tasks.md` |
| Finish | rows updated, `parity generate` + `check`; claim removed | archive checks every listed row moved, then removes the claim |

`parity check` enforces the link: a claim whose `change` doesn't exist, or has been archived,
fails CI, so finished work can't leave a stale claim behind.

## Tooling

- OpenSpec CLI, pinned to 1.16.0. Run `tools/agents/setup.sh` once per machine: it installs the
  pinned CLI with Bun and disables OpenSpec's anonymous usage telemetry in its per-user config,
  so it stays off for Claude Code, Codex and the terminal. Telemetry is also off without the
  script inside Claude Code sessions (`.claude/settings.json` sets `OPENSPEC_TELEMETRY=0` and
  `DO_NOT_TRACK=1`) and in CI (`OPENSPEC_TELEMETRY=0`, and OpenSpec turns itself off when `CI` is set).
- After upgrading OpenSpec, run `openspec update` to regenerate the `openspec-*` skills and
  `/opsx:*` commands, then `tools/agents/sync-skills.sh`.
- CI checks: `openspec validate --all --strict`, `parity check`, skill mirror in sync, relative
  Markdown links resolve.
