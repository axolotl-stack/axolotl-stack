## ADDED Requirements

### Requirement: Terrain stays within the transport's queue
Chunk streaming SHALL keep the bytes the transport reports queued towards a player, plus sub-chunk answers queued within the last 45 ms, within 512 KiB, half of the RakNet session's 1 MiB reliable-queue limit. A sub-chunk request MUST be answered column by column while its answers fit (one answer always goes out when nothing is queued); the rest MUST wait for a later tick in request order. No new column MUST be sent while the budget is full.

#### Scenario: Respawn into a stored view
- **WHEN** a player respawns or teleports 3000 blocks into a stored radius-12 view
- **THEN** the view loads and the session's queue never passes its 1 MiB limit, so the player is not disconnected with `QueueTooLong`

#### Scenario: Answers wait for the transport
- **WHEN** the transport reports half the budget queued and an answer of more than the other half is due
- **THEN** that answer waits; once the transport reports the queue drained, it goes out

Evidence: crates/unastar/src/world/ecs/streaming.rs (`SendWindow::may_answer`, `SendWindow::may_send`, tests `answers_wait_while_the_transport_holds_the_byte_budget`, `no_new_columns_while_the_transport_is_full`), crates/unastar/src/server/game/chunks.rs (`handle_subchunk_request`), crates/unastar/tests/stream_respawn.rs (`respawn_and_teleport_into_a_stored_view_stay_under_the_session_queue_limit`), [docs/quirks/bds.md](../../../../../../docs/quirks/bds.md) (World / chunk streaming)

### Requirement: Chunk sends are clocked by acknowledgements
Chunk streaming SHALL apply no per-tick count. Each player MUST have a send window of `LevelChunk`s not yet acknowledged by an answered `SubChunkRequest`: it starts at 128, grows by one per acknowledgement within 400 ms up to 256, halves (at most once per 400 ms, never below 8) when acknowledgements come later, and stops counting a column after 3 s. A single `SubChunk` packet MUST carry about 128 KiB of answers at most.

#### Scenario: Window growth and back-off
- **WHEN** acknowledgements arrive within 400 ms, and later they arrive after it
- **THEN** the window grows by one per acknowledgement up to 256, then halves once per 400 ms down to 8

#### Scenario: Large answers
- **WHEN** one request names many dense columns
- **THEN** the answer is split between columns into several `SubChunk` packets

Evidence: crates/unastar/src/world/ecs/streaming.rs (`SendWindow`, tests `prompt_acks_grow_the_window_by_one_each_up_to_the_max`, `late_acks_halve_the_window_once_per_interval_and_never_below_min`, `unacknowledged_columns_stop_counting_after_the_timeout`), crates/unastar/src/server/game/chunks.rs (`SUBCHUNK_ANSWER_BYTES`, test `large_answers_split_between_columns`), crates/unastar/src/server/game/mod.rs (test `the_view_streams_within_the_acknowledged_send_window`)
