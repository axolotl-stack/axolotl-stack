# Solutions

Lessons learned the hard way: traps, non-obvious root causes, measurements that drove a design,
techniques that worked. One lesson per file, under `<area>/`. Search before you start work:

```sh
rg -n -i '<topic>' docs/solutions
rg -l 'status: stale' docs/solutions     # notes a later change contradicted
```

Written with the `capture-solution` skill (`.agents/skills/capture-solution/SKILL.md`), usually
at the end of an OpenSpec change. A note that turns out wrong is marked `status: stale` with a
`superseded-by:` line, not silently deleted.

Front matter: `title`, `area`, `tags`, `date`, `origin` (PR, commit or archived change),
`status` (`current` | `stale`).

Not here: contracts (→ `openspec/specs/`), decisions (→ `docs/decisions/`), copied BDS
behaviour (→ `docs/quirks/`), vanilla progress (→ `docs/parity/`).
