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

### Requirement: Player movement is simulated and judged by the server
The server SHALL keep its own copy of each player's movement, simulate every `PlayerAuthInput` on it and judge the reported position with the acceptance rules in `docs/quirks/bds.md` (Entities / player movement authority, pull, horizontal take), answering a refused report with `CorrectPlayerMovePrediction` carrying the copy's state after that input. The copy MUST be the player's position for viewers, falls, `MOVING` and exhaustion. An input with a non-finite value MUST be rejected whole.

#### Scenario: Teleport-sized jump
- **WHEN** a `PlayerAuthInput` reports a position 100 blocks from the server's copy of a standing player
- **THEN** the player stays where the copy is and the client is corrected to it

#### Scenario: Too fast
- **WHEN** a client walking forward reports positions 0.4 blocks a tick ahead (authority capture phase `speed`)
- **THEN** the copy walks at walking speed and the client is corrected on the inputs BDS corrected, to BDS's position and velocity

#### Scenario: Through a wall
- **WHEN** a client reports moving 0.2 blocks a tick through a stone wall (phase `phase_wall`)
- **THEN** the copy stops at the wall and the client is corrected as BDS corrected it

#### Scenario: Claiming the ground while falling
- **WHEN** a client falling 10 blocks reports vertical collision on every input (phase `nofall`)
- **THEN** the copy falls, lands and deals the 7 damage BDS dealt

#### Scenario: A close position is taken
- **WHEN** a standing client reports a position 0.09 blocks from the copy (phase `offset_z_0_09`)
- **THEN** the copy takes that position and no correction is sent

#### Scenario: Non-finite input
- **WHEN** a `PlayerAuthInput` carries a NaN position or rotation
- **THEN** it is rejected whole with the warning `Rejected non-finite PlayerAuthInput` and the player does not move

Evidence: crates/unastar-rules/src/movement/mod.rs (`step`), crates/unastar-rules/src/movement/authority.rs (`Authority::judge`), crates/unastar/src/entity/player_life/authority.rs (`PlayerMotion::input`), crates/unastar/src/server/game/movement.rs (`process_auth_input`, test `non_finite_positions_and_rotations_are_rejected`), crates/unastar/src/entity/player_life/tests/authority.rs (`the_copy_moves_and_corrects_like_bds`, `close_positions_bds_keeps_are_taken`, `cheated_falls_hurt_like_bds`, `a_teleport_sized_jump_is_corrected`), crates/unastar/src/entity/player_life/testdata/bds-1.26.51-authority.json

### Requirement: A server teleport is enforced on the next input
When the server moves a player (a command, a plugin, an ender pearl or chorus fruit), it SHALL NOT simulate the player's next input: the copy MUST stay at rest at the destination, and a reported position beyond the acceptance threshold MUST be corrected to it, whether or not the input says `HandledTeleport` and even when that input would otherwise be trusted. A respawn MUST NOT count as a teleport: the next input is simulated from the spawn point.

#### Scenario: An input sent before the teleport
- **WHEN** `/tp` moves a player 8 blocks and its next input still reports the old position
- **THEN** that input is corrected to the destination with zero velocity and on the ground, and the player stays at the destination

#### Scenario: A client that never follows
- **WHEN** a client ignores a teleport and every correction (phase `tp_ignore_uncorrected`)
- **THEN** the copy stays at the destination and the client is corrected on the inputs BDS corrected

#### Scenario: A respawn the client ignores
- **WHEN** a respawned client's next input still reports where it died
- **THEN** that input is corrected to the copy simulated from the spawn point through that input

Evidence: crates/unastar/src/entity/player_life/authority.rs (`PlayerMotion::teleported`, `PlayerMotion::respawned`), crates/unastar/src/command/exec/teleport.rs, crates/unastar/src/server/game/item_use/mod.rs (`teleport`), crates/unastar/src/server/game/plugins.rs, crates/unastar/src/entity/player_life/respawn.rs, crates/unastar/src/server/game/command_tests.rs (`a_teleport_cannot_be_undone_by_the_input_already_on_its_way`), crates/unastar/src/entity/player_life/tests/authority.rs (`the_copy_moves_and_corrects_like_bds`), crates/unastar/src/entity/player_life/tests/respawn.rs (`the_copy_of_the_player_respawns_with_it`)

### Requirement: Movement the simulation does not cover is trusted
The server SHALL take a client's state without judging it where the simulation does not reproduce vanilla: flight in a game mode that can fly, slowness or levitation, nearby blocks with rules of their own, movement enchantments where they act, a riptide spin started in water, a crawl space, a client only slower than the copy's simulation of its keys (not after a knockback), or the first input after a join or respawn near the spawn point. Each trust MUST be bounded by the move it allows.

#### Scenario: Replayed vanilla movement
- **WHEN** the BDS movement capture's phases (walking, sprinting, sneaking, jumps, swims, glides, falls onto slime, beds, hay, water and powder snow, fire, lava, knockback) are replayed
- **THEN** the server sends no correction BDS did not send, and every correction it sends carries BDS's position and velocity

#### Scenario: Flying
- **WHEN** a flying creative player reports a position 10 blocks above the copy
- **THEN** the copy takes it and no correction is sent

#### Scenario: Teleport-sized jump while flying
- **WHEN** a flying creative player reports a position 100 blocks from the copy, twice
- **THEN** both inputs are rejected with the warning `Rejected implausible PlayerAuthInput movement delta`, the copy stays where it was and no correction is sent

#### Scenario: Claimed flight
- **WHEN** a survival client sends `StartFlying` with a position 5 blocks above the copy
- **THEN** the flag is ignored and the input is judged and corrected

#### Scenario: Situations the simulation does not cover
- **WHEN** a standing player sends an input under slowness or levitation, sneaking in swift sneak leggings, next to water in depth strider boots, on soul soil in soul speed boots, starting a riptide spin next to water, under a block at eye height or next to scaffolding
- **THEN** the input is trusted and no correction is sent

#### Scenario: A slower client
- **WHEN** a client eats while it walks, or holds the sprint key at 6 food, and so moves slower than the copy's simulation of its keys
- **THEN** each such input is taken and no correction is sent

#### Scenario: A knockback refused
- **WHEN** a client stands still after the server knocks it back
- **THEN** its inputs are judged, not taken as slower, and it is corrected

#### Scenario: Out of a near trust's reach
- **WHEN** a client next to a ladder reports a position 10 blocks away or inside the wall beside it, or claims a crawl space 5 blocks away
- **THEN** the input is judged and corrected

#### Scenario: A bounded velocity
- **WHEN** a trusted input reports a velocity of 1e30 blocks a tick
- **THEN** the copy's velocity is 4 blocks a tick on that axis

#### Scenario: Judging resumes where the client is
- **WHEN** a client starts sprinting during trusted inputs and keeps sprinting after they end
- **THEN** the copy follows it exactly, at sprint speed, without a correction

Evidence: crates/unastar/src/entity/player_life/authority.rs (`MotionTable`, `Trust`, `Situation`, `PlayerMotion::input`), crates/unastar/src/server/game/movement.rs (`MAX_AUTH_INPUT_MOVE_DELTA_PER_TICK`, `situation_of`, test `moves_within_the_delta_are_accepted_and_larger_ones_rejected`), crates/unastar/src/entity/player_life/tests/authority.rs (`flight_is_trusted_within_64_blocks`, `a_trusted_velocity_is_bounded`, `a_survival_client_cannot_fly`, `situations_the_simulation_does_not_cover_are_trusted`, `a_slower_client_is_taken`, `a_knockback_must_be_followed`, `near_trusts_do_not_reach_far`, `a_riptide_spin_is_trusted_for_twenty_inputs`, `a_sprint_goes_on_after_trusted_inputs`, `waterlogged_blocks_are_water_and_flowing_water_is_trusted`), crates/unastar/src/entity/player_life/tests/movement.rs (`assert_corrections_like_bds`), [docs/parity/systems.toml](../../../docs/parity/systems.toml) (`player.movement`)

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
