# server-authority Specification

## Purpose
What `unastar` re-checks on the server instead of trusting the client: how client packets reach game state (`crates/unastar/src/network/mod.rs`, `src/server/runtime.rs`, `src/server/game/packets.rs`), the limits on inbound and outbound traffic, and the checks applied to movement, block breaking and placing, inventory requests, item use, chunk radius and commands. It states our contracts only; which vanilla rules the checks reproduce is tracked in `docs/parity/systems.toml` and `docs/parity/commands.toml`, and odd BDS behaviour we copy is in `docs/quirks/bds.md`. Plugin cancellation of block and chat events is specified in `plugin-host`.

## Requirements

### Requirement: Client packets reach game state only through domain queues
`GameServer::route_packet` SHALL resolve the sending session to its player entity and push each recognised packet onto one domain queue (movement, blocks, inventory, item use, mob actions, chat and commands, chunks, respawn); only the systems that drain those queues change game state. A packet from an unknown session MUST be dropped. `MovePlayer` and every packet the router does not recognise (for example client game-mode or ability requests) MUST be ignored.

#### Scenario: Ignored client movement packet
- **WHEN** a client sends `MovePlayer`
- **THEN** the packet is dropped and the player's position changes only through `PlayerAuthInput`

#### Scenario: Unrouted packet
- **WHEN** a client sends a packet type the router has no arm for
- **THEN** it is logged at debug level and no game state changes

Evidence: crates/unastar/src/server/game/packets.rs (`route_packet`, `route_owned_packet`), crates/unastar/src/server/game/packet_queues.rs

### Requirement: Inbound packets are bounded per session and per tick
Inbound packets SHALL pass through one bounded channel of capacity `max(max_players × 128, 1024)`. A network task that finds the channel full MUST close its own connection. The tick loop MUST apply at most 64 packets per session per tick; further packets from that session in the same tick MUST be dropped with a warning, not deferred.

#### Scenario: Burst over the per-tick budget
- **WHEN** one session delivers 70 packets before a tick drains the channel
- **THEN** the first 64 are routed and the remaining 6 are dropped with the warning `dropping inbound packet over per-tick session budget`

#### Scenario: Full inbound channel
- **WHEN** a network task's `try_send` to the inbound channel fails because it is full
- **THEN** that task logs `Inbound event channel full; closing noisy connection`, closes its connection and reports `Disconnected`

Evidence: crates/unastar/src/server/runtime.rs (`MAX_PACKET_EVENTS_PER_SESSION_PER_TICK`, `NETWORK_EVENTS_PER_PLAYER`, `MIN_NETWORK_EVENT_CAPACITY`, `run`), crates/unastar/src/network/mod.rs (`run_network_loop`)

### Requirement: Outbound packets are bounded per session
Each joined session SHALL have an outbound queue of 1024 packets. `PlayerSession::send` MUST never block the tick: when the queue is full it MUST drop the packet, count it, and warn on the first drop and every hundredth. Queued packets MUST be flushed as one batch on each tick signal, and a packet that terminates the connection MUST end the network loop after it is sent.

#### Scenario: Slow client
- **WHEN** a session's outbound queue is full and the game sends another packet
- **THEN** `send` returns `false`, `packets_dropped` increases and the tick continues

Evidence: crates/unastar/src/entity/components/player.rs (`PlayerSession::send`, `packets_dropped`), crates/unastar/src/server/runtime.rs (`OUTBOUND_CHANNEL_CAPACITY`), crates/unastar/src/network/mod.rs (`run_network_loop`, `terminates_connection`), crates/unastar/src/server/game/mod.rs (test `player_spawn_sets_join_chunk_gate_without_network_io`)

### Requirement: Client-reported movement is accepted within a per-tick delta
The server SHALL take a player's position, rotation and input flags from `PlayerAuthInput`, and MUST reject (leave the player where it was) any update whose position is more than 64 blocks from the server-tracked position. There is no server-side collision or speed simulation; that gap is tracked by the `player.movement` parity row.

#### Scenario: Teleport-sized jump
- **WHEN** a `PlayerAuthInput` reports a position 100 blocks from the tracked one
- **THEN** the update is rejected with the warning `Rejected implausible PlayerAuthInput movement delta` and no move event is produced

Evidence: crates/unastar/src/server/game/movement.rs (`MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK`, `process_auth_input`), [docs/parity/systems.toml](../../../docs/parity/systems.toml) (`player.movement`)

### Requirement: Block interaction requires reach and a building game mode
Every block break action, click and placement SHALL pass `can_interact`: the player has a game mode that can break blocks (survival or creative), a finite server-tracked position, the target is inside the world height in a loaded chunk, and the nearest point of the target block is within 5 blocks of that position (6 in creative). A failed check MUST change nothing and send an `UpdateBlock` with the server's block; a failed click also resends the neighbour and the inventory.

#### Scenario: Out of reach
- **WHEN** a survival player tries to break a block beyond reach
- **THEN** the block is unchanged

#### Scenario: Adventure mode
- **WHEN** an adventure player sends a break for a block within reach
- **THEN** the block is unchanged

Evidence: crates/unastar/src/server/game/blocks.rs (`can_interact`, `correct_block`, `handle_block_click`, tests `distant_break_leaves_block_unchanged`, `adventure_break_leaves_block_unchanged`, `forged_placement_sends_authoritative_block_and_inventory_corrections`), crates/unastar/src/entity/components/player.rs (`GameMode::can_break_blocks`)

### Requirement: Survival breaking is timed by the server
The server SHALL start a break timer from its own break-time computation for the held tool and block. A survival break (from `StopDestroyBlock`, `PredictDestroyBlock` or `CreativeDestroyBlock`) MUST be accepted only for the block being mined and only after at least 90% of the expected ticks; an unbreakable block or air MUST be refused. Creative players break instantly. At most 64 block actions per `PlayerAuthInput` are processed. Break times follow the `player.block_breaking` parity row.

#### Scenario: Skipping the mining time
- **WHEN** a survival player sends `PredictDestroyBlock` for a block immediately after starting to mine it
- **THEN** the block is unchanged and the client receives an `UpdateBlock` correction

Evidence: crates/unastar/src/server/game/blocks.rs (`MAX_BLOCK_ACTIONS`, `process_auth_input_block_actions`, `break_block`, `player_break_ticks`, tests `survival_prediction_cannot_skip_mining_duration`, `break_time_uses_tool_and_ground_state`), crates/unastar/src/entity/components/player.rs (`BreakingState::validate_break`), [docs/parity/systems.toml](../../../docs/parity/systems.toml) (`player.block_breaking`), [docs/quirks/bds.md](../../../docs/quirks/bds.md) (Blocks / mining)

### Requirement: Placement uses the server's held stack
A placement SHALL take the block from the stack the server holds in the selected hotbar slot, never from the item named in the packet, and MUST be refused when that slot is empty, over its stack size, not a block, or when the block would intersect the placing player. In survival an accepted placement MUST consume exactly one item. The world and inventory MUST change only after plugin dispatch, with inventory and game mode re-checked at commit.

#### Scenario: No owned item
- **WHEN** a client sends a placement while its selected slot is empty on the server
- **THEN** no block is placed and corrections are sent

#### Scenario: Replayed commit
- **WHEN** the same accepted survival placement event is committed twice
- **THEN** the block is placed once and exactly one item is consumed

Evidence: crates/unastar/src/server/game/blocks.rs (`held_block`, `commit_block_event`, tests `placement_without_owned_item_is_rejected`, `placing_inside_the_player_is_rejected`, `allowed_survival_place_consumes_exactly_one_and_cannot_replay_over_block`, `deferred_place_revalidates_inventory_and_mode_before_commit`, `placement_waits_for_plugins_before_world_or_inventory_changes`), [docs/parity/systems.toml](../../../docs/parity/systems.toml) (`player.block_placing`)

### Requirement: Item stack requests are validated and atomic
Each `ItemStackRequest` SHALL be applied to a staging copy of the player's containers and committed only if every action succeeds; any failure MUST leave inventories, cursor, crafting state and world drops unchanged and answer only the failure status. Sources and destinations MUST be checked against the server's stacks and the open screen. The answers and status codes match BDS (see the `player.inventory` parity row and the Inventory quirks).

#### Scenario: Late failure rolls back
- **WHEN** a request drops a stack and moves items, and a later action in the same request fails
- **THEN** the failure status has no containers, no drop is staged, no equipment changes and every slot keeps its stack and stack id

#### Scenario: Destroy outside creative
- **WHEN** a survival player sends a `Destroy` action
- **THEN** the request fails with status 60 and the stack is kept

Evidence: crates/unastar/src/server/game/inventory/engine.rs (tests `engine_answers_like_bds_inventory_moves`, `later_action_failure_rolls_back_moves_cache_equipment_and_drops`, `transfer_validation_order_aliases_and_screen_constraints_are_stable`, `destroy_is_creative_screen_gated_and_consume_is_not_a_free_removal`, `unknown_recipe_rejects_and_rolls_back_the_entire_frame`), crates/unastar/src/server/game/inventory/mod.rs (`apply_inventory`, test `failed_grid_frame_rolls_back_input_output_and_all_world_drop_side_effects`), [docs/parity/systems.toml](../../../docs/parity/systems.toml) (`player.inventory`), [docs/quirks/bds.md](../../../docs/quirks/bds.md)

### Requirement: Invalid request ids end the session's inventory input
An `ItemStackRequest` whose client request id is non-negative or even SHALL be a fatal protocol violation: the server MUST answer with a `PacketViolationWarning` of severity terminating-connection, MUST NOT apply that request or any inventory packet queued after it, and the network loop MUST close the connection once the warning is sent.

#### Scenario: Request id 0
- **WHEN** a client sends request id 0 followed by a `MobEquipment` selecting slot 7
- **THEN** a `PacketViolationWarning` is sent, the held slot stays 0 and the inventory is unchanged

Evidence: crates/unastar/src/server/game/inventory/mod.rs (`invalid_request_warning`, `FatalInventoryRequest`, tests `fatal_request_ids_warn_and_stop_queued_inventory_actions_until_transport_closes`, `terminating_warning_matches_retained_bds_positive_id_capture`), crates/unastar/src/server/game/inventory/engine.rs (test `invalid_request_ids_are_fatal_and_cannot_mutate_or_assign_cache`)

### Requirement: Legacy inventory transactions are checked against server stacks
A normal `InventoryTransaction` SHALL only touch the main inventory (container 0), each slot at most once, with every `from_item` equal to the server's stack (prediction ids aside) and every item registered with a valid stack size. Outside creative the total count of each item identity, including damage and NBT, MUST be conserved. Any failure MUST change nothing and resend the inventory.

#### Scenario: Forged creation in survival
- **WHEN** a survival, adventure or spectator player sends a transaction turning an empty slot into 16 items
- **THEN** it is refused and the slot stays empty

#### Scenario: Forged metadata
- **WHEN** a survival player sends a transaction that changes a stack's aux value, block runtime id or user data, or empties the slot
- **THEN** it is refused and the stack keeps its 8 items

Evidence: crates/unastar/src/server/game/inventory/mod.rs (`apply_normal_actions`, `validated_item`, tests `forged_stacks_are_rejected_without_truncation`, `stale_from_item_rejects_entire_transaction`, `survival_moves_conserve_items_and_clear_source`, `unsupported_sources_containers_and_repeated_slots_are_atomic`, `creative_creation_requires_server_creative_mode`, `survival_cannot_change_item_metadata_or_destroy_items`)

### Requirement: Creative item creation requires server-side creative mode
Creating items from the creative menu (`CraftCreative`) SHALL succeed only when the server has the player in creative mode, and the created stack MUST be the server's advertised creative item, not one described by the client.

#### Scenario: Survival creative request
- **WHEN** a survival player sends a creative take of 16 items
- **THEN** the request fails with `Error`, nothing changes, and only the status is sent

Evidence: crates/unastar/src/server/game/inventory/engine.rs (`craft_creative`), crates/unastar/src/server/game/inventory/mod.rs (`creative_stack`, test `rejected_creative_requests_change_nothing_and_send_only_the_status`), crates/unastar/src/server/game/inventory/creative_tests.rs (`creative_menu_take_to_cursor_then_place_full_stack`)

### Requirement: Item use acts on the server's selected stack and clock
Air use (eating, drinking, bows, throwables) SHALL use the item the server holds in the selected hotbar slot and never the client's item description. Eating and drinking MUST finish on the server's tick count once the item's use duration passes; switching slots or releasing MUST cancel it. Food MUST be consumed once per completed use outside creative. Timings follow the Items quirks.

#### Scenario: Server-timed eating
- **WHEN** a hungry player starts eating and sends nothing more
- **THEN** the server finishes eating after the use duration and consumes one item

#### Scenario: Cancelled by slot change
- **WHEN** the player selects another hotbar slot while eating
- **THEN** eating stops and nothing is consumed

Evidence: crates/unastar/src/server/game/item_use/mod.rs, crates/unastar/src/server/game/item_use/tests.rs (`the_server_finishes_eating_after_the_use_duration`, `an_early_consume_finishes_at_once`, `switching_slots_or_releasing_cancels_eating`, `creative_players_eat_when_full_without_using_up_the_item`), [docs/quirks/bds.md](../../../docs/quirks/bds.md) (Items / eating)

### Requirement: Command permission is decided by the server
Player commands SHALL run with permission `any`, or `gamedirectors` when the player is an operator; console lines MUST run as `owner`. A command whose captured permission level is above the source's MUST answer `commands.generic.error.permissions` and MUST NOT run. Operator status MUST come from `server.operators` (display name, case-insensitive, or XUID) and `/op`; never from the client. Chat MUST carry the server's name for the sender. See the Commands quirks.

#### Scenario: Member runs an operator command
- **WHEN** a non-operator player runs `/summon minecraft:cow 6 70 9`
- **THEN** the answer is `commands.generic.error.permissions` naming `summon` and no cow spawns

#### Scenario: Configured operator
- **WHEN** players named `mobbot` and with XUID `2535400000000042` join a server whose operators are `MobBot` and `2535400000000042`
- **THEN** both are operators and players matching neither are not

Evidence: crates/unastar/src/command/mod.rs (`dispatch`), crates/unastar/src/command/context.rs (`player_permission`, `CommandSource::server`), crates/unastar/src/command/exec/op.rs (`OperatorList::grants`, test `operators_cannot_op_like_bds`), crates/unastar/src/server/game/commands.rs (`handle_text`, tests `members_get_the_bds_permission_error_for_operator_commands`, `console_lines_run_with_server_permission`), crates/unastar/src/server/game/mod.rs (test `configured_operators_are_granted_on_join_by_name_or_xuid`), [docs/parity/commands.toml](../../../docs/parity/commands.toml) (`teleport`), [docs/quirks/bds.md](../../../docs/quirks/bds.md) (Commands / permission)

### Requirement: Chunk radius requests are clamped
A `RequestChunkRadius` after join SHALL be granted `min(requested, server.max_chunk_radius, client max)`, with each term raised to at least 1, and answered with `ChunkRadiusUpdated` carrying the granted radius.

#### Scenario: Oversized request
- **WHEN** a client asks for radius 64 on a server whose `max_chunk_radius` is 12
- **THEN** the player's chunk radius becomes 12 and `ChunkRadiusUpdated` says 12

Evidence: crates/unastar/src/server/game/chunks.rs (`handle_chunk_radius_request`)

### Requirement: Command lines cannot exhaust the stack
Parsing a command line and running it SHALL NOT recurse per subcommand or per nested command: the parser MUST check a line of any length, nested `run` lines included, with a stack of its own, and `/execute` MUST run its chain and the `/execute` lines it runs on a stack of its own. A chain whose contexts would run through more than 640 subcommands, counting those of the `/execute` lines it runs, MUST answer one failure and nothing else. See decision 0058.

#### Scenario: A chain past the limit
- **WHEN** the console runs `execute` followed by 641 nested `execute run` and `testforblock 0 -64 0 bedrock`
- **THEN** the only answer is the unastar failure for chains deeper than 640 subcommands, and the server keeps running

#### Scenario: A long line that does not parse
- **WHEN** the console runs `execute` with 2000 `positioned ~ ~ ~` subcommands ending in `bogus`
- **THEN** the answer is `commands.generic.syntax` at `bogus`, as BDS 1.26.51 answers

Evidence: crates/unastar/src/command/parser.rs (`check_tails`), crates/unastar/src/command/exec/execute.rs (`run`, `MAX_DEPTH`, tests `long_chains_answer_like_bds`, `the_limit_counts_every_subcommand`), crates/unastar/src/command/exec/testdata/bds-command-probe.json (`execute_depth`), [decision 0058](../../../docs/decisions/0058-refuse-execute-chains-deeper-than-bds-survives.md)
