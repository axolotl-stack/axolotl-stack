use super::*;
use crate::world::StorageProvider;
use std::sync::atomic::{AtomicU64, Ordering};

struct ConfigFile(PathBuf);

impl ConfigFile {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let directory = std::env::temp_dir().join(format!(
            "unastar-config-{}-{}",
            std::process::id(),
            NEXT_ID.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&directory).unwrap();
        Self(directory.join("unastar.toml"))
    }
}

impl Drop for ConfigFile {
    fn drop(&mut self) {
        std::fs::remove_dir_all(self.0.parent().unwrap()).unwrap();
    }
}

/// The default generator is Java-style, which needs the `java-worldgen` templates.
#[cfg(feature = "java-worldgen")]
#[test]
fn generated_and_shipped_configs_load_with_the_same_safe_defaults() {
    let file = ConfigFile::new();
    let generated = UnastarConfig::load_or_create(&file.0).unwrap();
    let defaults = toml_edit::ser::to_string(&UnastarConfig::default()).unwrap();
    assert_eq!(toml_edit::ser::to_string(&generated).unwrap(), defaults);
    assert!(generated.server.online_mode);

    for sample in [
        include_str!("../../../../unastar.toml"),
        include_str!("../../unastar.toml"),
        "",
    ] {
        std::fs::write(&file.0, sample).unwrap();
        let config = UnastarConfig::load_or_create(&file.0).unwrap();
        assert!(config.server.online_mode);
        assert_eq!(toml_edit::ser::to_string(&config).unwrap(), defaults);
        assert_eq!(
            config.server_config().world,
            generated.server_config().world
        );
    }
}

/// Without `java-worldgen` the default (Java-style) world is refused with an error that
/// names the feature and the jar, instead of starting a world without structures.
#[cfg(not(feature = "java-worldgen"))]
#[test]
fn default_java_world_requires_the_java_worldgen_feature() {
    let file = ConfigFile::new();
    match UnastarConfig::load_or_create(&file.0) {
        Err(ConfigError::Validation(message)) => {
            assert!(message.contains("java-worldgen"), "{message}");
            assert!(message.contains("UNASTAR_JAVA_SERVER_JAR"), "{message}");
        }
        other => panic!("default Java-style config loaded without java-worldgen: {other:?}"),
    }
}

/// Settings tests below pick a generator that needs no build-time game files.
const VOID_WORLD: &str =
    "[world.generator]\nkind = 'void_spawn_platform'\nplatform_radius_chunks = 1\n";

#[test]
fn unknown_config_keys_are_rejected_instead_of_silently_ignored() {
    let file = ConfigFile::new();
    for source in [
        "unexpected = true",
        "[server]\nonline_mod = false",
        "[world]\nstorage_providor = 'blaze_db'",
        "[world.storage]\ndata_dir = 'custom-worlds'",
        "[world.storage]\nleveldb_enabled = false",
        "[world.storage]\nsave_on_shutdown = false",
        "[world.bounds]\nmode = 'infinite'\nradius_chunks = 2",
        "[world.bounds]\nmode = 'radius'\nradius_chunks = 2\nraduis_chunks = 3",
        "[world.generator]\nkind = 'void_spawn_platform'\nplatform_radius_chunks = 1\nplatform_radius_chunk = 4",
        "[world.generator]\nkind = 'vanilla'\nseed = 42\nunexpected = true",
        "[world.generator]\nkind = 'super_flat'\nseed = 42",
        "[players]\nsave_on_disconect = false",
        "[[spawn_rules]]\nprevious_postion = true",
        "[[spawn_rules]]\nlocation = { x = 1.0, y = 2.0, z = 3.0, yaaw = 90.0 }",
    ] {
        std::fs::write(&file.0, source).unwrap();
        assert!(
            matches!(
                UnastarConfig::load_or_create(&file.0),
                Err(ConfigError::Parse { .. })
            ),
            "accepted unknown config key: {source}",
        );
    }
}

#[test]
fn storage_settings_reach_runtime_configuration() {
    let file = ConfigFile::new();
    for (provider, expected) in [
        ("level_db", StorageProvider::LevelDb),
        ("blaze_db", StorageProvider::BlazeDb),
    ] {
        std::fs::write(
            &file.0,
            format!(
                "{VOID_WORLD}[world]\nstorage_provider = '{provider}'\nblazedb_cache_chunks = 73\n\
                 [players]\ndata_dir = 'custom-playerdata'\nleveldb_enabled = false\n\
                 save_on_disconnect = false\nsave_previous_position = false"
            ),
        )
        .unwrap();
        let config = UnastarConfig::load_or_create(&file.0).unwrap();
        let runtime = config.server_config();
        assert_eq!(runtime.world.storage_provider, expected);
        assert_eq!(runtime.world.blazedb_cache_chunks, 73);
        assert_eq!(config.players.data_dir, PathBuf::from("custom-playerdata"));
        assert!(!config.players.leveldb_enabled);
        assert!(!config.players.save_on_disconnect);
        assert!(!config.players.save_previous_position);
    }
}

#[test]
fn offline_mode_requires_an_explicit_opt_out() {
    let file = ConfigFile::new();
    std::fs::write(
        &file.0,
        format!("{VOID_WORLD}[server]\nonline_mode = false"),
    )
    .unwrap();
    assert!(
        !UnastarConfig::load_or_create(&file.0)
            .unwrap()
            .server
            .online_mode
    );
}

#[test]
fn strict_world_variants_still_load_valid_options() {
    use crate::world::{WorldBounds, WorldGenerator};
    for bounds in [
        WorldBounds::Infinite,
        WorldBounds::Radius { radius_chunks: 2 },
        WorldBounds::Rect {
            min_x: -1,
            max_x: 2,
            min_z: -3,
            max_z: 4,
        },
    ] {
        for generator in [
            WorldGenerator::SuperFlat,
            WorldGenerator::VoidSpawnPlatform {
                platform_radius_chunks: 2,
            },
            WorldGenerator::Java { seed: 42 },
        ] {
            let world = WorldConfig {
                bounds,
                generator,
                ..WorldConfig::default()
            };
            let source = toml_edit::ser::to_string(&world).unwrap();
            let decoded: WorldConfig = toml_edit::de::from_str(&source).unwrap();
            assert_eq!(decoded, world);
        }
    }
}
