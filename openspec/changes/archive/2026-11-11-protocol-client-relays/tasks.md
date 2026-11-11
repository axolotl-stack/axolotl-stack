# Tasks

## 1. Claim

- [x] 1.1 No `[[claim]]`: the change is proposed, applied and archived in one commit; no other claim
  covers the rows

## 2. Measure

- [x] 2.1 `bds-extractor` `!` actions, `--client-data`, `--blob-cache`; outputs kept when the server
  ends the session; jolyne `ClientConfig::blob_cache`
- [x] 2.2 `capture.py` guest scripts and ClientData; `protocol_probe.py` (`relay`, `abilities`,
  `cache`, `skins`, `ddui`) on a locked slot
- [x] 2.3 `commit_protocol_fixtures.py`; fixtures, timelines and README under
  `crates/bds-extractor/captures/1.26.51/protocol/`

## 3. Implement

- [x] 3.1 `roster::skin`: geometry rewrite, persona pieces and tints, colours, base64 fields, PlayFab id
- [x] 3.2 jolyne login validation: BDS's persona piece and tint types
- [x] 3.3 `join::abilities_packet` as BDS for every mode and permission; `AddPlayer` abilities;
  per-player abilities after the full list; the newcomer's abilities to online players; op/deop
  message before the abilities; no abilities on a game mode change
- [x] 3.4 `client_packets`: emote and emote-list relay, blob-status answer, close-reason check,
  ignored serverbound packets

## 4. Record

- [x] 4.1 Parity rows in `docs/parity/protocol.toml` and `systems.toml`; `cargo run -p parity --
  generate` and `-- check`
- [x] 4.2 Quirks in `docs/quirks/bds.md`; solution note; READMEs (`bds-extractor`, `tools/bds-sweep`)

## 5. Validate

- [x] 5.1 `cargo fmt --all -- --check`
- [x] 5.2 `cargo clippy -p unastar -p jolyne -p bds-extractor --all-targets -- -D warnings`
- [x] 5.3 `cargo test -p unastar --lib --tests`, `cargo test -p jolyne --lib`,
  `cargo test -p bds-extractor`, `cargo test -p parity`
- [x] 5.4 `python -m unittest discover -s tools/bds-sweep -p "test_*.py"`
- [x] 5.5 `openspec validate --all --strict`, `bun tools/agents/check-links.ts`
