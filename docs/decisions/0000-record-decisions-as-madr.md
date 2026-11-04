---
status: accepted
date: 2026-11-04
decision-makers: [project owner]
---

# Record decisions as MADR files in docs/decisions

## Context and Problem Statement

Decisions were scattered across `thoughts/shared/` plans and research documents (D1–D13,
WD1–WD5, "§7 Q1–Q5 resolved"), crate READMEs and a long-term direction doc. Agents and
contributors could not tell which were current, superseded or never made.

## Considered Options

- Keep decisions inside plans and research docs
- One decisions table in a single file
- One MADR 4 file per decision in `docs/decisions/`

## Decision Outcome

Chosen option: one MADR 4 file per decision, because each decision gets a stable number and
link, a status that can say "superseded by NNNN", and a fixed shape agents can write and search
(`record-decision` skill).

### Consequences

- Good, because supersession is explicit and old reasoning is never lost.
- Good, because OpenSpec `design.md` decisions have a permanent home at archive time.
- Bad, because records reconstructed from old plans carry only the reasoning those plans wrote down.

## More Information

Records 0001–0032 were reconstructed on 2026-11-04 from `thoughts/shared/` (removed; see git
history before that date) and checked against the code. "Accepted" on a reconstructed record
means the owner approved it in the source document or the decision shipped. The old ids are
kept in each record's title line so code comments citing "D3" or "WD5" can be followed.
