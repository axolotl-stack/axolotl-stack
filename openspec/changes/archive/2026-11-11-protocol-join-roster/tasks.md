# Tasks

## 1. Claim

- [x] 1.1 No `[[claim]]`: the change is proposed, applied and archived in one commit, and a claim
  naming an archived change fails `parity check`; no other claim covers the rows

## 2. Measure

- [x] 2.1 `capture.py --pair` (`--guests`, `--guest-gap`), unit tests in `test_bds_sweep.py`
- [x] 2.2 Pair runs on a locked slot: thunder, rain, 7/12/20 guests; `roster_probe.py` colour reuse
- [x] 2.3 `commit_roster_fixtures.py`; fixtures and README under `crates/bds-extractor/captures/1.26.51/roster/`

## 3. Implement

- [x] 3.1 Vendor `CameraPresets`, `CameraAimAssistPresets`, `TrimData`; test against the member join
- [x] 3.2 `server::game::roster`: entries from the login, colours, sleeping-player event, adventure settings
- [x] 3.3 Join packets in BDS order; `SetSpawnPosition`, `PlayerFog`, `CameraSpline`; no join `SetPlayerGameType` (game mode in `StartGame`)
- [x] 3.4 Join and leave announcements to online players, before `AddPlayer`
- [x] 3.5 `SetHealth` before `PlayStatus(PlayerSpawn)`

## 4. Record

- [x] 4.1 Parity rows in `docs/parity/protocol.toml`; `cargo run -p parity -- generate` and `-- check`
- [x] 4.2 Quirks in `docs/quirks/bds.md`; solution note; decision 0044 (proposed)

## 5. Validate

- [x] 5.1 `cargo fmt --all -- --check`
- [x] 5.2 `cargo clippy -p unastar --all-targets -- -D warnings`
- [x] 5.3 `cargo test -p unastar --lib --tests`, `cargo test -p parity`
- [x] 5.4 `python -m unittest discover -s tools/bds-sweep -p "test_*.py"`
- [x] 5.5 `openspec validate --all --strict`, `bun tools/agents/check-links.ts`
