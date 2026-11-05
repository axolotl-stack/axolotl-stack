---
name: record-decision
description: Write an architecture/any decision record (MADR 4) in docs/decisions. Use when a choice between real alternatives was made or approved by the owner — a data source, a codegen strategy, an API shape, a policy like "copy BDS exactly" — or when an earlier decision is superseded. Also use during OpenSpec archive when design.md contains decisions.
---

# Record a decision

Decision records answer "why is it built this way?" and are never deleted, only superseded.
Format: MADR 4 (https://adr.github.io/madr/), template at `docs/decisions/template.md`.

## Steps

1. Check existing records: `rg -n -i '<topic>' docs/decisions/`. If this changes an earlier
   decision, the new record says `supersedes: NNNN` and you set the old one's
   `status: superseded by NNNN` (the only edit ever made to an accepted record).
2. Next number: the highest `docs/decisions/NNNN-*.md` + 1, zero-padded to 4 digits.
   File: `docs/decisions/NNNN-short-title-with-dashes.md`.
3. Fill the template. Required: front matter (`status`, `date`, `decision-makers`), title as the
   decision itself ("Copy BDS behaviour exactly, quirks included"), **Context and Problem**,
   **Considered Options** (at least two real ones), **Decision Outcome** with the reason, and
   **Consequences** (good and bad). Optional sections only when they add something.
4. `status: proposed` until the owner approves; `accepted` once they have. Don't mark a decision
   accepted on your own authority.
5. Add the record to the index table in `docs/decisions/README.md`.
6. Link it from where it matters (the OpenSpec change's `design.md`, a crate README, the spec).

Keep it short: a decision record is a page, not a design document. Link research in
`docs/research/` for the long version.
