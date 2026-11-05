## RENAMED Requirements

- FROM: `### Requirement: Subchunk palettes store runtime block-state ids`
- TO: `### Requirement: Subchunk palettes store block names and states`

## MODIFIED Requirements

### Requirement: Subchunk palettes store block names and states
Both providers SHALL store each subchunk in the version 9 layout (version byte, storage count, Y index, then one or two paletted storages) whose palette entries are block names plus state properties in NBT (`{name, states, version}`), the Bedrock on-disk form, so a saved world does not depend on the runtime palette order. Loading MUST accept versions 8 and 9, at most two storages, and both the NBT palette and the earlier runtime-id palette.

#### Scenario: Round trip within one build
- **WHEN** a chunk with stone in every section is saved and loaded by the same build
- **THEN** every block holds the same runtime id

#### Scenario: LevelDB subchunk that does not decode
- **WHEN** a stored LevelDB subchunk has an unsupported version or storage count
- **THEN** a warning is logged and that section loads as air while the rest of the column loads

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

## ADDED Requirements

### Requirement: Unknown saved block states fail to load
Loading a subchunk whose palette names a block or state set the registry does not know MUST fail that chunk with a typed error naming the entry, and MUST NOT substitute another block.

#### Scenario: Unknown block name
- **WHEN** a saved palette entry names a block that is not in the registry
- **THEN** loading that chunk returns an error naming the block, and no air or fallback block is placed
