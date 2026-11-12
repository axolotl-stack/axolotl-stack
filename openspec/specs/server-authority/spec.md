# server-authority Specification

## Purpose
What `unastar` re-checks on the server instead of trusting the client: how client packets reach game state (`crates/unastar/src/network/mod.rs`, `src/network/inbound.rs`, `src/server/runtime.rs`, `src/server/game/packets.rs`), the limits on inbound and outbound traffic, and the checks applied to movement, block breaking and placing, inventory requests, item use, chunk radius and commands. It states our contracts only; which vanilla rules the checks reproduce is tracked in `docs/parity/systems.toml` and `docs/parity/commands.toml`, and odd BDS behaviour we copy is in `docs/quirks/bds.md`. Plugin cancellation of block and chat events is specified in `plugin-host`.

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
Each joined session SHALL have its own inbound queue of 512 packets. The tick loop MUST route at most 64 packets per session per tick, in arrival order, and MUST defer the rest to later ticks, never drop them. The session routed first MUST rotate each tick. A network task whose own queue is full MUST send `Disconnect` (`Kicked`, "Too many packets.") and close only its own connection.

#### Scenario: Burst over the per-tick budget
- **WHEN** one session delivers 70 packets before a tick
- **THEN** the first 64 are routed that tick and the remaining 6 are routed, in order, on the next tick

#### Scenario: Each session has its own budget and queue
- **WHEN** one session's queue is full and another session sends a packet
- **THEN** only the full session's network task closes its connection, and the other session's packet is routed

#### Scenario: Rotating first session
- **WHEN** two sessions both have queued packets on consecutive ticks
- **THEN** the session routed first alternates between them

#### Scenario: Flooding client
- **WHEN** a client sends more packets than its queue holds before the tick loop drains it
- **THEN** the server logs `Inbound packet queue full; closing noisy connection` and the client receives a `Disconnect` with reason `Kicked` and message "Too many packets."

Evidence: crates/unastar/src/network/inbound.rs (`INBOUND_PACKETS_PER_SESSION`, `MAX_PACKETS_PER_SESSION_PER_TICK`, `InboundQueues::drain_tick`, tests `a_burst_over_the_tick_budget_is_deferred_in_order_not_dropped`, `each_session_gets_its_own_budget`, `the_first_session_of_a_tick_rotates`, `a_full_queue_belongs_to_the_flooding_session_only`), crates/unastar/src/network/mod.rs (`run_network_loop`, `FLOOD_DISCONNECT_MESSAGE`), crates/unastar/tests/flood_loopback.rs (`a_flooding_client_is_disconnected_with_a_reason`)

### Requirement: A leaving session is saved after its last routed packets
When a session's network task ends, the tick SHALL route that session's packets within the tick's budget, drop whatever it still has queued (logging the count at debug), run the game tick, and only then save (when `players.save_on_disconnect`) and despawn the player. A client `Disconnect` packet MUST NOT despawn the player itself; the leave path above saves and despawns it.

#### Scenario: Leaver with a queued backlog
- **WHEN** a session with more than 64 queued packets disconnects
- **THEN** its first 64 packets are applied by the game tick and are part of its save, the rest are dropped, and the player is despawned

#### Scenario: Client Disconnect packet
- **WHEN** a client sends a `Disconnect` packet and its connection ends
- **THEN** the player is saved and then despawned

Evidence: crates/unastar/src/server/runtime.rs (`network_tick`, tests `a_leaving_session_is_saved_after_its_budgeted_packets_and_the_rest_are_dropped`, `a_client_disconnect_packet_still_saves_the_player`), crates/unastar/src/network/inbound.rs (`InboundQueues::close`, test `closing_a_session_drops_its_remaining_packets_then_forgets_it`), crates/unastar/src/server/game/packets.rs (`route_packet`)

### Requirement: Outbound packets are bounded per session
Each joined session SHALL have an outbound queue of 1024 packets. `PlayerSession::send` MUST never block the tick: when the queue is full it MUST drop the packet, count it, and warn on the first drop and every hundredth. Queued packets MUST be flushed as one batch on each tick signal, and a packet that terminates the connection MUST end the network loop after it is sent.

#### Scenario: Slow client
- **WHEN** a session's outbound queue is full and the game sends another packet
- **THEN** `send` returns `false`, `packets_dropped` increases and the tick continues

Evidence: crates/unastar/src/entity/components/player.rs (`PlayerSession::send`, `packets_dropped`), crates/unastar/src/server/runtime.rs (`OUTBOUND_CHANNEL_CAPACITY`), crates/unastar/src/network/mod.rs (`run_network_loop`, `terminates_connection`), crates/unastar/src/server/game/mod.rs (test `player_spawn_sets_join_chunk_gate_without_network_io`)

### Requirement: Client-reported movement is accepted within a per-tick delta
The server SHALL take a player's position, rotation and input flags from `PlayerAuthInput`, and MUST reject whole (leave the player where it was and forward none of its block actions) any update whose position, rotation, head yaw or move vector is not finite, or whose position is more than 64 blocks from the server-tracked position. Rotation is not range-checked. There is no server-side collision or speed simulation; see the `player.movement` parity row.

#### Scenario: Teleport-sized jump
- **WHEN** a `PlayerAuthInput` reports a position 100 blocks from the tracked one
- **THEN** the update is rejected with the warning `Rejected implausible PlayerAuthInput movement delta`, no move event is produced and its block actions are not processed

#### Scenario: Non-finite input
- **WHEN** a `PlayerAuthInput` carries a NaN position or an infinite rotation
- **THEN** the update is rejected with the warning `Rejected non-finite PlayerAuthInput` and the stored position and rotation are unchanged

Evidence: crates/unastar/src/server/game/movement.rs (`MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK`, `apply_movement`, `process_auth_input`, `auth_input_is_finite`, tests `moves_within_the_delta_are_accepted_and_larger_ones_rejected`, `non_finite_positions_and_rotations_are_rejected`), [docs/parity/systems.toml](../../../docs/parity/systems.toml) (`player.movement`)

### Requirement: Block interaction requires reach and a building game mode
Every block break action, click and placement SHALL pass `can_interact`: the player has a game mode that can break blocks (survival or creative), a finite server-tracked position, the target is inside the world height in a loaded chunk, and the nearest point of the target block is within 5 blocks of that position (6 in creative). A failed check MUST change nothing and send an `UpdateBlock` with the server's block; a failed click also resends the neighbour and the inventory.

#### Scenario: Out of reach
- **WHEN** a survival player tries to break a block beyond reach
- **THEN** the block is unchanged

#### Scenario: Adventure mode
- **WHEN** an adventure player sends a break for a block within reach
- **THEN** the block is unchanged

Evidence: crates/unastar/src/server/game/blocks.rs (`can_interact`, `correct_block`, `handle_block_click`, tests `distant_break_leaves_block_unchanged`, `adventure_break_leaves_block_unchanged`, `forged_placement_sends_authoritative_block_and_inventory_corrections`), crates/unastar/src/entity/components/player.rs (`GameMode::can_break_blocks`)

### Requirement: Survival breaking advances once per input on the server
The server SHALL advance each survival break's progress by its own per-input break fraction for the held item, block and player state, once per accepted `PlayerAuthInput` of the breaking player (idle inputs too), never on a tick without one. The block MUST break only when that progress completes, by the server itself; an earlier `PredictDestroyBlock` MUST get `UpdateBlock` corrections and not end the break. Creative players break instantly. At most 64 block actions per input are processed.

#### Scenario: Skipping the mining time
- **WHEN** a survival player sends `PredictDestroyBlock` on the 8th input of a 15-input break
- **THEN** the block is unchanged, the client receives `UpdateBlock` corrections, and the block still breaks on the 15th input

#### Scenario: The server completes the break
- **WHEN** a survival player breaking dirt by hand keeps sending inputs without predicting the break
- **THEN** the block breaks on the 15th input

#### Scenario: A frozen client
- **WHEN** a breaking player sends no `PlayerAuthInput` for ten server ticks
- **THEN** the break does not advance during those ticks and still completes on its 15th input

Evidence: crates/unastar/src/server/game/blocks.rs (`MAX_BLOCK_ACTIONS`, `process_auth_input_block_actions`, `advance_breaking`, `refuse_prediction`, `break_block`, `player_break_ticks`, tests `survival_breaks_advance_per_input_and_refuse_early_predictions`, `break_time_uses_tool_and_ground_state`, `block_actions_past_the_per_input_limit_are_ignored`), crates/unastar/src/server/game/blocks/breaking_tests.rs (`survival_breaking_answers_every_input_like_bds`, replaying `testdata/bds-breaking.jsonl`), crates/unastar/src/entity/components/player.rs (`BreakingState`), crates/unastar/src/server/game/movement.rs (`apply_movement`), [docs/parity/systems.toml](../../../docs/parity/systems.toml) (`player.block_breaking`), [docs/quirks/bds.md](../../../docs/quirks/bds.md) (Blocks / mining: the packets that answer each input, and when a break ends)

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
A normal `InventoryTransaction` SHALL only touch the main inventory (container 0), each slot at most once, with every `from_item` equal to the server's stack (prediction ids aside) and every item registered with a valid stack size. Outside creative the total count of each item identity, including damage and NBT, MUST be conserved. Any failure MUST change nothing and resend the inventory. Received transactions MUST be logged at trace level only.

#### Scenario: Forged creation in survival
- **WHEN** a survival, adventure or spectator player sends a transaction turning an empty slot into 16 items
- **THEN** it is refused and the slot stays empty

#### Scenario: Forged metadata
- **WHEN** a survival player sends a transaction that changes a stack's aux value, block runtime id or user data, or empties the slot
- **THEN** it is refused and the stack keeps its 8 items

#### Scenario: Transaction logging
- **WHEN** a client sends any `InventoryTransaction`
- **THEN** no `info`-level event is logged for it, so clients cannot drive the default log volume

Evidence: crates/unastar/src/server/game/inventory/mod.rs (`apply_normal_actions`, `validated_item`, tests `forged_stacks_are_rejected_without_truncation`, `stale_from_item_rejects_entire_transaction`, `survival_moves_conserve_items_and_clear_source`, `unsupported_sources_containers_and_repeated_slots_are_atomic`, `creative_creation_requires_server_creative_mode`, `survival_cannot_change_item_metadata_or_destroy_items`, `inventory_transactions_are_not_logged_at_info`)

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

Evidence: crates/unastar/src/server/game/chunks.rs (`handle_chunk_radius_request`, `granted_chunk_radius`, test `chunk_radius_requests_are_clamped`)

### Requirement: Survival client actions cannot break a block
In survival only the server's completed progress SHALL break a block, and never air or an unbreakable block. A survival player's `CreativeDestroyBlock` MUST NOT break a block. `StopDestroyBlock` and `AbortDestroyBlock` MUST NOT break a block. A breaking action whose face is outside 0 to 5 MUST be ignored.

#### Scenario: A creative destroy from survival
- **WHEN** a survival player sends `CreativeDestroyBlock` for a block within reach
- **THEN** the block is unchanged

#### Scenario: A released break
- **WHEN** a survival player sends `StopDestroyBlock` on the 6th input of a 15-input break
- **THEN** the block is unchanged on that input

#### Scenario: An aborted break
- **WHEN** a survival player aborts a break after five inputs and keeps sending inputs
- **THEN** the block is unchanged

#### Scenario: No valid face
- **WHEN** a survival player sends `StartDestroyBlock` with face -602
- **THEN** no break starts and no event is sent for it

Evidence: crates/unastar/src/server/game/blocks.rs (`process_auth_input_block_actions`, `break_block`, tests `survival_creative_destroy_is_refused`, `breaking_actions_without_a_valid_face_are_ignored`), crates/unastar/src/server/game/blocks/breaking_tests.rs (`survival_breaking_answers_every_input_like_bds`: the abort and release scenarios)

### Requirement: Inputs that arrive together are applied together
A player's `PlayerAuthInput`s that arrive in the same tick SHALL each advance the break, in arrival order, within that tick; the server does not hold a client to server time. A player's block actions after an input that completed a break MUST wait for the next tick, when that break has been committed. At most 64 actions MAY wait per player; more MUST be dropped.

#### Scenario: A burst of inputs
- **WHEN** a survival player breaking dirt by hand sends its 2nd to 16th inputs at once
- **THEN** the block breaks on the 15th input without waiting for further ticks, and the 16th input finds air

Evidence: crates/unastar/src/server/game/blocks.rs (`apply_block_actions`, `MAX_WAITING_BLOCK_ACTIONS`), crates/unastar/src/server/game/blocks/breaking_tests.rs (`survival_breaking_answers_every_input_like_bds`: the burst scenario), [docs/quirks/bds.md](../../../docs/quirks/bds.md) (Blocks / mining: inputs are not held to server time)

### Requirement: Terrain stays within the transport's queue
Chunk streaming SHALL keep the bytes the transport reports queued towards a player, plus sub-chunk answers queued within the last 45 ms, within 512 KiB, half of the RakNet session's 1 MiB reliable-queue limit. A sub-chunk request MUST be answered column by column while its answers fit (one answer always goes out when nothing is queued); the rest MUST wait for a later tick in request order. No new column MUST be sent while the budget is full.

#### Scenario: Respawn into a stored view
- **WHEN** a player respawns or teleports 3000 blocks into a stored radius-12 view
- **THEN** the view loads and the session's queue never passes its 1 MiB limit, so the player is not disconnected with `QueueTooLong`

#### Scenario: Answers wait for the transport
- **WHEN** the transport reports half the budget queued and an answer of more than the other half is due
- **THEN** that answer waits; once the transport reports the queue drained, it goes out

Evidence: crates/unastar/src/world/ecs/streaming.rs (`SendWindow::may_answer`, `SendWindow::may_send`, tests `answers_wait_while_the_transport_holds_the_byte_budget`, `no_new_columns_while_the_transport_is_full`), crates/unastar/src/server/game/chunks.rs (`handle_subchunk_request`), crates/unastar/tests/stream_respawn.rs (`respawn_and_teleport_into_a_stored_view_stay_under_the_session_queue_limit`), [docs/quirks/bds.md](../../../docs/quirks/bds.md) (World / chunk streaming)

### Requirement: Chunk sends are clocked by acknowledgements
Chunk streaming SHALL apply no per-tick count. Each player MUST have a send window of `LevelChunk`s not yet acknowledged by an answered `SubChunkRequest`: it starts at 128, grows by one per acknowledgement within 400 ms up to 256, halves (at most once per 400 ms, never below 8) when acknowledgements come later, and stops counting a column after 3 s. A single `SubChunk` packet MUST carry about 128 KiB of answers at most.

#### Scenario: Window growth and back-off
- **WHEN** acknowledgements arrive within 400 ms, and later they arrive after it
- **THEN** the window grows by one per acknowledgement up to 256, then halves once per 400 ms down to 8

#### Scenario: Large answers
- **WHEN** one request names many dense columns
- **THEN** the answer is split between columns into several `SubChunk` packets

Evidence: crates/unastar/src/world/ecs/streaming.rs (`SendWindow`, tests `prompt_acks_grow_the_window_by_one_each_up_to_the_max`, `late_acks_halve_the_window_once_per_interval_and_never_below_min`, `unacknowledged_columns_stop_counting_after_the_timeout`), crates/unastar/src/server/game/chunks.rs (`SUBCHUNK_ANSWER_BYTES`, test `large_answers_split_between_columns`), crates/unastar/src/server/game/mod.rs (test `the_view_streams_within_the_acknowledged_send_window`)
