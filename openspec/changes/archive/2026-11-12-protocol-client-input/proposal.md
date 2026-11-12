# Proposal: book editing, measured with a live client

## Why

`BookEditPacket` is `missing`: in the round 2 relay runs BDS 1.26.51 ignored every edit, and the
note in `docs/solutions/tooling/scripted-client-packets-in-bds-captures.md` blamed the missing
`PlayerAuthInput`. PR #71 (`origin/integration/2026-11-04-lanes`, commits `54cbae36`, `8d72462b`,
`e53e5f83`) has a book module built from wiki limits (256 characters, 16-character titles) and a
recording of a real client editing a book on BDS with Endstone; it was never re-verified against
plain BDS.

## What Changes

- bds-extractor: `--input` (a `PlayerAuthInput` stream that follows the server's moves and
  corrections), `!equip SLOT`, `!use-item SLOT` and `!pick-block` actions, and `--dump` to decode a
  recording. `protocol_probe.py book` (five sessions) and `commit_protocol_fixtures.py` changes;
  fixtures in `crates/bds-extractor/captures/1.26.51/protocol/book/`.
- unastar: `server::game::books`: a book and quill opens with an item use on air (`UseKind::OpenBook`)
  and gets an empty page list; `BookEdit` edits only the open book, pads pages, caps them at 50 and
  answers with BDS's `InventoryTransaction`; signing makes a written book. Book items are resolved
  at startup (`BookItems`). Item NBT is written with sorted keys (`item::nbt`, decision 0048).

Crates: `unastar`, `bds-extractor`, `tools/bds-sweep` (Python).

## Impact

### Parity rows

`protocol`, owner `protocol`:

| Row | From | To |
|---|---|---|
| `BookEditPacket` | missing | verified |

No claim in `docs/parity/claims.toml` covers it. The PR #71 limits (256 characters, 16-character
titles) are not copied: BDS keeps longer text.

## Out of scope

- The interact-text `SetActorData` BDS sends when the held item changes ("action.interact.write",
  "Read") and the attribute and actor-data resync after opening a book (items lane).
- Lecterns and reading written books.
