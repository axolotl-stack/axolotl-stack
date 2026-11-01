# Durable player item grants

Components request `grant-items`, `read-inventory` and `item-values`. A current
`WorldTx.grant_plan()` checks the real provider and native runtime readiness.
Unsupported providers and an ownership load still in progress return typed errors.
The plan binds its persistent world UUID and dimension; submission in another world
is rejected.

`GrantPlan.with_item(stack, quantity, placement)` retains the complete detached item
value. Quantity is an exact `u32`, independent of the prototype's count. Legal
native stack limits split both stackable and unstackable quantities. No quantity
is silently reduced.

`AnyInventoryOrDrop` merges compatible main-inventory stacks, fills empty main
slots, then drops the remainder. `EmptyMainOrDrop`, `EmptyArmourOrDrop` and
`EmptyOffhandOrDrop` fill only an empty specified slot. If that slot is occupied,
even by a compatible stack, the entire entry becomes world drops. Any amount
exceeding the one empty slot's legal capacity also becomes drops. Positional
placement never searches other inventory slots or displaces an existing item.

`request_hash()` supplies the exact logical hash used by the resulting receipt.
`Player.grant_items(key, plan)` stages an owned `InventoryOperation`; a callback
failure discards it. Only `Committed` proves persistence. Its account-scoped,
module-scoped `grant_receipt(key)` survives reconnect and restart. Reusing a key
with another logical intent conflicts. Replay reads the existing proof without
replacing a newer inventory or recreating collected items.

The stock LevelDB player provider writes the account, receipt and separately keyed
world-item ownership records in one synchronous atomic batch. Native inventory
publication and overflow actors follow that acknowledgement. Player pickup updates
the collector's canonical inventory and the exact item row in the same kind of
batch. Expiry and out-of-world removal durably retire the row before removing the
actor. Compact permanent tombstones prevent ID reuse. Pickup and retirement encode
only changed rows; the stable discovery index changes only on creation.

Positive delivery reserves the canonical inventory write lane while the actual
recipient stays `Ready` or `Retained`. Health, armour, food, movement, native wear
and selected-item consumption remain live. `Player.inventory_busy()` reports this
independently of physical availability. Competing replacements, slot permutations
and world transfers return busy/unavailable instead of changing those inputs.

Placement is selected against the authoritative submission image. Acknowledgement
adds only incoming counts to the current slots; it never reinstalls an older
inventory, health or armour state. If native progress changed the destination,
additional world ownership is acknowledged before publishing that part. A bounded
catch-up checkpoint retains native progress observed at the first acknowledgement;
ordinary later simulation is not a promise of durable per-tick state.

A fatal hit stays fatal during IO. Incoming items that would leave the corpse are
persisted as world items, while keep-inventory and keep-on-death items stay carried.
The actual post-drop state is acknowledged before ordinary death loot, notifications
or respawn. An exact current native receipt and original actor lineage authorize
this continuation; a rejected request does not invent a receipt. The native terminal
stamp is separate from component-owned source authority and historical archives.

An ambiguous write keeps the original write owner and the outcome uncertain until
explicit recovery. A correction failure after an earlier acknowledgement also stays
uncertain; it cannot report the whole request as not applied. Dropping an operation
or quitting does not cancel accepted IO or substitute another actor. `live_applied`
describes the original publication, not client receipt or rendering.

Limits remain explicit: at most 128 entries, 16,384 total items and 256 initially
planned overflow stacks; 4,096 active persistent items, 65,536 lifetime identities
and 16 MiB of item records per player provider. Destination corrections may create
additional rows from already-owed inventory quantities, subject to the same global
limits and at most 255 correction phases. The component byte allowance reserves
worst-case fragment copies before staging. Preflight exhaustion rejects; inability
to finish after an earlier write remains pending/recovery. There is no silent
eviction or reduction of the accepted opaque metadata limit.

Managed overflow items currently support player pickup and durable retirement.
They are excluded from ordinary merging, hopper collection and mob equipment
pickup because those paths do not yet have matching durable item ownership
transactions. The synchronous `/kill` command also rejects these items instead
of claiming an unacknowledged retirement. Their physics uses real native item actors. Restart restores their
recorded initial position and restarts the native despawn timer; intermediate
physics and timer state are not archived. Other player providers must explicitly
implement the atomic item-record capability; ordinary atomic account saves alone
do not satisfy it.
