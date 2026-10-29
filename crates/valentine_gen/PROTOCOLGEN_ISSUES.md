# protocolgen schema defects found by capture conformance

Upstream: [cinnabar-mc/protocolgen](https://github.com/cinnabar-mc/protocolgen),
pinned here as the `crates/valentine_gen/protocolgen` submodule at
`e37c952` (branch `feat/mojang-actor-symbols-1.26.51`: `7b2e47c` plus the
Mojang actor-symbol import of PR #43; `generated/1.26.51/manifest.json`,
protocol 2193). The submodule is
never patched; Valentine applies the corrections below while lowering the
manifest (`src/parser/protocolgen_corrections.rs`), and generation fails if the
corrected manifest node changes or disappears.

Evidence comes from `tools/client-diff` conformance captures of BDS 1.26.51
(Endstone 0.11.12): every packet BDS sent or received in the capture sessions
was decoded and re-encoded by Valentine (see
`tools/client-diff/conformance-report.md`). Each entry is written so it can be
filed upstream as is.

## 1. `LevelEventGenericPacket` field 1 is a compound body, not a rooted NBT tag

**Manifest location:** `packets[id=124].fields[ordinal=1]` (`__[[CTD]]__`):

```json
"encode": {
  "kind": "primitive",
  "primitive": { "code": "nbt_le", ... },
  "encoding": "network"
}
```

**Observed wire shape:** BDS writes the event data as a compound body,
i.e. the compound's *children* — each a named tag (`tag id`, `varuint` name
length, name, payload) — followed by `TAG_End` (`0x00`). There is no root tag
ID and no root name. The body ends the packet.

**Effect of the current schema:** a decoder that reads one rooted `nbt_le`
value treats the first child tag as the root (its ID and name are well-formed),
stops after that child's payload, and leaves the remaining children and the
`TAG_End` as trailing packet bytes. A Jolyne client failed in play with
`TrailingPacketBytes { remaining: 46 }` on event 9801.

**Captured evidence (BDS 1.26.51):** 1 443 LevelEventGeneric packets from two
capture sessions (697 before the correction, 746 in the committed conformance
run) across 9 event IDs, every one of which parses as "named tags until
`TAG_End`" consuming exactly the rest of the packet, and none as a single
rooted tag:

| Event ID | Meaning | Example body (decoded) |
|---:|---|---|
| 9801 | sleeping players | `ableToSleep:int=1, overworldPlayerCount:int=1, sleepingPlayerCount:int=0` |
| 1901 | record/music play | `fadeSeconds:float, repeatMode:int, trackName:string="record.cat", volume:float` |
| 1902 | music stop | `fadeSeconds:float` |
| 2026 | sculk charge | `originX/Y/Z:float, pos<N>x/y/z:float, ...` |
| 2027 | sculk vibration | `origin:compound{type,x,y,z}, speed:float, target:compound{...}, timeToLive:float` |
| 2035–2038 | sculk sensor/shrieker | `originX/Y/Z:int` or `charge/facing/x/y/z` |

Smallest example, event 2038 (packet ID `0x7c`, zigzag event ID, body):

```text
7c ec 1f 03 01 78 01 03 01 79 77 03 01 7a 06 00
   └eventId┘ └int "x"=-1┘ └int "y"=-60┘ └int "z"=3┘ └TAG_End
```

All nine fixtures are committed in
`crates/valentine/tests/fixtures/conformance/level_event_generic.jsonl`.

**Reference implementations:** gophertunnel
(`minecraft/protocol/packet/level_event_generic.go`) reads the data as raw
remaining bytes and documents it as "loosely floating NBT tags"; its note that
callers should *append* `0x00` when wrapping in a compound is inaccurate — the
terminating `TAG_End` is already on the wire, so wrapping needs only the
`0x0a 0x00` prefix.

**Suggested patch:** give canonical NBT nodes a layout distinct from the
rooted value, e.g.

```diff
 "encode": {
   "kind": "primitive",
-  "primitive": { "code": "nbt_le", "width": 0, "signed": false, "zigzag": false, "endianness": "little" },
+  "primitive": { "code": "nbt_le_compound_body", "width": 0, "signed": false, "zigzag": false, "endianness": "little" },
   "encoding": "network"
 }
```

(or an `"nbt_layout": "compound_body"` attribute on `nbt_le` nodes), and select
it for fields BDS writes as a compound body rather than as a rooted named tag
with a root header. The Endstone pin
(`packets/LevelEventGenericPacket.json#/fields/1`) is the claim to adjudicate.
