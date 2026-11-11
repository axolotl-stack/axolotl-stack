//! The world database's level-data records, as BDS 1.26.51 writes them
//! (measured with `tools/bds-sweep/level_data.py`). Each is one rooted
//! little-endian NBT compound with an empty name:
//!
//! | Key | Holds |
//! |---|---|
//! | `scoreboard` | `Objectives` (creation order; each with `Scores` in the order they were first set), `Entries` (one per holder with a score: `IdentityType` 1 player `PlayerId`, 2 actor `EntityID`, 3 fake `FakePlayerName`, and its `ScoreboardId`), `DisplayObjectives` (slot `Name`, `ObjectiveName`, `SortOrder` 0 ascending / 1 descending), `LastUniqueID` (the last scoreboard id handed out) and an empty `Criteria` list |
//! | `mobevents` | `events_enabled` and one byte per mob event id |
//! | `tickingarea_<uuid>` | one area: `Dimension`, `IsCircle`, `MinX`/`MinZ`/`MaxX`/`MaxZ` (block coordinates of the first and last chunk), `Name`, `Preload` (written only when set) |
//! | `structuretemplate_<name>` | a structure saved in `disk` mode: `format_version` 2, `size`, `structure` (`block_indices`, `entities`, `palette.default`), `structure_world_origin` |
//! | `WorldClocks` | `clocks`: `Name`, `Time` (the clock as an int), `IsPaused` |
//!
//! Decoders accept exactly the fields BDS writes and fail on anything else:
//! a record unastar does not understand is left alone, never rewritten.

use super::nbt::{self, Compound, CompoundExt, FieldError, NbtError, Tag};

pub const SCOREBOARD_KEY: &[u8] = b"scoreboard";
pub const MOB_EVENTS_KEY: &[u8] = b"mobevents";
pub const WORLD_CLOCKS_KEY: &[u8] = b"WorldClocks";
pub const TICKING_AREA_PREFIX: &[u8] = b"tickingarea_";
pub const STRUCTURE_PREFIX: &[u8] = b"structuretemplate_";

/// Keys of the single records unastar reads and writes.
pub const SINGLE_KEYS: [&[u8]; 3] = [SCOREBOARD_KEY, MOB_EVENTS_KEY, WORLD_CLOCKS_KEY];
/// Key prefixes of the record families unastar reads and writes.
pub const PREFIXES: [&[u8]; 2] = [TICKING_AREA_PREFIX, STRUCTURE_PREFIX];

/// Whether `key` is one of the level-data records unastar keeps.
pub fn is_level_record(key: &[u8]) -> bool {
    SINGLE_KEYS.contains(&key) || PREFIXES.iter().any(|prefix| key.starts_with(prefix))
}

/// Why a record cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    #[error(transparent)]
    Nbt(#[from] NbtError),
    #[error(transparent)]
    Field(#[from] FieldError),
    #[error("{0}")]
    Invalid(String),
}

fn invalid(text: impl Into<String>) -> RecordError {
    RecordError::Invalid(text.into())
}

/// Fails unless `compound` has only the named fields.
fn only(compound: &Compound, fields: &[&str]) -> Result<(), RecordError> {
    match compound.keys().find(|key| !fields.contains(&key.as_str())) {
        Some(key) => Err(invalid(format!("unexpected field {key:?}"))),
        None => Ok(()),
    }
}

fn compounds<'a>(items: &'a [Tag], what: &str) -> Result<Vec<&'a Compound>, RecordError> {
    items
        .iter()
        .map(|item| {
            item.as_compound()
                .ok_or_else(|| invalid(format!("{what} entry is not a compound")))
        })
        .collect()
}

fn fields(pairs: Vec<(&str, Tag)>) -> Compound {
    pairs
        .into_iter()
        .map(|(name, tag)| (name.to_owned(), tag))
        .collect()
}

// --- scoreboard ---------------------------------------------------------------

/// Who a scoreboard entry belongs to (`IdentityType`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StoredIdentity {
    /// 1: a player, by the player's actor unique id.
    Player(i64),
    /// 2: another actor, by its actor unique id.
    Entity(i64),
    /// 3: a name with no actor behind it.
    Fake(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObjective {
    pub name: String,
    pub display_name: String,
    pub criteria: String,
    /// `(ScoreboardId, Score)` in stored order.
    pub scores: Vec<(i64, i32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDisplay {
    /// `list`, `sidebar` or `belowname`.
    pub slot: String,
    pub objective: String,
    /// 0 ascending, 1 descending.
    pub sort_order: i8,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoredScoreboard {
    pub objectives: Vec<StoredObjective>,
    /// `(ScoreboardId, identity)`; BDS writes the holders that have a score.
    pub entries: Vec<(i64, StoredIdentity)>,
    pub displays: Vec<StoredDisplay>,
    pub last_unique_id: i64,
}

impl StoredScoreboard {
    pub fn decode(bytes: &[u8]) -> Result<Self, RecordError> {
        let root = nbt::read_record(bytes)?;
        only(
            &root,
            &[
                "Criteria",
                "DisplayObjectives",
                "Entries",
                "LastUniqueID",
                "Objectives",
            ],
        )?;
        if !root.list("Criteria")?.is_empty() {
            return Err(invalid("scoreboard Criteria is not empty"));
        }
        let mut board = Self {
            last_unique_id: root.long("LastUniqueID")?,
            ..Self::default()
        };
        for entry in compounds(root.list("Entries")?, "Entries")? {
            let id = entry.long("ScoreboardId")?;
            let identity = match entry.byte("IdentityType")? {
                1 => {
                    only(entry, &["IdentityType", "PlayerId", "ScoreboardId"])?;
                    StoredIdentity::Player(entry.long("PlayerId")?)
                }
                2 => {
                    only(entry, &["EntityID", "IdentityType", "ScoreboardId"])?;
                    StoredIdentity::Entity(entry.long("EntityID")?)
                }
                3 => {
                    only(entry, &["FakePlayerName", "IdentityType", "ScoreboardId"])?;
                    StoredIdentity::Fake(entry.string("FakePlayerName")?.to_owned())
                }
                other => return Err(invalid(format!("unknown IdentityType {other}"))),
            };
            if board.entries.iter().any(|(known, _)| *known == id) {
                return Err(invalid(format!("ScoreboardId {id} is listed twice")));
            }
            board.entries.push((id, identity));
        }
        for objective in compounds(root.list("Objectives")?, "Objectives")? {
            only(objective, &["Criteria", "DisplayName", "Name", "Scores"])?;
            let mut scores = Vec::new();
            for score in compounds(objective.list("Scores")?, "Scores")? {
                only(score, &["Score", "ScoreboardId"])?;
                let id = score.long("ScoreboardId")?;
                if !board.entries.iter().any(|(known, _)| *known == id) {
                    return Err(invalid(format!(
                        "a score names ScoreboardId {id}, no entry"
                    )));
                }
                scores.push((id, score.int("Score")?));
            }
            board.objectives.push(StoredObjective {
                name: objective.string("Name")?.to_owned(),
                display_name: objective.string("DisplayName")?.to_owned(),
                criteria: objective.string("Criteria")?.to_owned(),
                scores,
            });
        }
        for display in compounds(root.list("DisplayObjectives")?, "DisplayObjectives")? {
            only(display, &["Name", "ObjectiveName", "SortOrder"])?;
            board.displays.push(StoredDisplay {
                slot: display.string("Name")?.to_owned(),
                objective: display.string("ObjectiveName")?.to_owned(),
                sort_order: display.byte("SortOrder")?,
            });
        }
        Ok(board)
    }

    pub fn encode(&self) -> Vec<u8> {
        let entries = self
            .entries
            .iter()
            .map(|(id, identity)| {
                let mut entry = match identity {
                    StoredIdentity::Player(player) => fields(vec![
                        ("IdentityType", Tag::Byte(1)),
                        ("PlayerId", Tag::Long(*player)),
                    ]),
                    StoredIdentity::Entity(actor) => fields(vec![
                        ("IdentityType", Tag::Byte(2)),
                        ("EntityID", Tag::Long(*actor)),
                    ]),
                    StoredIdentity::Fake(name) => fields(vec![
                        ("IdentityType", Tag::Byte(3)),
                        ("FakePlayerName", Tag::String(name.clone())),
                    ]),
                };
                entry.insert("ScoreboardId".into(), Tag::Long(*id));
                Tag::Compound(entry)
            })
            .collect();
        let objectives = self
            .objectives
            .iter()
            .map(|objective| {
                Tag::Compound(fields(vec![
                    ("Criteria", Tag::String(objective.criteria.clone())),
                    ("DisplayName", Tag::String(objective.display_name.clone())),
                    ("Name", Tag::String(objective.name.clone())),
                    (
                        "Scores",
                        Tag::list(
                            objective
                                .scores
                                .iter()
                                .map(|(id, score)| {
                                    Tag::Compound(fields(vec![
                                        ("Score", Tag::Int(*score)),
                                        ("ScoreboardId", Tag::Long(*id)),
                                    ]))
                                })
                                .collect(),
                        ),
                    ),
                ]))
            })
            .collect();
        let displays = self
            .displays
            .iter()
            .map(|display| {
                Tag::Compound(fields(vec![
                    ("Name", Tag::String(display.slot.clone())),
                    ("ObjectiveName", Tag::String(display.objective.clone())),
                    ("SortOrder", Tag::Byte(display.sort_order)),
                ]))
            })
            .collect();
        nbt::write_record(&fields(vec![
            ("Criteria", Tag::list(Vec::new())),
            ("DisplayObjectives", Tag::list(displays)),
            ("Entries", Tag::list(entries)),
            ("LastUniqueID", Tag::Long(self.last_unique_id)),
            ("Objectives", Tag::list(objectives)),
        ]))
    }
}

// --- mob events ---------------------------------------------------------------

/// The `mobevents` switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredMobEvents {
    pub events_enabled: bool,
    pub ender_dragon: bool,
    pub pillager_patrols: bool,
    pub wandering_trader: bool,
}

const MOB_EVENT_FIELDS: [&str; 4] = [
    "events_enabled",
    "minecraft:ender_dragon_event",
    "minecraft:pillager_patrols_event",
    "minecraft:wandering_trader_event",
];

impl StoredMobEvents {
    pub fn decode(bytes: &[u8]) -> Result<Self, RecordError> {
        let root = nbt::read_record(bytes)?;
        only(&root, &MOB_EVENT_FIELDS)?;
        let switch = |name: &str| -> Result<bool, RecordError> {
            match root.byte(name)? {
                0 => Ok(false),
                1 => Ok(true),
                other => Err(invalid(format!("{name} is {other}, not 0 or 1"))),
            }
        };
        Ok(Self {
            events_enabled: switch(MOB_EVENT_FIELDS[0])?,
            ender_dragon: switch(MOB_EVENT_FIELDS[1])?,
            pillager_patrols: switch(MOB_EVENT_FIELDS[2])?,
            wandering_trader: switch(MOB_EVENT_FIELDS[3])?,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let values = [
            self.events_enabled,
            self.ender_dragon,
            self.pillager_patrols,
            self.wandering_trader,
        ];
        nbt::write_record(
            &MOB_EVENT_FIELDS
                .iter()
                .zip(values)
                .map(|(name, on)| ((*name).to_owned(), Tag::Byte(i8::from(on))))
                .collect(),
        )
    }
}

// --- ticking areas ------------------------------------------------------------

/// One `tickingarea_<uuid>` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredTickingArea {
    pub dimension: i32,
    pub circle: bool,
    /// Block coordinates of the first chunk's corner.
    pub min: (i32, i32),
    /// Block coordinates of the last chunk's corner.
    pub max: (i32, i32),
    pub name: String,
    pub preload: bool,
}

impl StoredTickingArea {
    pub fn key(uuid: &str) -> Vec<u8> {
        [TICKING_AREA_PREFIX, uuid.as_bytes()].concat()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RecordError> {
        let root = nbt::read_record(bytes)?;
        only(
            &root,
            &[
                "Dimension",
                "IsCircle",
                "MaxX",
                "MaxZ",
                "MinX",
                "MinZ",
                "Name",
                "Preload",
            ],
        )?;
        let flag = |name: &str, value: i8| match value {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(invalid(format!("{name} is {other}, not 0 or 1"))),
        };
        let area = Self {
            dimension: root.int("Dimension")?,
            circle: flag("IsCircle", root.byte("IsCircle")?)?,
            min: (root.int("MinX")?, root.int("MinZ")?),
            max: (root.int("MaxX")?, root.int("MaxZ")?),
            name: root.string("Name")?.to_owned(),
            preload: match root.opt_byte("Preload")? {
                None => false,
                Some(value) => flag("Preload", value)?,
            },
        };
        let chunk_aligned = [area.min.0, area.min.1, area.max.0, area.max.1]
            .iter()
            .all(|v| v.rem_euclid(16) == 0);
        if !chunk_aligned || area.min.0 > area.max.0 || area.min.1 > area.max.1 {
            return Err(invalid(format!(
                "ticking area {:?} bounds {:?} to {:?} are not chunk corners in order",
                area.name, area.min, area.max
            )));
        }
        if area.circle
            && (area.max.0 - area.min.0 != area.max.1 - area.min.1
                || (area.max.0 - area.min.0) % 32 != 0)
        {
            return Err(invalid(format!(
                "circle ticking area {:?} is not square around a chunk",
                area.name
            )));
        }
        Ok(area)
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut root = fields(vec![
            ("Dimension", Tag::Int(self.dimension)),
            ("IsCircle", Tag::Byte(i8::from(self.circle))),
            ("MaxX", Tag::Int(self.max.0)),
            ("MaxZ", Tag::Int(self.max.1)),
            ("MinX", Tag::Int(self.min.0)),
            ("MinZ", Tag::Int(self.min.1)),
            ("Name", Tag::String(self.name.clone())),
        ]);
        if self.preload {
            root.insert("Preload".into(), Tag::Byte(1));
        }
        nbt::write_record(&root)
    }
}

// --- structures ---------------------------------------------------------------

/// The block-state `version` BDS 1.26.51 writes in structure palettes.
pub const BLOCK_STATE_VERSION: i32 = 18_168_865;

/// One palette entry: a block name and its states.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredBlockState {
    pub name: String,
    pub states: Compound,
    pub version: i32,
}

/// One `structuretemplate_<name>` record.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredStructure {
    pub size: [i32; 3],
    /// Where the structure was saved from.
    pub origin: [i32; 3],
    /// Palette indices per layer (z fastest, then y, then x); -1 is no
    /// block. BDS writes one layer.
    pub layers: Vec<Vec<i32>>,
    pub palette: Vec<StoredBlockState>,
    /// Saved actors (unastar saves none).
    pub entities: Vec<Tag>,
    /// Block entity data by block index (unastar saves none).
    pub block_position_data: Compound,
}

fn three_ints(root: &Compound, name: &str) -> Result<[i32; 3], RecordError> {
    let items = root.list(name)?;
    match items {
        [Tag::Int(x), Tag::Int(y), Tag::Int(z)] => Ok([*x, *y, *z]),
        _ => Err(invalid(format!("{name} is not three Ints"))),
    }
}

impl StoredStructure {
    pub fn key(name: &str) -> Vec<u8> {
        [STRUCTURE_PREFIX, name.as_bytes()].concat()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RecordError> {
        let root = nbt::read_record(bytes)?;
        only(
            &root,
            &[
                "format_version",
                "size",
                "structure",
                "structure_world_origin",
            ],
        )?;
        let version = root.int("format_version")?;
        if !(1..=2).contains(&version) {
            return Err(invalid(format!("structure format_version {version}")));
        }
        let size = three_ints(&root, "size")?;
        if size.iter().any(|axis| *axis <= 0) {
            return Err(invalid(format!("structure size {size:?}")));
        }
        let volume = size.iter().map(|axis| *axis as usize).product::<usize>();
        let structure = root.compound("structure")?;
        only(structure, &["block_indices", "entities", "palette"])?;
        let palette = structure.compound("palette")?;
        only(palette, &["default"])?;
        let default = palette.compound("default")?;
        only(default, &["block_palette", "block_position_data"])?;
        let mut states = Vec::new();
        for entry in compounds(default.list("block_palette")?, "block_palette")? {
            only(entry, &["name", "states", "version"])?;
            states.push(StoredBlockState {
                name: entry.string("name")?.to_owned(),
                states: entry.compound("states")?.clone(),
                version: entry.int("version")?,
            });
        }
        let mut layers = Vec::new();
        for layer in structure.list("block_indices")? {
            let Tag::IntArray(indices) = layer else {
                return Err(invalid("a block_indices layer is not an Int array"));
            };
            if indices.len() != volume {
                return Err(invalid(format!(
                    "a layer has {} indices for a structure of {volume} blocks",
                    indices.len()
                )));
            }
            if let Some(bad) = indices
                .iter()
                .find(|index| **index < -1 || **index >= states.len() as i32)
            {
                return Err(invalid(format!(
                    "block index {bad} is outside the palette of {}",
                    states.len()
                )));
            }
            layers.push(indices.clone());
        }
        if layers.is_empty() || layers.len() > 2 {
            return Err(invalid(format!("structure has {} layers", layers.len())));
        }
        Ok(Self {
            size,
            origin: three_ints(&root, "structure_world_origin")?,
            layers,
            palette: states,
            entities: structure.list("entities")?.to_vec(),
            block_position_data: default.compound("block_position_data")?.clone(),
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let ints = |values: [i32; 3]| Tag::list(values.iter().copied().map(Tag::Int).collect());
        let palette = self
            .palette
            .iter()
            .map(|state| {
                Tag::Compound(fields(vec![
                    ("name", Tag::String(state.name.clone())),
                    ("states", Tag::Compound(state.states.clone())),
                    ("version", Tag::Int(state.version)),
                ]))
            })
            .collect();
        let default = fields(vec![
            ("block_palette", Tag::list(palette)),
            (
                "block_position_data",
                Tag::Compound(self.block_position_data.clone()),
            ),
        ]);
        let structure = fields(vec![
            (
                "block_indices",
                Tag::list(self.layers.iter().cloned().map(Tag::IntArray).collect()),
            ),
            ("entities", Tag::list(self.entities.clone())),
            (
                "palette",
                Tag::Compound(fields(vec![("default", Tag::Compound(default))])),
            ),
        ]);
        nbt::write_record(&fields(vec![
            ("format_version", Tag::Int(2)),
            ("size", ints(self.size)),
            ("structure", Tag::Compound(structure)),
            ("structure_world_origin", ints(self.origin)),
        ]))
    }
}

// --- world clocks -------------------------------------------------------------

/// The overworld clock's name in `WorldClocks`.
pub const OVERWORLD_CLOCK: &str = "minecraft:overworld";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredClock {
    pub name: String,
    pub time: i32,
    pub paused: bool,
}

/// The `WorldClocks` record.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoredClocks(pub Vec<StoredClock>);

impl StoredClocks {
    pub fn decode(bytes: &[u8]) -> Result<Self, RecordError> {
        let root = nbt::read_record(bytes)?;
        only(&root, &["clocks"])?;
        let mut clocks = Vec::new();
        for clock in compounds(root.list("clocks")?, "clocks")? {
            only(clock, &["IsPaused", "Name", "Time"])?;
            clocks.push(StoredClock {
                name: clock.string("Name")?.to_owned(),
                time: clock.int("Time")?,
                paused: match clock.byte("IsPaused")? {
                    0 => false,
                    1 => true,
                    other => return Err(invalid(format!("IsPaused is {other}"))),
                },
            });
        }
        Ok(Self(clocks))
    }

    pub fn encode(&self) -> Vec<u8> {
        let clocks = self
            .0
            .iter()
            .map(|clock| {
                Tag::Compound(fields(vec![
                    ("IsPaused", Tag::Byte(i8::from(clock.paused))),
                    ("Name", Tag::String(clock.name.clone())),
                    ("Time", Tag::Int(clock.time)),
                ]))
            })
            .collect();
        nbt::write_record(&fields(vec![("clocks", Tag::list(clocks))]))
    }

    /// The overworld clock's time, if the record has that clock.
    pub fn overworld(&self) -> Option<i32> {
        self.0
            .iter()
            .find(|clock| clock.name == OVERWORLD_CLOCK)
            .map(|clock| clock.time)
    }

    /// Sets the overworld clock, adding it if missing.
    pub fn set_overworld(&mut self, time: i32) {
        match self
            .0
            .iter_mut()
            .find(|clock| clock.name == OVERWORLD_CLOCK)
        {
            Some(clock) => clock.time = time,
            None => self.0.push(StoredClock {
                name: OVERWORLD_CLOCK.into(),
                time,
                paused: false,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_round_trip() {
        let board = StoredScoreboard {
            objectives: vec![StoredObjective {
                name: "obj1".into(),
                display_name: "Objective One".into(),
                criteria: "dummy".into(),
                scores: vec![(2, -7), (1, 5)],
            }],
            entries: vec![
                (1, StoredIdentity::Fake("FakeA".into())),
                (2, StoredIdentity::Player(-8_589_934_590)),
                (3, StoredIdentity::Entity(-8_589_934_591)),
            ],
            displays: vec![StoredDisplay {
                slot: "sidebar".into(),
                objective: "obj1".into(),
                sort_order: 0,
            }],
            last_unique_id: 4,
        };
        assert_eq!(StoredScoreboard::decode(&board.encode()).unwrap(), board);

        let events = StoredMobEvents {
            events_enabled: false,
            ender_dragon: true,
            pillager_patrols: false,
            wandering_trader: true,
        };
        assert_eq!(StoredMobEvents::decode(&events.encode()).unwrap(), events);

        let area = StoredTickingArea {
            dimension: 0,
            circle: true,
            min: (64, 64),
            max: (128, 128),
            name: "circlearea".into(),
            preload: false,
        };
        assert_eq!(StoredTickingArea::decode(&area.encode()).unwrap(), area);

        let mut clocks = StoredClocks::default();
        clocks.set_overworld(4321);
        assert_eq!(StoredClocks::decode(&clocks.encode()).unwrap(), clocks);
        assert_eq!(clocks.overworld(), Some(4321));
    }

    #[test]
    fn scores_must_name_an_entry() {
        let board = StoredScoreboard {
            objectives: vec![StoredObjective {
                name: "o".into(),
                display_name: "o".into(),
                criteria: "dummy".into(),
                scores: vec![(9, 1)],
            }],
            ..StoredScoreboard::default()
        };
        assert!(StoredScoreboard::decode(&board.encode()).is_err());
    }

    #[test]
    fn ticking_areas_must_cover_whole_chunks() {
        let mut area = StoredTickingArea {
            dimension: 0,
            circle: false,
            min: (0, 0),
            max: (17, 16),
            name: "a".into(),
            preload: true,
        };
        assert!(StoredTickingArea::decode(&area.encode()).is_err());
        area.max = (-16, 16);
        assert!(StoredTickingArea::decode(&area.encode()).is_err());
    }

    #[test]
    fn level_record_keys() {
        assert!(is_level_record(b"scoreboard"));
        assert!(is_level_record(
            b"tickingarea_3152dda7-059c-4842-a31a-c9ce8ef3a036"
        ));
        assert!(is_level_record(b"structuretemplate_mystructure:s1"));
        assert!(!is_level_record(b"schedulerWT"));
        assert!(!is_level_record(b"player_server_e0d44980"));
    }
}
