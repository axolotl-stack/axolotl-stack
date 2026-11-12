# parity-catalog Specification

## Purpose
The vanilla-parity catalog records, per domain, every thing vanilla Bedrock 1.26.51 has (and, for worldgen, Java 1.21.11) and how far `unastar` is with each. `crates/parity` generates the row lists from vanilla data, validates the hand-edited status files and claims in `docs/parity/`, and renders `docs/parity/PARITY.md`. This spec covers the tool's contracts; the row contents and statuses themselves are data, described in `docs/parity/README.md`. Worldgen generators and their fixtures are specified in `worldgen-pipeline`.

## Requirements

### Requirement: Rows are generated from vanilla data
`parity` SHALL generate the rows of seven domains (`entities`, `blocks`, `items`, `commands`, `protocol`, `worldgen`, `systems`) from vanilla data sources, so a new vanilla version adds rows without hand edits. Only `systems` MAY be a hand-maintained list, in `crates/parity/src/sources/systems.rs`. Row ids MUST be unique within a domain; a duplicate MUST fail generation.

#### Scenario: Row order
- **WHEN** a domain's rows are collected
- **THEN** they are sorted by family, each parent before its sub-rows (`<parent>/<suffix>`), then by id

#### Scenario: Duplicate id
- **WHEN** a source yields the same id twice in one domain
- **THEN** `DomainRows::new` fails with "generated id `<id>` appears twice"

Evidence: crates/parity/src/catalog.rs (`Domain`, `DomainRows::new`, `Row::child`), crates/parity/src/sources/mod.rs (`generate`), crates/parity/src/sources/blocks.rs, crates/parity/src/sources/entities.rs, crates/parity/src/sources/items.rs, crates/parity/src/sources/commands.rs, crates/parity/src/sources/protocol.rs, crates/parity/src/sources/worldgen.rs, crates/parity/src/sources/systems.rs

### Requirement: Exactly one status entry per row
Each domain SHALL have one status file, `docs/parity/<domain>.toml`, with exactly one entry per generated row. `check` MUST report every row without an entry and every entry naming no generated row. `generate` MUST add `{ status = "missing" }` for new rows and MUST refuse, without writing that file, when an entry names a row no longer generated.

#### Scenario: Stale entry
- **WHEN** a status file has an entry whose id is no longer generated
- **THEN** `check` reports "status entry `<id>` names no generated row" and `generate` fails asking to rename or delete it

#### Scenario: New vanilla row
- **WHEN** a source yields a row with no entry and `generate` runs
- **THEN** the status file gains a `missing` entry for it

Evidence: crates/parity/src/check.rs (`validate`, `unknown_ids`), crates/parity/src/main.rs (`generate`, `check`), crates/parity/src/status.rs (`Entry::missing`, `load`)

### Requirement: Status entry schema
A status entry SHALL have a `status` of `missing`, `partial`, `done`, `verified` or `not_applicable`, and only the optional fields `owner`, `tests` (list), `evidence`, `gaps` and `notes`. Any other field or status value MUST fail to parse, and a status file that fails to parse MUST fail both commands.

#### Scenario: Unknown field
- **WHEN** an entry contains a field outside the schema
- **THEN** loading the status file fails with a parse error naming the file

Evidence: crates/parity/src/status.rs (`Status`, `Entry`, `load`, `unknown_fields_are_rejected`)

### Requirement: Status-specific required fields
`check` SHALL require `gaps` for `partial`, `owner` and at least one `tests` reference for `done` and `verified`, `notes` for `not_applicable`, and for `verified` an `evidence` value other than empty or `none` that exists under the repository root and is tracked by git. `missing` MUST require nothing.

#### Scenario: Verified without evidence
- **WHEN** a `verified` entry has `evidence = "none"`
- **THEN** `check` reports "`verified` needs native `evidence`"

#### Scenario: Evidence path does not exist
- **WHEN** a `verified` entry's `evidence` is not a path in the repository
- **THEN** `check` reports "evidence `<path>` is not a path in the repository"

Evidence: crates/parity/src/check.rs (`validate_entry`, `validate_entry_requires_each_status_fields`)
Verified by: `cargo run -p parity -- check` (.github/workflows/test.yml, "Vanilla parity catalog")

### Requirement: Verified evidence must be tracked by git
`check` SHALL treat `verified` evidence as tracked when `git ls-files` lists it, lists a file under it, or it lies inside a submodule. Paths MUST be normalised first (`/` or `\` separators; `./x` and `a/../x` mean `x`); a path climbing above the root is never tracked. `git ls-files` MUST run at most once per run, and when it cannot run the problem MUST be reported once, saying to run `parity` inside a git checkout.

#### Scenario: Untracked or ignored evidence
- **WHEN** a `verified` entry's `evidence` exists on disk but is untracked or gitignored
- **THEN** `check` reports "evidence `<path>` is not tracked by git"

#### Scenario: Equivalent spellings and submodules
- **WHEN** evidence is written `./captures/x.bin`, `captures/../captures/x.bin` or `captures\x.bin` for a tracked file, or names a file inside a submodule
- **THEN** it counts as tracked

#### Scenario: No git checkout
- **WHEN** `check` runs outside a git checkout with several `verified` rows
- **THEN** exactly one problem is reported, saying to run `parity` inside a git checkout

Evidence: crates/parity/src/check.rs (`TestFiles::is_tracked`, `Tracked`, `normalise`, `verified_evidence_must_be_tracked_by_git`, `a_missing_git_checkout_is_reported_once`, `submodule_contents_count_as_tracked`)

### Requirement: Test references must resolve
Every `tests` reference SHALL be `path` or `path::fn_name` relative to the repository root. `check` MUST fail when the file does not exist or, for `path::...`, when the file has no test function named by the last `::` segment: an `fn` item starting its line (after `pub`, `async` or `unsafe`) whose attributes directly above include `#[test]`, `#[tokio::test]` or `#[tokio::test(...)]`. Comments, strings and helper functions MUST NOT count.

#### Scenario: Renamed test
- **WHEN** a `done` row references `crates/unastar/tests/worldgen_parity.rs::old_name` and that function was renamed
- **THEN** `check` reports "test `<reference>`: no `#[test]` function `old_name` in `crates/unastar/tests/worldgen_parity.rs`"

#### Scenario: Name only in a comment or a helper
- **WHEN** the referenced name appears only in a comment, a string, or an `fn` without a test attribute
- **THEN** the reference does not resolve

#### Scenario: Attributes and comments in between
- **WHEN** other attributes (including multi-line ones), line comments or doc comments sit between the test attribute and the `fn`
- **THEN** the reference resolves

Evidence: crates/parity/src/check.rs (`TestFiles::resolve`, `defines_fn`, `attributes_above`, `is_test_attribute`, `test_references_must_name_test_functions`, `resolve_checks_the_file_and_the_function`)

### Requirement: Engine internals are rejected in free text
`check` SHALL reject an entry whose `gaps`, `notes` or `evidence`, or a claim whose `area` or `owner`, contains (case-insensitively) `fun_`, `0x14`, `decompil`, `disassembl` or `rva `, so catalog text describes behaviour, not engine internals (decision 0016).

#### Scenario: Recovered function name in gaps
- **WHEN** a row's `gaps` mentions `FUN_1400abcd`
- **THEN** `check` reports that `gaps` contains `fun_` and asks to describe behaviour instead

Evidence: crates/parity/src/check.rs (`forbidden_provenance`, `validate_entry`, `forbidden_provenance_flags_engine_internals_case_insensitively`), crates/parity/src/claims.rs (`validate`), docs/decisions/0016-describe-behaviour-never-engine-internals.md

### Requirement: Status files are kept in canonical form
`generate` SHALL rewrite each status file in canonical form: a generated header, a `# <family> (<count>)` comment before each family, and one inline table per row in catalog order with fields in the order `status`, `owner`, `tests`, `evidence`, `gaps`, `notes`, empty fields omitted and the row label as a trailing comment. `check` MUST fail when a complete status file differs from that form, ignoring only CRLF/LF differences.

#### Scenario: Hand-reordered file
- **WHEN** a contributor moves an entry out of catalog order or writes fields in another order
- **THEN** `check` reports "docs/parity/<domain>.toml is not in canonical form; run `parity generate`"

Evidence: crates/parity/src/status.rs (`render`, `render_line`, `render_writes_families_in_catalog_order_with_one_inline_table_per_row`, `rendered_files_load_back_to_the_same_entries`), crates/parity/src/main.rs (`check`, `same_text`)

### Requirement: PARITY.md is generated and checked for staleness
`PARITY.md` SHALL be rendered only by `generate`, from the rows, status files and claims. When no other problem exists, `check` MUST fail if the committed file differs from a fresh render (CRLF/LF ignored). Done+verified MUST be (done + verified) / (rows - not_applicable), shown as `n/a` when no row applies; the items headline MUST be its behaviour sub-rows, every other domain's its top-level rows.

#### Scenario: Status edited without regenerating
- **WHEN** a row's status changes and `PARITY.md` is not regenerated
- **THEN** `check` reports "docs/parity/PARITY.md is stale; run `parity generate`"

#### Scenario: Item headline
- **WHEN** an item has a `done` registry row and a `partial` behaviour sub-row
- **THEN** the items summary lists the behaviour sub-rows first, labels the registry line "registered", and the family table counts only the sub-row

Evidence: crates/parity/src/report.rs (`render`, `Counts::percent`, `where_to_help_splits_open_rows_into_free_families_and_claims`, `item_registration_rows_are_labelled_and_behaviour_sub_rows_are_the_headline`), crates/parity/src/catalog.rs (`Domain::headline`), crates/parity/src/main.rs (`check`)

### Requirement: Claim schema
`docs/parity/claims.toml` SHALL hold `[[claim]]` tables with `area`, `owner`, `since`, `domain` and optional `families`, `ids`, `prefixes` and `change`; unknown fields MUST fail to parse. `since` MUST be a TOML date without a time; a quoted string MUST fail to parse. `area` and `owner` MUST NOT be blank. An absent claims file means no claims.

#### Scenario: Wrong field or quoted date
- **WHEN** a claim uses `family` instead of `families`, or `since = '2026-10-30'`
- **THEN** parsing the claims file fails

#### Scenario: Date-time and blank owner
- **WHEN** a claim has `since = 2026-10-30T12:00:00` and `owner = " "`
- **THEN** `check` reports "`since` must be a date" and "`owner` is empty"

Evidence: crates/parity/src/claims.rs (`ClaimsFile`, `Claim`, `load`, `validate`, `unknown_fields_and_string_dates_fail_to_parse`, `claims_need_an_owner_a_date_and_a_selector`)

### Requirement: Claim selectors must match generated rows
A claim SHALL name a known domain and select at least one of `families`, `ids` or `prefixes`. Each family, id and non-empty prefix MUST match at least one row of that domain. A claim covers a row of its own domain when the row's family is listed, its id or parent id is listed, or its id starts with a listed prefix. Claims MUST NOT change any row's status.

#### Scenario: Typos are rejected
- **WHEN** a claim names family `door`, id `minecraft:oak_dor`, a prefix matching nothing, or domain `block`
- **THEN** `check` reports each one ("no blocks family", "no blocks row", "matches no blocks row", "unknown domain")

#### Scenario: Ids cover their sub-rows
- **WHEN** a claim lists `ids = ["minecraft:oak_door"]`
- **THEN** it covers `minecraft:oak_door` and `minecraft:oak_door/redstone`, and no rows of other domains

Evidence: crates/parity/src/claims.rs (`Claim::covers`, `validate`, `families_ids_and_prefixes_select_rows_and_ids_take_their_sub_rows`, `a_claim_only_covers_rows_of_its_own_domain`, `unknown_domains_families_ids_and_prefixes_are_rejected`)

### Requirement: Claims never overlap
`check` SHALL report one problem per pair of claims that cover a common row, naming both areas and owners, the domain, the number of shared rows and one example id, so every open row is either free or claimed by exactly one claim.

#### Scenario: Two claims on liquids
- **WHEN** one claim selects family `liquids` and id `minecraft:sponge`, and another selects families `surface_dynamic` and `liquids`
- **THEN** exactly one problem is reported, saying the claims overlap on 2 blocks rows

Evidence: crates/parity/src/claims.rs (`overlaps`, `overlapping_claims_are_rejected_with_the_shared_row_count`)

### Requirement: A claim's change must be an active OpenSpec change
When a claim sets `change`, `check` SHALL require it to be a kebab-case name (lowercase ASCII letters, digits and `-`) with `openspec/changes/<change>/proposal.md` present. A change found only under `openspec/changes/archive/<YYYY-MM-DD>-<change>` MUST be reported as archived, so the claim is removed when the change is archived. Claims without `change` MUST pass this check.

#### Scenario: Active change
- **WHEN** a claim names `liquid-flow` and `openspec/changes/liquid-flow/proposal.md` exists
- **THEN** no problem is reported

#### Scenario: Archived, missing or malformed change
- **WHEN** a claim names an archived change, a change that does not exist, or `Liquid Flow`
- **THEN** `check` reports "is archived", "no active OpenSpec change" or "kebab-case" respectively

Evidence: crates/parity/src/claims.rs (`validate_changes`, `CHANGES_DIR`, `a_claim_may_name_its_active_openspec_change`, `claims_naming_missing_archived_or_malformed_changes_are_rejected`), crates/parity/src/main.rs (`validate_all`)

### Requirement: Bedrock worldgen rows use a committed snapshot
Bedrock worldgen rows SHALL be read from the BDS install named by `UNASTAR_BDS_DIR` when it is set, and otherwise from the committed snapshot `docs/parity/source/bedrock-worldgen.tsv` (`family<TAB>id<TAB>label` lines, identifiers and short labels only). With the variable set, `generate` MUST rewrite the snapshot and `check` MUST fail if it is stale; without it, both MUST use the snapshot and `generate` MUST say so.

#### Scenario: Check without an install
- **WHEN** `cargo run -p parity -- check` runs without `UNASTAR_BDS_DIR` (as in CI)
- **THEN** Bedrock worldgen rows come from the snapshot and no install is read

#### Scenario: Snapshot out of date
- **WHEN** `check` runs with `UNASTAR_BDS_DIR` set and the install yields different rows
- **THEN** it reports that `docs/parity/source/bedrock-worldgen.tsv` is stale against the BDS install

#### Scenario: Malformed snapshot
- **WHEN** a non-comment snapshot line has fewer than two tab-separated fields
- **THEN** generation fails with "malformed line"

Evidence: crates/parity/src/sources/worldgen.rs (`bedrock_rows`, `bedrock_rows_from_install`, `render_snapshot`, `read_snapshot`, `BEDROCK_SNAPSHOT`, `BDS_DIR_VAR`, `snapshot_round_trips_sorted_by_family_then_id`, `snapshot_reading_fails_on_a_missing_file_or_a_malformed_line`), crates/parity/src/main.rs (`generate`, `check`), docs/parity/source/bedrock-worldgen.tsv
Verified by: `cargo run -p parity -- check` (.github/workflows/test.yml)
