# Tasks

## 1. Evidence and model (this commit)

- [x] 1.1 `order_probe`: three scripted players in one process, script-controlled order and batching within a tick (`crates/bds-sweep/src/bin/order_probe.rs`)
- [x] 1.2 Race matrix captures on BDS 1.26.51 with repeats; outcome distributions; stalls excluded (`tools/bds-sweep/pvp_race*.py`)
- [x] 1.3 Fixtures `crates/unastar/src/server/game/testdata/pvp-race-*.jsonl` (one trial per variant, all trials' outcomes in the header)
- [x] 1.4 Model `crates/unastar/src/server/game/packet_order.rs` with tests that replay the captures (`packet_order_tests.rs`)
- [x] 1.5 Benchmark `packet_order_bench.rs` (ignored test) and numbers in design.md
- [x] 1.6 Quirk rows (`docs/quirks/bds.md`), decision 0045 (proposed), solution notes

## 2. Wire the passes (after the movement, items, blocks and entities lanes merge)

- [ ] 2.1 `ClientPacketLog` resource; `route_packet` appends instead of pushing to domain queues
- [ ] 2.2 Handlers over `&mut World` for movement, block actions, inventory actions, item use and throws, mob actions, the PvP hit (0046), chat and commands, respawn
- [ ] 2.3 `apply_client_packets` (exclusive, `PacketApplySet`): network pass with `PlayerQueue`, then player pass in the measured player order
- [ ] 2.4 Schedule: replace the domain systems and their edges in `server/game/mod.rs`; remove `consolidate_domain_events`
- [ ] 2.5 `network/inbound.rs`: visit sessions in the player order instead of rotating
- [ ] 2.6 Remove `CommandOrder`, `run_early_commands`, `applied_before_commands`; `command_order_tests.rs` stays green
- [ ] 2.7 Un-ignore the end-to-end replays of every `pvp-race-*` fixture through `GameServer::route_packet` (needs the PvP hit from 0046)
- [ ] 2.8 Update `docs/parity/systems.toml` (`player.combat`, `player.movement` tests, with their owners) and run `cargo run -p parity -- generate` and `-- check`

## 3. Validate

- [ ] 3.1 `cargo fmt --all -- --check`
- [ ] 3.2 `cargo clippy -p unastar --all-targets -- -D warnings`
- [ ] 3.3 `cargo test -p unastar --lib --tests`
- [ ] 3.4 `cargo test -p unastar --release --lib packet_order_bench -- --ignored --nocapture`; the packet stage stays within the design's budget
- [ ] 3.5 `cargo run -p parity -- check`; `openspec validate --all --strict`; `bun tools/agents/check-links.ts`
- [ ] 3.6 `review-change`; the owner accepts 0045; archive into `server-authority`
