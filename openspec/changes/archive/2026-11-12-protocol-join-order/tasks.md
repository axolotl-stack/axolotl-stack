# Tasks

## 1. Claim

- [x] 1.1 No `[[claim]]`: the change is proposed, applied and archived in one commit, and a claim
  naming an archived change fails `parity check`; no other claim covers the rows

## 2. Implement

- [x] 2.1 jolyne `WorldJoinParams::before_start_game`, queued first in the `StartGame` batch; test
  `game_packets_can_precede_start_game_in_its_batch`
- [x] 2.2 unastar `OnlineRoster` and `Reservation`; the connection task reserves and sends
  `roster::before_start_game`; spawn takes the reserved colour; despawn frees the place
- [x] 2.3 Tests: the three packets byte for byte against member join `00011`-`00013`; reservations;
  the join/leave announcement test in the new order

## 3. Record

- [x] 3.1 Rewrite decision 0044 as accepted (owner, 2026-11-11) and its index row
- [x] 3.2 Parity rows `PlayerListPacket`, `LevelEventGenericPacket`, `StartGamePacket`,
  `SyncWorldClocksPacket`; `cargo run -p parity -- generate` and `-- check`
- [x] 3.3 Quirk row "Join / player list"

## 4. Validate

- [x] 4.1 `cargo fmt --all -- --check`
- [x] 4.2 `cargo clippy -p unastar -p jolyne --all-targets -- -D warnings`, jolyne feature
  combinations from `.github/workflows/test.yml`
- [x] 4.3 `cargo test -p jolyne`, `cargo test -p unastar --lib --tests`
- [x] 4.4 `openspec validate --all --strict`, `bun tools/agents/check-links.ts`
