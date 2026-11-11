//! `level.dat`: the world's settings file, as BDS 1.26.51 writes it.
//!
//! Layout (measured): a little-endian `i32` storage version (10), a
//! little-endian `i32` byte length of what follows, then one rooted NBT
//! compound with an empty name. The compound holds the world settings
//! (`LevelName`, `RandomSeed`, `Generator`, ...), the clock (`Time`,
//! `currentTick`), `Difficulty`, the world spawn (`SpawnX/Y/Z`, `SpawnY`
//! 32767 while no spawn was set), the weather (`rainLevel`, `rainTime`,
//! `lightningLevel`, `lightningTime`), every game rule under its lower-case
//! name (`keepinventory`, `playerwaypoints` as an int: 1 everyone, 0 off;
//! `locatorbar` is not saved) and `worldStartCount`, which counts down by one
//! every time the world starts.
//!
//! [`LevelDat`] keeps the whole compound, fields unastar does not use
//! included, so a world BDS saved keeps every setting when unastar saves it
//! again.

use super::nbt::{self, Compound, NbtError, Tag};

/// The storage version BDS 1.26.51 writes in the header (and in
/// `StorageVersion`).
pub const STORAGE_VERSION: i32 = 10;

/// `SpawnY` of a world whose spawn was never set: BDS looks for a safe
/// height at `SpawnX`, `SpawnZ`.
pub const UNSET_SPAWN_Y: i32 = 32767;

/// Why a `level.dat` cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LevelDatError {
    #[error("level.dat is {0} bytes, shorter than its 8-byte header")]
    Short(usize),
    #[error("level.dat header says {header} bytes follow, but {actual} do")]
    Length { header: i32, actual: usize },
    #[error("level.dat storage version {0} is not one unastar reads (1 to {STORAGE_VERSION})")]
    Version(i32),
    #[error("level.dat: {0}")]
    Nbt(#[from] NbtError),
    #[error("level.dat field {name:?}: {problem}")]
    Field { name: String, problem: String },
}

/// One `level.dat`: its header's storage version and its compound.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelDat {
    pub storage_version: i32,
    pub root: Compound,
}

impl LevelDat {
    /// Parses a whole file; the header's length must cover exactly the
    /// compound that follows.
    pub fn read(bytes: &[u8]) -> Result<Self, LevelDatError> {
        if bytes.len() < 8 {
            return Err(LevelDatError::Short(bytes.len()));
        }
        let storage_version = i32::from_le_bytes(bytes[0..4].try_into().expect("4 bytes"));
        let length = i32::from_le_bytes(bytes[4..8].try_into().expect("4 bytes"));
        if usize::try_from(length).ok() != Some(bytes.len() - 8) {
            return Err(LevelDatError::Length {
                header: length,
                actual: bytes.len() - 8,
            });
        }
        if !(1..=STORAGE_VERSION).contains(&storage_version) {
            return Err(LevelDatError::Version(storage_version));
        }
        let root = nbt::read_record(&bytes[8..])?;
        Ok(Self {
            storage_version,
            root,
        })
    }

    /// The file's bytes: header, then the compound (keys in byte order, as
    /// BDS writes them).
    pub fn write(&self) -> Vec<u8> {
        let body = nbt::write_record(&self.root);
        let mut out = Vec::with_capacity(body.len() + 8);
        out.extend_from_slice(&self.storage_version.to_le_bytes());
        out.extend_from_slice(
            &i32::try_from(body.len())
                .expect("level.dat fits an i32 length")
                .to_le_bytes(),
        );
        out.extend_from_slice(&body);
        out
    }

    fn field_error(name: &str, problem: impl Into<String>) -> LevelDatError {
        LevelDatError::Field {
            name: name.to_owned(),
            problem: problem.into(),
        }
    }

    /// A field of one type; `Ok(None)` when absent, an error for another type.
    fn get<'a, T>(
        &'a self,
        name: &str,
        kind: &str,
        pick: impl FnOnce(&'a Tag) -> Option<T>,
    ) -> Result<Option<T>, LevelDatError> {
        match self.root.get(name) {
            None => Ok(None),
            Some(tag) => pick(tag)
                .map(Some)
                .ok_or_else(|| Self::field_error(name, format!("is not {kind}"))),
        }
    }

    pub fn byte(&self, name: &str) -> Result<Option<i8>, LevelDatError> {
        self.get(name, "a Byte", |t| match t {
            Tag::Byte(v) => Some(*v),
            _ => None,
        })
    }

    pub fn int(&self, name: &str) -> Result<Option<i32>, LevelDatError> {
        self.get(name, "an Int", |t| match t {
            Tag::Int(v) => Some(*v),
            _ => None,
        })
    }

    pub fn long(&self, name: &str) -> Result<Option<i64>, LevelDatError> {
        self.get(name, "a Long", |t| match t {
            Tag::Long(v) => Some(*v),
            _ => None,
        })
    }

    pub fn float(&self, name: &str) -> Result<Option<f32>, LevelDatError> {
        self.get(name, "a Float", |t| match t {
            Tag::Float(v) => Some(*v),
            _ => None,
        })
    }

    pub fn set(&mut self, name: &str, tag: Tag) {
        self.root.insert(name.to_owned(), tag);
    }

    /// The level.dat of a world BDS 1.26.51 has just created: every field
    /// and value of the measured fresh file (flat generator, seed 12345,
    /// creative, easy, member permission level) except the values `world`
    /// gives. Settings that BDS takes from `server.properties` at start
    /// (difficulty, game type) are written as `world` says.
    pub fn fresh(world: &NewWorld) -> Self {
        let mut root = Compound::new();
        for (name, tag) in fresh_fields() {
            root.insert(name.to_owned(), tag);
        }
        let mut dat = Self {
            storage_version: STORAGE_VERSION,
            root,
        };
        dat.set("LevelName", Tag::String(world.level_name.clone()));
        dat.set("RandomSeed", Tag::Long(world.seed));
        dat.set("Generator", Tag::Int(world.generator));
        dat.set("GameType", Tag::Int(world.game_type));
        dat.set("Difficulty", Tag::Int(world.difficulty));
        dat.set("Time", Tag::Long(0));
        dat.set("currentTick", Tag::Long(0));
        // The next start counts down from the largest value (BDS's first
        // start writes 4294967294).
        dat.set("worldStartCount", Tag::Long(i64::from(u32::MAX)));
        dat
    }
}

/// What a new world's level.dat says about the world.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewWorld {
    pub level_name: String,
    pub seed: i64,
    /// `Generator`: 1 infinite (BDS's default world), 2 flat.
    pub generator: i32,
    /// `GameType`: the default game mode's wire value.
    pub game_type: i32,
    /// `Difficulty`: 0 peaceful to 3 hard.
    pub difficulty: i32,
}

fn compound(fields: &[(&str, Tag)]) -> Tag {
    Tag::Compound(
        fields
            .iter()
            .map(|(name, tag)| ((*name).to_owned(), tag.clone()))
            .collect(),
    )
}

fn ints(values: &[i32]) -> Tag {
    Tag::list(values.iter().copied().map(Tag::Int).collect())
}

/// BDS 1.26.51's level.dat for a new flat world (seed 12345, creative, easy,
/// members), measured with `tools/bds-sweep/level_data.py` after the first
/// start; `LastPlayed`, `rainTime` and `lightningTime` are that start's.
fn fresh_fields() -> Vec<(&'static str, Tag)> {
    let b = Tag::Byte;
    let i = Tag::Int;
    vec![
        ("BiomeOverride", Tag::String("minecraft:".into())),
        ("CenterMapsToOrigin", b(0)),
        ("ConfirmedPlatformLockedContent", b(0)),
        ("Difficulty", i(1)),
        ("FlatWorldLayers", Tag::String(FLAT_WORLD_LAYERS.into())),
        ("ForceGameType", b(0)),
        ("GameType", i(1)),
        ("Generator", i(2)),
        ("HasUncompleteWorldFileOnDisk", b(0)),
        ("InventoryVersion", Tag::String("1.26.51".into())),
        ("IsHardcore", b(0)),
        ("LANBroadcast", b(1)),
        ("LANBroadcastIntent", b(1)),
        ("LastPlayed", Tag::Long(1_794_360_938)),
        ("LevelName", Tag::String("level-data".into())),
        ("LimitedWorldOriginX", i(0)),
        ("LimitedWorldOriginY", i(32767)),
        ("LimitedWorldOriginZ", i(0)),
        ("MinimumCompatibleClientVersion", ints(&[1, 26, 50, 0, 0])),
        ("MultiplayerGame", b(1)),
        ("MultiplayerGameIntent", b(0)),
        ("NetherScale", i(8)),
        ("NetworkVersion", i(2193)),
        ("Platform", i(2)),
        ("PlatformBroadcastIntent", i(0)),
        ("PlayerHasDied", b(0)),
        ("RandomSeed", Tag::Long(12345)),
        ("SpawnV1Villagers", b(0)),
        ("SpawnX", i(0)),
        ("SpawnY", i(UNSET_SPAWN_Y)),
        ("SpawnZ", i(0)),
        ("StorageVersion", i(STORAGE_VERSION)),
        ("Time", Tag::Long(4)),
        ("WorldVersion", i(1)),
        ("XBLBroadcastIntent", i(0)),
        (
            "abilities",
            compound(&[
                ("attackmobs", b(1)),
                ("attackplayers", b(1)),
                ("build", b(1)),
                ("doorsandswitches", b(1)),
                ("flySpeed", Tag::Float(0.05)),
                ("flying", b(0)),
                ("instabuild", b(0)),
                ("invulnerable", b(0)),
                ("lightning", b(0)),
                ("mayfly", b(0)),
                ("mine", b(1)),
                ("op", b(0)),
                ("opencontainers", b(1)),
                ("teleport", b(0)),
                ("verticalFlySpeed", Tag::Float(1.0)),
                ("walkSpeed", Tag::Float(0.1)),
            ]),
        ),
        ("allowAnonymousBlockDropsInEditorWorlds", b(0)),
        ("bonusChestEnabled", b(0)),
        ("bonusChestSpawned", b(0)),
        ("cheatsEnabled", b(0)),
        ("commandblockoutput", b(1)),
        ("commandblocksenabled", b(1)),
        ("commandsEnabled", b(1)),
        ("currentTick", Tag::Long(4)),
        ("daylightCycle", i(0)),
        ("dodaylightcycle", b(1)),
        ("doentitydrops", b(1)),
        ("dofiretick", b(1)),
        ("doimmediaterespawn", b(0)),
        ("doinsomnia", b(1)),
        ("dolimitedcrafting", b(0)),
        ("domobloot", b(1)),
        ("domobspawning", b(1)),
        ("dotiledrops", b(1)),
        ("doweathercycle", b(1)),
        ("drowningdamage", b(1)),
        ("editorWorldType", i(0)),
        ("eduOffer", i(0)),
        ("educationFeaturesEnabled", b(0)),
        (
            "experiments",
            compound(&[
                ("experiments_ever_used", b(0)),
                ("saved_with_toggled_experiments", b(0)),
            ]),
        ),
        ("falldamage", b(1)),
        ("firedamage", b(1)),
        ("freezedamage", b(1)),
        ("functioncommandlimit", i(10000)),
        ("hasBeenLoadedInCreative", b(1)),
        ("hasLockedBehaviorPack", b(0)),
        ("hasLockedResourcePack", b(0)),
        ("immutableWorld", b(0)),
        ("isCreatedInEditor", b(0)),
        ("isExportedFromEditor", b(0)),
        ("isFromLockedTemplate", b(0)),
        ("isFromWorldTemplate", b(0)),
        ("isRandomSeedAllowed", b(0)),
        ("isSingleUseWorld", b(0)),
        ("isWorldTemplateOptionLocked", b(0)),
        ("keepinventory", b(0)),
        ("lastOpenedWithVersion", ints(&[1, 26, 51, 1, 0])),
        ("lightningLevel", Tag::Float(0.0)),
        ("lightningTime", i(21377)),
        ("limitedWorldDepth", i(16)),
        ("limitedWorldWidth", i(16)),
        ("maxcommandchainlength", i(65535)),
        ("mobgriefing", b(1)),
        ("naturalregeneration", b(1)),
        ("permissionsLevel", i(0)),
        ("playerPermissionsLevel", i(1)),
        ("playerssleepingpercentage", i(100)),
        ("playerwaypoints", i(1)),
        ("prid", Tag::String(String::new())),
        ("projectilescanbreakblocks", b(1)),
        ("pvp", b(1)),
        ("rainLevel", Tag::Float(0.0)),
        ("rainTime", i(162_686)),
        ("randomtickspeed", i(1)),
        ("recipesunlock", b(1)),
        ("requiresCopiedPackRemovalCheck", b(0)),
        ("respawnblocksexplode", b(1)),
        ("sendcommandfeedback", b(1)),
        ("serverChunkTickRange", i(4)),
        ("serverEditorConnectionPolicy", i(0)),
        ("showbordereffect", b(1)),
        ("showcoordinates", b(0)),
        ("showdaysplayed", b(0)),
        ("showdeathmessages", b(1)),
        ("showrecipemessages", b(1)),
        ("showtags", b(1)),
        ("spawnMobs", b(1)),
        ("spawnradius", i(10)),
        ("startWithMapEnabled", b(0)),
        ("texturePacksRequired", b(0)),
        ("tntexplodes", b(1)),
        ("tntexplosiondropdecay", b(0)),
        ("useMsaGamertagsOnly", b(1)),
        ("worldStartCount", Tag::Long(4_294_967_294)),
        ("world_policies", compound(&[])),
    ]
}

/// `FlatWorldLayers` of BDS's default flat world (one bedrock, two dirt and
/// one grass block layer), as its first start writes it.
const FLAT_WORLD_LAYERS: &str = "{\"biome_id\":1,\"block_layers\":[{\"block_name\":\"minecraft:bedrock\",\"count\":1},{\"block_name\":\"minecraft:dirt\",\"count\":2},{\"block_name\":\"minecraft:grass_block\",\"count\":1}],\"encoding_version\":6,\"structure_options\":null,\"world_version\":\"version.post_1_18\"}\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_header_must_match_the_compound() {
        let dat = LevelDat::fresh(&NewWorld {
            level_name: "default".into(),
            seed: 1,
            generator: 1,
            game_type: 0,
            difficulty: 2,
        });
        let bytes = dat.write();
        assert_eq!(&bytes[0..4], &10i32.to_le_bytes());
        assert_eq!(
            i32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize,
            bytes.len() - 8
        );
        assert_eq!(LevelDat::read(&bytes).unwrap(), dat);

        let mut longer = bytes.clone();
        longer.push(0);
        assert!(matches!(
            LevelDat::read(&longer),
            Err(LevelDatError::Length { .. })
        ));
        assert_eq!(LevelDat::read(&bytes[..5]), Err(LevelDatError::Short(5)));
        let mut version = bytes.clone();
        version[0..4].copy_from_slice(&11i32.to_le_bytes());
        assert_eq!(LevelDat::read(&version), Err(LevelDatError::Version(11)));
    }

    #[test]
    fn typed_fields_reject_other_types() {
        let mut dat = LevelDat::fresh(&NewWorld {
            level_name: "w".into(),
            seed: 0,
            generator: 2,
            game_type: 1,
            difficulty: 1,
        });
        assert_eq!(dat.long("Time"), Ok(Some(0)));
        assert_eq!(dat.int("SpawnY"), Ok(Some(UNSET_SPAWN_Y)));
        assert_eq!(dat.int("nothing"), Ok(None));
        dat.set("Time", Tag::Int(5));
        assert!(matches!(dat.long("Time"), Err(LevelDatError::Field { .. })));
    }
}
