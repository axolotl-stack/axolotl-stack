# Wire conformance

Checks that protocol code generated from Mojang's schemas puts the **same bytes
on the wire** as hand-written implementations that are validated against real
servers.

## Why this exists

Mojang's published schemas are a good skeleton but are not authoritative about
bytes. Migrating to 1.26.40 required 147 corrections across more than 100
schemas, and essentially none of them were discoverable from the docs alone:

- fields marked `required` that actually carry a presence byte (`ChangeDimension`,
  `Transfer`, `PlayerAuthInput`, and others) — every one desyncs the packet;
- `DimensionDefinition` missing its leading identifier string entirely;
- `Compression` on an 8-bit field meaning a raw byte in one packet and a varint
  in another;
- array-level serialization options that apply to the elements.

Each was found by comparing against an implementation. Without this check they
are found by desyncing against a live server instead.

## How it works

Three stages, deliberately kept separate:

1. `valentine_gen --emit-wire-manifest` walks the **resolved IR** and writes each
   packet's fully flattened wire operations. This asks the generator what it
   actually emits rather than parsing generated Rust, which is what the earlier
   approach did — it stalled at every nested type and left 142 packets unchecked.
2. `extract_gophertunnel.go` walks gophertunnel's `Marshal` methods with `go/ast`,
   recursing through helper types, and produces the same shape. It needs no
   module resolution and downloads nothing.
3. `compare.mjs` diffs the two per packet and classifies each as `AGREEMENT`,
   `DIVERGENCE`, `UNRESOLVED`, or `NO_ORACLE_PACKET`.

## The gate

`compare.mjs` exits non-zero when a packet diverges and is **not** listed in
`expected-divergences.json`. Every accepted entry carries a reason, so "known
divergence" cannot quietly become "any divergence". It also fails if agreement
drops below `min_agreement`, which catches packets sliding from verified into
unverified.

A new divergence means one of three things, in order of likelihood:

1. the generated protocol is wrong — fix it with an **override that cites the
   oracle file and line**, never by hand-editing generated output;
2. the oracle changed — check whether its wire actually changed before assuming
   we are at fault;
3. the comparison is too coarse — fix the comparator, and say so in the entry.

## Running locally

```sh
# 1. oracle checkout at the pinned ref (see oracle-pins.json)
git clone https://github.com/Sandertv/gophertunnel /tmp/gt && git -C /tmp/gt checkout <ref>

# 2. manifest from the generator
cd crates/valentine_gen
cargo run -p valentine_gen -- --source mojang --proto \
  --output-dir /tmp/generated --emit-wire-manifest /tmp/conf/manifest.json

# 3. oracle extraction
cd tools/wire-conformance && go run . /tmp/gt
mv gophertunnel-flat.json gtx2-diagnostics.json /tmp/conf/

# 4. compare and gate
WIRE_CONFORMANCE_DIR=/tmp/conf node tools/wire-conformance/compare.mjs
```

Drop a `cloudburst.json` into the same directory to enable the CloudburstMC
cross-check when adjudicating a divergence. It is optional and never a gate
input: its absence must not be able to turn a divergence into an agreement, and
it is stale for some packets (`PlayerAuthInput` still inherits v766's bitmask),
so treat its silence as *no evidence* rather than as agreement.

## Known limitations

- **Unresolved is not agreement.** Packets whose bytes sit behind gophertunnel
  interface methods (`ItemInstance`, `EntityMetadata`, `ItemDescriptorCount`,
  `BEARGB`) cannot be read statically. gophertunnel implements them; the
  extractor just cannot follow them yet. They are reported, never assumed.
- **Runtime branches never resolve.** Where gophertunnel switches on a value at
  runtime, there is no single linear byte sequence to compare. Only a live
  connection settles those.
- **UUID is compared as 16 bytes at a position**, not byte-for-byte ordering.
- Pins are commits, not branches. A branch citation silently rots — that
  invalidated pinned overrides twice during the 1.26.40 migration.
