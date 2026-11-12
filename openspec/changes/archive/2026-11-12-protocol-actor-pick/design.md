# Design: picking actors

## Decisions

### Where the actor's item comes from

Options: (a) derive it from data (`AvailableActorIdentifiers.hasspawnegg`, `<actor>_spawn_egg`
names, creative content); (b) a measured table of every summonable actor.

None of the data predicts BDS: armor stands have an item and the ender dragon an egg, and both
give nothing; the wither's egg is not in the creative inventory and is given; axolotls give a
bucket; vehicles give items no name rule finds. Mobs that move away from the player right after
the summon are missed (the reach is 14 blocks), so the table takes an answer from any of three
runs.
So (b), measured like the other built-in facts (`data/measured`, decision D4), decoded with typed
errors and resolved against the item registry at startup (`ActorPickItems::resolve`).

### Reusing the block pick

The actor pick selects its item with the block pick's `select_item` (hotbar search, swap, creative
placement), which gains an optional `GuiDataPickItem` name sent before the inventory, as BDS orders
it. Block picks pass no name until their names are measured.

## Risks

- The table key is the identifier the mob was created with (`MobKind`), which is the type BDS
  announces in `AddActor` (`villager` summons a `villager_v2`).
