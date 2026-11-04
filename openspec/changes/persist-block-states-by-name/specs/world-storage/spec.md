## Purpose

What the server guarantees about worlds it saves and loads: block, chunk and player data that
survives restarts, version changes and exchange with BDS.

## ADDED Requirements

### Requirement: Block states are saved by name and state
The server SHALL save each subchunk palette entry as its block name plus state properties, so a saved world does not depend on the order of the runtime block palette. Loading MUST map each saved name and state set to the current runtime block state.

#### Scenario: Round trip of every block state
- **WHEN** a chunk containing every block state is saved and loaded again
- **THEN** every position holds the same block state as before saving

#### Scenario: Palette order changes
- **WHEN** a world saved by one build is loaded by a build whose runtime palette order differs
- **THEN** every block loads as the same named block with the same states

### Requirement: Unknown saved block states fail to load
Loading a subchunk whose palette names a block or state set the registry does not know MUST fail with a typed error naming the entry, and MUST NOT substitute another block.

#### Scenario: Unknown block name
- **WHEN** a saved palette entry names a block that is not in the registry
- **THEN** loading that chunk returns an error naming the block, and no air or fallback block is placed

### Requirement: Worlds saved in the earlier format still load
The server SHALL load subchunks written in the earlier runtime-id palette format, so worlds saved before this change keep working.

#### Scenario: Legacy subchunk
- **WHEN** a subchunk written with runtime-id palette entries is loaded
- **THEN** its blocks load unchanged
- **AND** the next save writes it in the name-and-state form
