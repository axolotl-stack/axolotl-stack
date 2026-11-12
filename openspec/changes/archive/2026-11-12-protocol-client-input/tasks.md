# Tasks

## 1. Claim

- [x] 1.1 No `[[claim]]`: proposed, applied and archived in one commit; no claim covers the row

## 2. Measure

- [x] 2.1 bds-extractor `--input`, `!equip`, `!use-item`, `!pick-block`, `--dump`; unit tests
- [x] 2.2 `protocol_probe.py book` on a locked slot (`book`, `creative`, `unopened`, `limits`,
  `gate`); `commit_protocol_fixtures.py` copies the session scripts and leaves the input stream out
  of timelines
- [x] 2.3 Re-check PR #71's book module and recording against the fixtures

## 3. Implement

- [x] 3.1 `item::nbt::write_sorted`, used by `with_item_tag` (decision 0048)
- [x] 3.2 `server::game::books`: `BookItems`, `OpenBook`, `open`, `edit`, `edit_transaction`,
  `apply_book_edit`; `UseKind::OpenBook`; `BookEdit` routed
- [x] 3.3 Tests against the fixtures: accepted edits byte for byte, refusals, the gate, opening

## 4. Record

- [x] 4.1 Parity row `BookEditPacket`; `cargo run -p parity -- generate` and `-- check`
- [x] 4.2 Quirk rows (books, start position); solution note; correct the round 2 note
- [x] 4.3 Decision 0048 (proposed)

## 5. Validate

- [x] 5.1 `cargo fmt --all -- --check`
- [x] 5.2 `cargo clippy -p unastar -p jolyne -p bds-extractor --all-targets -- -D warnings`
- [x] 5.3 `cargo test -p unastar --lib --tests`, `cargo test -p bds-extractor`
- [x] 5.4 `python -m unittest discover -s tools/bds-sweep -p "test_*.py"`
- [x] 5.5 `openspec validate --all --strict`, `bun tools/agents/check-links.ts`
