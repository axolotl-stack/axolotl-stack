# java_data

Compact Java Edition block data for the Java-style generator's feature and
structure code generation (pinned Java version: **1.21.11**, same as
`../worldgen_data`).

`blocks.json` lists every Java block in registry order with its block type,
properties (in report order, last property varying fastest), default state
offset and one flag byte per state (bit 0 legacy solid, bits 1-6 full collision
face in `down, up, north, south, west, east` order, bit 7 full-cube collision).
A single number replaces the per-state array when all states share the flags.

Sources, not committed:

- `reports/blocks.json` from the 1.21.11 server's data generator;
- `data/pc/1.21.11/blockCollisionShapes.json` from
  [PrismarineJS/minecraft-data](https://github.com/PrismarineJS/minecraft-data)
  (MIT) at commit `5e063fa99cfe8ded065ec6788b66696b7e4ef143`.

Regenerate:

```sh
cargo run -p unastar_noise --example derive_java_blocks -- \
  <datagen>/reports/blocks.json <minecraft-data>/data/pc/1.21.11/blockCollisionShapes.json \
  crates/unastar_noise/java_data/blocks.json "<source note>"
```

The solid flag follows the Java rule (non-empty collision shape whose bounds
have an average edge of at least 0.7291666 or a full height) plus the blocks
that force it on (signs, banners, pressure plates). The derivation asserts that
state ids are contiguous and in mixed-radix property order.
