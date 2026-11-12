# Design: serverbound packet limits, held books and survival picks

## Decisions

### Where the limits are checked

Decision [0064](../../../../docs/decisions/0064-refuse-oversized-serverbound-packets-in-the-network-task.md)
(proposed): in unastar's network task, between `recv_packet_raw` and `decode_borrowed`. The size
check needs the raw packet; the field checks must fire whatever the game state is (BDS refuses a
book edit with no book open). A refused packet gets the violation through the existing
terminating-packet path and never reaches the tick loop.

### Counting characters

BDS counts code points (two- and four-byte characters measured). Valid UTF-8 is counted in
place; invalid UTF-8 counts each bad sequence as one replacement character (no allocation unless
invalid), so stray continuation bytes cannot slip past a limit.

### The interact text

A component holds the text last sent (`InteractText`, none at join); a `NetworkSendSet` system
compares it with the held item each tick and sends a one-entry `SetActorData` when it changes.
One place covers every way the held item changes (selection, commands, signing, picks).

### Survival actor picks

`select_item` already implements the survival rules of block picks (select or swap, never
create); the actor pick now passes survival players to it instead of returning early.

## Risks

- What BDS does with invalid UTF-8 is not measured; unastar counts each bad sequence once.
- The survival actor pick uses the creative reach (14 blocks); the survival reach is not measured.
