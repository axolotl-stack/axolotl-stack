## RENAMED Requirements

- FROM: `### Requirement: Subchunk palettes store runtime block-state ids`
- TO: `### Requirement: Subchunk palettes store block names and states`

## MODIFIED Requirements

### Requirement: Subchunk palettes store block names and states
Both providers SHALL store each subchunk in the version 9 layout (version byte, storage count, Y index, then one or two paletted storages) whose palette entries are block names plus state properties in NBT (`{name, states, version}`), the Bedrock on-disk form, so a saved world does not depend on the runtime palette order. Loading MUST accept versions 8 and 9, at most two storages, and per storage both the NBT palette (header low bit 0) and the earlier runtime-id palette (low bit 1); the record's key, not its Y byte, places it. A subchunk that cannot be read or decoded MUST fail the whole column load.

#### Scenario: Round trip within one build
- **WHEN** a chunk with stone in every section is saved and loaded by the same build
- **THEN** every block holds the same runtime id

#### Scenario: LevelDB subchunk that does not decode
- **WHEN** a stored LevelDB subchunk has an unsupported version or storage count
- **THEN** `load_column` returns `Err(StorageError::Deserialization(_))` naming the chunk and subchunk Y, so the column is neither loaded as air nor regenerated, and the stored bytes are kept

#### Scenario: Round trip of every block state
- **WHEN** a chunk containing every block state is saved and loaded again
- **THEN** every position holds the same block state as before saving

#### Scenario: Palette order changes
- **WHEN** a world saved by one build is loaded by a build whose runtime palette order differs
- **THEN** every block loads as the same named block with the same states

#### Scenario: Legacy runtime-id subchunk
- **WHEN** a subchunk written with runtime-id palette entries is loaded
- **THEN** its blocks load unchanged
- **AND** the next save writes it in the name-and-state form

#### Scenario: Subchunk saved by BDS
- **WHEN** a subchunk record saved by BDS 1.26.51 is loaded: a palette of 0, 1, 2, 3, 4, 5, 6, 8 or 16 bits, unused palette entries, one or two storages, any dimension
- **THEN** every cell holds the block state BDS reports for it after reloading the same world

#### Scenario: Lenient framing BDS accepts
- **WHEN** a version 9 record's Y byte differs from its key, bytes follow its last storage, or a palette lists more entries than its bit width can index
- **THEN** it loads at the key's Y; the extra bytes are not read, and the extra entries are resolved but no cell can use them (quirk "World / storage" in `docs/quirks/bds.md`)

#### Scenario: Index outside the palette
- **WHEN** a stored storage has an index past the end of its palette
- **THEN** `load_column` returns `Err(StorageError::Deserialization(_))`, nothing panics, and the stored bytes are kept

## ADDED Requirements

### Requirement: Unknown saved block states fail to load
Loading a subchunk whose palette names a block or state set the registry does not know MUST fail that chunk with a typed error naming the entry, and MUST NOT substitute another block.

#### Scenario: Unknown block name
- **WHEN** a saved palette entry names a block that is not in the registry
- **THEN** loading that chunk returns an error naming the block, and no air or fallback block is placed

#### Scenario: Unknown state
- **WHEN** a saved palette entry names a known block with a state value, a state type or a set of state keys no state of that block has
- **THEN** loading that chunk returns an error naming the block and the states
