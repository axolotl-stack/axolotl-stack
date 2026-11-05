---
name: review-change
description: Review a diff (working tree, branch, or PR) against this repository's own rules before it is committed or merged — parity rows, provenance, generated code, specs, quirks, decisions, validation. Use before opening a PR, when asked to review a change in this repo, or at the end of an OpenSpec apply.
---

# Review a change against repo rules

This checks the project's rules. It complements, not replaces, a correctness review of the code
itself. If your tool can run a subagent, run this review in one so it reads the diff fresh.

## Gather

```sh
git diff --stat main...HEAD     # or the range/PR you were given; include uncommitted work
git diff main...HEAD
```

If `repowise` MCP tools are available, call `get_change_risk` on the range and look at the top
drivers first. Skip silently if not installed.

## Checklist

Report each item as pass / fail (with file:line) / not applicable.

1. **Parity.** Gameplay behaviour changed in `crates/unastar*` → matching rows in
   `docs/parity/*.toml` changed in the same diff, `PARITY.md` regenerated; `done` rows name a
   test that exists; `verified` rows name committed evidence.
2. **Provenance.** No Bedrock server/client internal class, function or symbol names, addresses or
   offsets anywhere in the diff (code, comments, tests, fixtures, docs, commit messages).
3. **Generated code.** No hand edits under `crates/valentine/bedrock_versions/` or
   `crates/valentine/src/bedrock/` unless the generator changed and the output was regenerated.
4. **Specs.** If behaviour named in `openspec/specs/*/spec.md` changed, there is an OpenSpec change
   with a delta spec (or the archive already merged it). Specs don't restate vanilla behaviour.
5. **Quirks and decisions.** Odd BDS behaviour copied → row in `docs/quirks/`. A choice between
   alternatives → record in `docs/decisions/`. Lesson worth keeping → note in `docs/solutions/`.
6. **Layers.** No new dependency from a lower layer onto a higher one (transport → protocol →
   jolyne → unastar); Bevy-free crates stay Bevy-free.
7. **Fail fast.** No new silent fallbacks or guessed wire/data values.
8. **Docs.** Nearby README updated when behaviour, workflow or architecture changed; relative links
   resolve; no new references to removed paths.
9. **Validation actually run**: fmt, clippy `-D warnings` for touched crates, their tests,
   `parity check`, `openspec validate --all --strict` if `openspec/` changed. Say what you ran.

End with a short verdict: ready / ready after fixes (list) / needs rework.
