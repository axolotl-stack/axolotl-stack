## Context

BDS 1.26.51 liquids were measured with timeline sweeps (every watched position read every tick)
and client captures cut per tick. Findings that shape the design:

- A liquid set by `/setblock` or Endstone never starts flowing; only a neighbour change, a bucket
  or its own spread wakes it. Water ticks 5 game ticks after waking, lava 30 (10 in the nether).
- The flow follows the Beta-era rules: depth recomputed from neighbours, falling depth 8 plus the
  parent's depth, sources between two sources on a solid block or a source, drop search 5 blocks
  for water and 3 for lava, lava step 2 in the overworld and 1 in the nether, no random part.
- Ticks scheduled into another chunk land a tick early about half the time (BDS's chunk order
  changes from tick to tick); ticks due in the same tick inside one chunk run in an order that is
  neither first-in-first-out nor a simple heap over the scheduling order we could reconstruct.
- Changes are sent once per tick per subchunk: one change as `UpdateBlock`, several as
  `UpdateSubChunkBlocks`.

## Decisions

1. **Engine shape.** Options: piggyback on `block_use::ScheduledBlocks` (one global queue) or a
   dedicated resource with per-chunk queues. Chosen: a dedicated `Liquids` resource with
   per-chunk queues and a random per-tick chunk order, because the chunk-crossing timing is part
   of the measured behaviour and the global queue cannot express it. The engine is generic over a
   `LiquidTarget` so the replays run in memory.
2. **Same-tick order.** Options: keep searching for BDS's order, or run due ticks in the order they
   were scheduled and skip order-sensitive cells in the replays with the reason. Chosen: the
   latter; the searched models (first-in-first-out, last-in-first-out, a standard binary heap,
   every spread and neighbour order) each explain at most 7 of the 10 clean cells. Recorded as a
   gap.
3. **Block facts.** Measured per block into `unastar-data` (`liquids/blocks.json`) and decoded at
   startup, never typed per block in Rust (decision 0013 and data-before-code).
4. **Liquid layer on writes.** The shared write path asks the liquid module which storage a new
   block keeps (a block that holds water keeps a source; air over a waterlogged block leaves the
   water), so every writer (placement, commands, redstone, block updates) agrees.

## Risks

- Stairs: only the first palette state was measured (full side +x); other directions are
  assumed by symmetry.
- The layer write path changes every block write; the full unastar suite guards it.
