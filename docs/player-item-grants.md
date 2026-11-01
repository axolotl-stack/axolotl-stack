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

An ambiguous write keeps the original inventory owner quarantined and the outcome
uncertain until explicit recovery. Dropping a guest operation or quitting does not
cancel accepted IO or substitute another actor. The existing `live_applied` field
describes the original inventory publication, not client receipt or rendering.
The current coordinator withholds both inventory and health during accepted IO.
This capability does not yet provide a nonfreezing inventory-only reservation.

Limits remain explicit: at most 128 entries, 16,384 total items and 256 overflow
actors per operation; 4,096 active persistent items, 65,536 lifetime item identities
and 16 MiB of item records per player provider. Exhaustion rejects the operation;
there is no silent eviction. Resource and byte allowances also cover retained work.

Managed overflow items currently support player pickup and durable retirement.
They are excluded from ordinary merging, hopper collection and mob equipment
pickup because those paths do not yet have matching durable item ownership
transactions. The synchronous `/kill` command also rejects these items instead
of claiming an unacknowledged retirement. Their physics uses real native item actors. Restart restores their
recorded initial position and restarts the native despawn timer; intermediate
physics and timer state are not archived. Other player providers must explicitly
implement the atomic item-record capability; ordinary atomic account saves alone
do not satisfy it.
