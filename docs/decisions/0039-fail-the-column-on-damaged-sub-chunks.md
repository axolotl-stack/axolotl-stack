---
status: accepted
date: 2026-11-10
decision-makers: [project owner]
---

# Fail the column load on damaged sub-chunk data instead of loading BDS's substitutes

## Context and Problem Statement

BDS 1.26.51 loads damaged sub-chunk records with substitutes: an index past the palette
reads as the palette's first entry, an unknown block name becomes a placeholder that keeps
the name, an unknown or mistyped state becomes some state of the block, a truncated last
entry still loads (measured by editing saved records; quirk "World / storage" in
`docs/quirks/bds.md`). BDS never writes such data itself. unastar copies BDS exactly
([0013](0013-copy-bds-behaviour-exactly.md)), but a column it loads is saved again after
any edit, so a substitute would replace the original bytes on disk. Should unastar copy
the substitutes? Raised by OpenSpec change `persist-block-states-by-name`.

## Considered Options

- Copy BDS's substitutes
- Fail the column load and keep the record on disk

## Decision Outcome

Chosen option: "Fail the column load and keep the record on disk", because a later save
of a column loaded with substitutes would overwrite the damaged bytes for good, while a
failed load keeps them for repair, as the world-storage spec requires for any record that
does not decode. The harmless leniencies (the ignored y byte, trailing bytes, a palette
longer than its width) are still copied.

### Consequences

- Good, because damage from other tools or disk errors stays visible and repairable.
- Good, because it keeps the fail-fast rule: no silent fallback blocks.
- Bad, because a column BDS would show stays unloaded until it is repaired.

## More Information

Measurements: `docs/solutions/storage/bds-subchunk-palettes-are-nbt-block-states.md`.
Tests: `crates/unastar/src/world/chunk.rs` (`indices_outside_the_palette_are_errors_not_panics`),
`crates/unastar/src/world/chunk/persistent.rs` (`unknown_or_mistyped_entries_are_errors_not_substitutes`).
