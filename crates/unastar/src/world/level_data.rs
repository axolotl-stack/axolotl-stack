//! The world's level data in the ECS: what `level.dat` and the world
//! database's level records hold (`crate::storage::level`, BDS 1.26.51's
//! layout), applied when the server starts and captured when it stops.
//!
//! | Saved | ECS state | When the world loads |
//! |---|---|---|
//! | `Time` | [`WorldTime`] | restored |
//! | `currentTick` | [`TickCounter`] (`/time query gametime`, StartGame's level time) | restored |
//! | game rules (lower-case names; `locatorbar` is not saved) | [`GameRules`] | restored |
//! | `Difficulty` | [`Difficulty`] | written, not restored: BDS takes the difficulty from `server.properties` at every start (measured), unastar from its config |
//! | `SpawnX/Y/Z` (`SpawnY` 32767: never set) | [`LevelSpawn`], `WorldSpawn` | restored when set |
//! | `rainLevel`, `lightningLevel` (`rainTime`, `lightningTime` kept) | [`Weather`], [`WeatherLevels`] | restored |
//! | `worldStartCount` | — | counted down at every start, as BDS does |
//! | `scoreboard` | [`Scoreboard`] | restored |
//! | `mobevents` | [`MobEvents`] | restored |
//! | `tickingarea_<uuid>` | [`TickingAreas`] | restored, in key order |
//! | `structuretemplate_<name>` | [`Structures`] (`disk` saves) | restored |
//! | `WorldClocks` | [`WorldTime`] | written with the clock; other clocks kept |
//!
//! Every other `level.dat` field and every other record is kept as it was,
//! so a world BDS saved keeps its settings through unastar.
//!
//! Corrupt data is never replaced: an unreadable `level.dat`, `scoreboard`,
//! `mobevents` or `WorldClocks` stops the server before it runs, and an
//! unreadable ticking area or structure record is skipped with a warning
//! and left on disk (unastar writes and deletes only records it loaded or
//! made).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::IVec3;
use unastar_data::game_rules::GameRuleValue;

use crate::command::exec::{MobEvents, StructureTemplate, Structures, TickingArea, TickingAreas};
use crate::ecs::resources::TickCounter;
use crate::registry::BlockRegistry;
use crate::storage::StorageError;
use crate::storage::level::dat::{LevelDat, LevelDatError, NewWorld, UNSET_SPAWN_Y};
use crate::storage::level::nbt::Tag;
use crate::storage::level::records::{
    self, RecordError, StoredClocks, StoredMobEvents, StoredScoreboard, StoredStructure,
    StoredTickingArea,
};
use crate::storage::level::{LevelFileError, LevelFiles, LevelRecordStore, RawRecord};
use crate::world::rules::{Difficulty, GameRules, StartGameWorldState, WorldTime};
use crate::world::scoreboard::{PlayerUniqueId, Scoreboard};
use crate::world::weather::{Weather, WeatherLevels, WeatherState};

/// The world spawn a command set or the level data holds, if any.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LevelSpawn(pub Option<IVec3>);

/// Why the level data cannot be loaded or saved.
#[derive(Debug, thiserror::Error)]
pub enum LevelDataError {
    #[error(transparent)]
    File(#[from] LevelFileError),
    #[error(transparent)]
    Dat(#[from] LevelDatError),
    #[error("level records: {0}")]
    Storage(#[from] StorageError),
    #[error("level record {key}: {error}")]
    Record { key: String, error: RecordError },
    #[error("level data: {0}")]
    Invalid(String),
    #[error("writing level.dat: {0}")]
    Write(#[from] std::io::Error),
}

fn key_text(key: &[u8]) -> String {
    String::from_utf8_lossy(key).into_owned()
}

/// Where the world's level data lives, and what was loaded from it.
#[derive(Resource)]
pub struct LevelStorage {
    files: LevelFiles,
    records: Arc<dyn LevelRecordStore>,
    dat: LevelDat,
    clocks: StoredClocks,
    /// Ticking area and structure records unastar loaded or wrote; one whose
    /// area or structure is gone is deleted at the next save.
    owned: BTreeSet<Vec<u8>>,
}

/// What a world's level data holds, decoded and checked.
pub struct LoadedLevel {
    storage: LevelStorage,
    time: i64,
    current_tick: u64,
    rules: GameRules,
    spawn: Option<IVec3>,
    weather: (f32, f32),
    scoreboard: Option<StoredScoreboard>,
    mob_events: Option<StoredMobEvents>,
    areas: Vec<(String, StoredTickingArea)>,
    structures: Vec<(String, StoredStructure)>,
}

/// What a new world's level.dat says, from the configuration: BDS's flat
/// generator (2) for the flat and void worlds, its infinite one (1) with the
/// seed for generated terrain; the default game mode and difficulty.
pub fn new_world(config: &crate::server::ServerConfig) -> NewWorld {
    use crate::entity::components::GameMode;
    use crate::world::WorldGenerator;
    let (generator, seed) = match config.world.generator {
        WorldGenerator::SuperFlat | WorldGenerator::VoidSpawnPlatform { .. } => (2, 0),
        WorldGenerator::Java { seed } | WorldGenerator::Bedrock { seed } => (1, seed),
    };
    NewWorld {
        level_name: "default".into(),
        seed,
        generator,
        game_type: match config.default_gamemode {
            GameMode::Survival => 0,
            GameMode::Creative => 1,
            GameMode::Adventure => 2,
            GameMode::Spectator => 6,
        },
        difficulty: config.difficulty.wire() as i32,
    }
}

/// Reads and checks the world's level data: its level.dat (a fresh one
/// from `new_world` when there is none) and its level records.
pub async fn open(
    files: LevelFiles,
    records: Arc<dyn LevelRecordStore>,
    new_world: &NewWorld,
) -> Result<LoadedLevel, LevelDataError> {
    let dat = files.read()?;
    let raw = records.load_level_records().await?;
    decode(files, records, dat, raw, new_world)
}

fn decode(
    files: LevelFiles,
    records: Arc<dyn LevelRecordStore>,
    dat: Option<LevelDat>,
    raw: Vec<RawRecord>,
    new_world: &NewWorld,
) -> Result<LoadedLevel, LevelDataError> {
    let mut dat = dat.unwrap_or_else(|| LevelDat::fresh(new_world));
    let time = dat.long("Time")?.unwrap_or(0);
    if !(WorldTime::MIN..=WorldTime::MAX).contains(&time) {
        return Err(LevelDataError::Invalid(format!(
            "Time {time} is outside the clock's range"
        )));
    }
    let current_tick = dat.long("currentTick")?.unwrap_or(0);
    let current_tick = u64::try_from(current_tick)
        .map_err(|_| LevelDataError::Invalid(format!("currentTick {current_tick} is negative")))?;
    let rules = read_rules(&dat)?;
    let spawn = match (dat.int("SpawnX")?, dat.int("SpawnY")?, dat.int("SpawnZ")?) {
        (Some(x), Some(y), Some(z)) if y != UNSET_SPAWN_Y => Some(IVec3::new(x, y, z)),
        _ => None,
    };
    let level = |name: &str| -> Result<f32, LevelDataError> {
        let value = dat.float(name)?.unwrap_or(0.0);
        if !(0.0..=1.0).contains(&value) {
            return Err(LevelDataError::Invalid(format!(
                "{name} {value} is not in 0..=1"
            )));
        }
        Ok(value)
    };
    let weather = (level("rainLevel")?, level("lightningLevel")?);
    // BDS counts this down by one at every start (4294967294 after the
    // first start, 4294967293 after the second, measured).
    let starts = dat.long("worldStartCount")?.unwrap_or(i64::from(u32::MAX));
    dat.set("worldStartCount", Tag::Long((starts - 1).max(0)));

    let mut loaded = LoadedLevel {
        storage: LevelStorage {
            files,
            records,
            dat,
            clocks: StoredClocks::default(),
            owned: BTreeSet::new(),
        },
        time,
        current_tick,
        rules,
        spawn,
        weather,
        scoreboard: None,
        mob_events: None,
        areas: Vec::new(),
        structures: Vec::new(),
    };
    let single = |key: &[u8], error: RecordError| LevelDataError::Record {
        key: key_text(key),
        error,
    };
    for (key, value) in raw {
        match key.as_slice() {
            records::SCOREBOARD_KEY => {
                loaded.scoreboard =
                    Some(StoredScoreboard::decode(&value).map_err(|e| single(&key, e))?);
            }
            records::MOB_EVENTS_KEY => {
                loaded.mob_events =
                    Some(StoredMobEvents::decode(&value).map_err(|e| single(&key, e))?);
            }
            records::WORLD_CLOCKS_KEY => {
                loaded.storage.clocks =
                    StoredClocks::decode(&value).map_err(|e| single(&key, e))?;
            }
            _ if key.starts_with(records::TICKING_AREA_PREFIX) => {
                let uuid = key_text(&key[records::TICKING_AREA_PREFIX.len()..]);
                match StoredTickingArea::decode(&value) {
                    Ok(area) => {
                        loaded.storage.owned.insert(key);
                        loaded.areas.push((uuid, area));
                    }
                    Err(error) => tracing::warn!(
                        key = %key_text(&key),
                        %error,
                        "Ticking area record is unreadable; leaving it as it is"
                    ),
                }
            }
            _ if key.starts_with(records::STRUCTURE_PREFIX) => {
                let name = key_text(&key[records::STRUCTURE_PREFIX.len()..]);
                match StoredStructure::decode(&value) {
                    Ok(structure) => {
                        loaded.storage.owned.insert(key);
                        loaded.structures.push((name, structure));
                    }
                    Err(error) => tracing::warn!(
                        key = %key_text(&key),
                        %error,
                        "Structure record is unreadable; leaving it as it is"
                    ),
                }
            }
            _ => {}
        }
    }
    if let Some(board) = &loaded.scoreboard {
        // Checked now so a scoreboard that cannot be used stops the start.
        Scoreboard::from_stored(board.clone()).map_err(|error| LevelDataError::Record {
            key: key_text(records::SCOREBOARD_KEY),
            error: RecordError::Invalid(error),
        })?;
    }
    Ok(loaded)
}

/// Game rules from their lower-case level.dat names; a rule BDS does not
/// save (`locatorbar`) keeps its default.
fn read_rules(dat: &LevelDat) -> Result<GameRules, LevelDataError> {
    let mut rules = GameRules::default();
    for rule in unastar_data::game_rules::vanilla() {
        let key = rule.name.to_ascii_lowercase();
        let value = match rule.value {
            GameRuleValue::Bool(_) => match dat.byte(&key)? {
                None => continue,
                Some(0) => GameRuleValue::Bool(false),
                Some(1) => GameRuleValue::Bool(true),
                Some(other) => {
                    return Err(LevelDataError::Invalid(format!(
                        "game rule {key} is {other}, not 0 or 1"
                    )));
                }
            },
            GameRuleValue::Int(_) => match dat.int(&key)? {
                None => continue,
                Some(value) => GameRuleValue::Int(value),
            },
            GameRuleValue::Float(_) => match dat.float(&key)? {
                None => continue,
                Some(value) => GameRuleValue::Float(value),
            },
        };
        rules
            .set_value(&rule.name, value)
            .expect("a rule takes a value of its own type");
    }
    Ok(rules)
}

/// The weather saved levels stand for: thunder while lightning remains, rain
/// while rain does. [INFERENCE] BDS saves no weather state besides the
/// levels and countdowns; the measured thunderstorm (both levels 1) reloaded
/// as thunder.
fn weather_state((rain, lightning): (f32, f32)) -> WeatherState {
    if lightning > 0.0 {
        WeatherState::Thunder
    } else if rain > 0.0 {
        WeatherState::Rain
    } else {
        WeatherState::Clear
    }
}

impl LoadedLevel {
    /// Puts the level data into the world. Fails, before changing anything,
    /// on a structure whose blocks are not in the palette.
    pub fn apply(self, world: &mut World) -> Result<(), LevelDataError> {
        let blocks = BlockRegistry::vanilla();
        let mut structures = BTreeMap::new();
        let mut owned = self.storage.owned;
        for (name, stored) in &self.structures {
            match StructureTemplate::from_stored(stored, blocks) {
                Ok(template) => {
                    structures.insert(name.clone(), template);
                }
                Err(error) => {
                    tracing::warn!(
                        structure = %name,
                        %error,
                        "Saved structure cannot be loaded; leaving its record as it is"
                    );
                    owned.remove(&StoredStructure::key(name));
                }
            }
        }
        let scoreboard = self
            .scoreboard
            .map(|stored| Scoreboard::from_stored(stored).expect("checked when decoded"));

        world.insert_resource(WorldTime { ticks: self.time });
        world
            .get_resource_or_insert_with(TickCounter::default)
            .current = self.current_tick;
        world.insert_resource(self.rules);
        world.insert_resource(LevelSpawn(self.spawn));
        if let Some(block) = self.spawn {
            world.insert_resource(crate::entity::player_life::respawn::WorldSpawn(
                standing_in(block),
            ));
        }
        let state = weather_state(self.weather);
        world.insert_resource(Weather {
            state,
            duration: None,
        });
        world.insert_resource(WeatherLevels::restored(
            self.weather.0,
            self.weather.1,
            self.current_tick,
            state,
        ));
        if let Some(board) = scoreboard {
            world.insert_resource(board);
        }
        if let Some(events) = self.mob_events {
            world.insert_resource(MobEvents {
                enabled: events.events_enabled,
                pillager_patrols: events.pillager_patrols,
                wandering_trader: events.wandering_trader,
                ender_dragon: events.ender_dragon,
            });
        }
        world.insert_resource(TickingAreas::from_stored(
            self.areas
                .iter()
                .map(|(uuid, area)| TickingArea::from_stored(uuid, area))
                .collect(),
        ));
        world.insert_resource(Structures {
            templates: structures,
        });
        publish(world);
        world.insert_resource(LevelStorage {
            owned,
            ..self.storage
        });
        Ok(())
    }
}

/// A player standing in the middle of `block`, in the `Position`
/// convention (as `/setworldspawn` places the world spawn).
pub fn standing_in(block: IVec3) -> glam::DVec3 {
    block.as_dvec3() + glam::DVec3::new(0.5, crate::entity::PLAYER_EYE_HEIGHT, 0.5)
}

/// Hands the loaded clock, rules and spawn to the joins that build
/// StartGame off the game thread.
fn publish(world: &World) {
    if let Some(state) = world.get_resource::<StartGameWorldState>() {
        state.publish(
            *world.resource::<WorldTime>(),
            *world.resource::<Difficulty>(),
            world.resource::<GameRules>(),
        );
        state.set_world_spawn(world.resource::<LevelSpawn>().0);
        state.set_level_tick(world.resource::<TickCounter>().current);
    }
}

/// What a save writes, taken from the world in one go.
pub struct LevelSave {
    files: LevelFiles,
    records: Arc<dyn LevelRecordStore>,
    dat: LevelDat,
    level_name: String,
    puts: Vec<RawRecord>,
    deletes: Vec<Vec<u8>>,
}

/// Takes the level data from the world for a save; `None` when the world
/// has no level storage (persistence off).
pub fn capture(world: &mut World) -> Option<LevelSave> {
    let tick = world.get_resource::<TickCounter>().map_or(0, |t| t.current);
    let time = *world.resource::<WorldTime>();
    let difficulty = *world.resource::<Difficulty>();
    let rules = world.resource::<GameRules>().clone();
    let spawn = world
        .get_resource::<LevelSpawn>()
        .copied()
        .unwrap_or_default();
    let weather = world.get_resource::<Weather>().copied().unwrap_or_default();
    let levels = world
        .get_resource::<WeatherLevels>()
        .copied()
        .unwrap_or_default();
    let board = world.get_resource::<Scoreboard>().cloned();
    let events = world
        .get_resource::<MobEvents>()
        .copied()
        .unwrap_or_default();
    let areas = world
        .get_resource::<TickingAreas>()
        .cloned()
        .unwrap_or_default();
    let structures = world
        .get_resource::<Structures>()
        .cloned()
        .unwrap_or_default();
    let stored_board = board.map(|board| {
        board.to_stored(|entity| {
            if let Some(id) = world.get::<PlayerUniqueId>(entity) {
                return Some(records::StoredIdentity::Player(id.0));
            }
            world
                .get::<crate::entity::mob::persistence::MobUniqueId>(entity)
                .map(|id| records::StoredIdentity::Entity(id.0))
        })
    });
    let mut storage = world.get_resource_mut::<LevelStorage>()?;

    let dat = &mut storage.dat;
    dat.set("Time", Tag::Long(time.ticks));
    dat.set(
        "currentTick",
        Tag::Long(i64::try_from(tick).unwrap_or(i64::MAX)),
    );
    dat.set("Difficulty", Tag::Int(difficulty.wire() as i32));
    for (rule, value) in rules.iter() {
        let key = rule.name.to_ascii_lowercase();
        if !dat.root.contains_key(&key) {
            continue;
        }
        let tag = match value {
            GameRuleValue::Bool(on) => Tag::Byte(i8::from(on)),
            GameRuleValue::Int(v) => Tag::Int(v),
            GameRuleValue::Float(v) => Tag::Float(v),
        };
        dat.set(&key, tag);
    }
    if let Some(block) = spawn.0 {
        dat.set("SpawnX", Tag::Int(block.x));
        dat.set("SpawnY", Tag::Int(block.y));
        dat.set("SpawnZ", Tag::Int(block.z));
    }
    let (rain, lightning) = levels.at(tick);
    dat.set("rainLevel", Tag::Float(rain));
    dat.set("lightningLevel", Tag::Float(lightning));
    if let Some(duration) = weather.duration {
        // Measured for `/weather thunder 12345`: both countdowns hold the
        // duration. [INFERENCE] The same for rain and clear.
        dat.set("rainTime", Tag::Int(duration));
        dat.set("lightningTime", Tag::Int(duration));
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    dat.set("LastPlayed", Tag::Long(now));
    let level_name = match dat.root.get("LevelName") {
        Some(Tag::String(name)) => name.clone(),
        _ => "default".into(),
    };
    let dat = dat.clone();

    let mut puts = Vec::new();
    storage.clocks.set_overworld(time.wire());
    puts.push((records::WORLD_CLOCKS_KEY.to_vec(), storage.clocks.encode()));
    if let Some(board) = stored_board {
        puts.push((records::SCOREBOARD_KEY.to_vec(), board.encode()));
    }
    puts.push((
        records::MOB_EVENTS_KEY.to_vec(),
        StoredMobEvents {
            events_enabled: events.enabled,
            ender_dragon: events.ender_dragon,
            pillager_patrols: events.pillager_patrols,
            wandering_trader: events.wandering_trader,
        }
        .encode(),
    ));
    let mut owned = BTreeSet::new();
    for area in areas.areas() {
        let (uuid, stored) = area.to_stored();
        let key = StoredTickingArea::key(&uuid);
        owned.insert(key.clone());
        puts.push((key, stored.encode()));
    }
    let blocks = BlockRegistry::vanilla();
    for (name, template) in structures.templates.iter().filter(|(_, t)| t.disk) {
        let key = StoredStructure::key(name);
        owned.insert(key.clone());
        puts.push((key, template.to_stored(blocks).encode()));
    }
    let deletes = storage.owned.difference(&owned).cloned().collect();
    storage.owned = owned;
    Some(LevelSave {
        files: storage.files.clone(),
        records: storage.records.clone(),
        dat,
        level_name,
        puts,
        deletes,
    })
}

impl LevelSave {
    /// Writes level.dat (the previous one becomes `level.dat_old`) and the
    /// level records.
    pub async fn write(self) -> Result<(), LevelDataError> {
        let (files, dat, name) = (self.files, self.dat, self.level_name);
        tokio::task::spawn_blocking(move || files.write(&dat, &name))
            .await
            .map_err(|e| LevelDataError::Invalid(format!("join error: {e}")))??;
        self.records
            .save_level_records(self.puts, self.deletes)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
