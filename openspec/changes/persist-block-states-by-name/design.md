# Design: persist block states by name

## On-disk block-state format (proposed: record 0033)

| Option | Good | Bad |
|---|---|---|
| A. Name + states NBT palette (Bedrock on-disk form) | BDS can open our worlds and we can open BDS worlds; survives palette changes | Slower encode/decode; needs a reverse lookup from name+states to runtime id |
| B. Keep runtime palette positions | Fast, already implemented | Worlds break on any palette change; not BDS-compatible, so the storage docs must stop claiming it |

Recommendation: A, reading B as a legacy format so existing worlds keep loading. Recorded as
[0033](../../../docs/decisions/0033-persist-block-states-by-name.md), status `proposed`; set it to `accepted` in the pull
request that implements this change once the owner approves.

## Notes

- Decode with a lookup keyed by name plus the full state map; an unknown name or state must be a
  typed load error, never a silent air block (fail fast).
- The legacy reader is detected per subchunk from the palette entry encoding.
- Fixture: a small BDS 1.26.51 world region with blocks that have multiple properties
  (stairs, doors, `pillar_axis` blocks), compared block-for-block after load.
