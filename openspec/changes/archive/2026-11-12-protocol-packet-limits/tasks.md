# Tasks

## 1. Claim

- [x] 1.1 No `[[claim]]`: proposed, applied and archived in one commit; no claim covers the rows

## 2. Measure

- [x] 2.1 Extractor actions `!chat`, `!command-request`, `!blob-status missing=seen`
- [x] 2.2 `protocol_probe.py size`, `book held`, `pick-data`, `cache-miss` on a locked slot
- [x] 2.3 `commit_protocol_fixtures.py --sent-sizes`; commit the fixtures

## 3. Implement

- [x] 3.1 jolyne `RawPacket::payload_len`, stream `decode_borrowed`
- [x] 3.2 `network::packet_limits` (`admit`, `oversized`, `field_violation`) in the network task
- [x] 3.3 `books::InteractText`, `sync_interact_text`
- [x] 3.4 Survival actor picks
- [x] 3.5 Tests against the fixtures

## 4. Record

- [x] 4.1 Parity rows; `parity generate`/`check`
- [x] 4.2 Quirk rows (packet size, string limits, interact text, survival picks, picks with data,
  blob cache, book pages and opening)
- [x] 4.3 Decision 0064 (proposed); solution notes `serverbound-packet-limits`,
  `client-blob-cache` (the blob cache handover)

## 5. Validate

- [x] 5.1 `cargo fmt --all -- --check`
- [x] 5.2 `cargo clippy -p unastar -p jolyne -p bds-extractor --all-targets -- -D warnings`
- [x] 5.3 `cargo test -p unastar --lib --tests`, `cargo test -p jolyne`, `cargo test -p bds-extractor`
- [x] 5.4 `python -m unittest discover -s tools/bds-sweep -p "test_*.py"`
- [x] 5.5 `openspec validate --all --strict`, `bun tools/agents/check-links.ts`
