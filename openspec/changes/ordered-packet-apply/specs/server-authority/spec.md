## RENAMED Requirements

- FROM: `### Requirement: Client packets reach game state only through domain queues`
- TO: `### Requirement: Client packets reach game state only through the packet log`

## MODIFIED Requirements

### Requirement: Client packets reach game state only through the packet log
`GameServer::route_packet` SHALL resolve the sending session to its player entity and append each recognised packet, with its session and entity, to one per-tick client packet log in arrival order. Only the exclusive packet system that consumes the log, through one handler per packet kind, changes game state. A packet from an unknown session MUST be dropped. `MovePlayer` and every packet the router does not recognise (for example client game-mode or ability requests) MUST be ignored.

#### Scenario: Ignored client movement packet
- **WHEN** a client sends `MovePlayer`
- **THEN** the packet is dropped and the player's position changes only through `PlayerAuthInput`

#### Scenario: Unrouted packet
- **WHEN** a client sends a packet type the router has no arm for
- **THEN** it is logged at debug level and no game state changes

#### Scenario: Routing does not change state
- **WHEN** a packet is routed and the tick has not run yet
- **THEN** the packet is in the client packet log and no component or resource other than the log has changed

## ADDED Requirements

### Requirement: The network pass applies or queues each packet in arrival order
Each tick the packet system SHALL first take each session's logged packets oldest first. It MUST apply a packet whose kind takes effect on arrival at once, MUST queue a `PlayerAuthInput` for its player, and MUST hold an attack or item use in the air until that player's next `PlayerAuthInput`, queued with that input. The kinds and their timing are listed in `crates/unastar/src/server/game/packet_order.rs`, following the BDS 1.26.51 captures recorded in `docs/quirks/bds.md`.

#### Scenario: An attack waits for the attacker's next input
- **WHEN** a player sends an attack and then no `PlayerAuthInput` for five ticks
- **THEN** the attack has no effect until the tick in which that player's next input is applied

#### Scenario: A command after an attack is in place for it
- **WHEN** a player sends an attack and then a command, followed by an input, in one tick
- **THEN** the command takes effect before the attack is resolved

Evidence: crates/unastar/src/server/game/packet_order.rs (`PlayerQueue`, `timing`), crates/unastar/src/server/game/packet_order_tests.rs, crates/unastar/src/server/game/testdata/pvp-race-*.jsonl

### Requirement: The player pass resolves held actions before their input
After the network pass the packet system SHALL take every queued input of each player oldest first. For each input it MUST resolve the actions held for it with the player's state from before that input, and only then apply the input's flags, rotation and movement.

#### Scenario: An attack uses the state from before its input
- **WHEN** a sprinting player sends an attack and then, in the same tick, an input that stops sprinting
- **THEN** the hit is resolved with the player sprinting

#### Scenario: Queued inputs are all consumed
- **WHEN** a player's twelve inputs arrive in one tick
- **THEN** all twelve are applied in that tick's player pass, in the order sent, each after the actions held for it

Evidence: crates/unastar/src/server/game/packet_order_tests.rs (`a_held_attack_uses_the_sprint_state_from_before_its_input_like_bds`)

### Requirement: Packet application is deterministic
The order in which packets are applied SHALL depend only on the logged packets, their sessions and the player order, and MUST NOT depend on system scheduling or thread timing. Each pass MUST visit players in one fixed order per tick. That order is documented in `packet_order.rs`. Two runs of the same packets in the same order MUST produce the same game state and the same outbound packets.

#### Scenario: Replay is reproducible
- **WHEN** the same sequence of routed packets is replayed twice into identical worlds
- **THEN** the resulting game state and the packets sent are identical

#### Scenario: Two players hit each other in one tick
- **WHEN** two players at one health attack each other and both attacks resolve in the same player pass
- **THEN** exactly one player dies, the one the player order resolves second, and the dead player's held attack is dropped
