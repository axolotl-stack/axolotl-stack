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
fn difficulty_defaults_to_easy_and_reaches_runtime_configuration() {
    use crate::world::rules::Difficulty;
    let file = ConfigFile::new();
    std::fs::write(&file.0, VOID_WORLD).unwrap();
    let config = UnastarConfig::load_or_create(&file.0).unwrap();
    assert_eq!(config.server_config().difficulty, Difficulty::Easy);

    std::fs::write(
        &file.0,
        format!("{VOID_WORLD}[server]\ndifficulty = 'hard'"),
    )
    .unwrap();
    let config = UnastarConfig::load_or_create(&file.0).unwrap();
    assert_eq!(config.server_config().difficulty, Difficulty::Hard);

    std::fs::write(
        &file.0,
        format!("{VOID_WORLD}[server]\ndifficulty = 'brutal'"),
    )
    .unwrap();
    assert!(matches!(
        UnastarConfig::load_or_create(&file.0),
        Err(ConfigError::Parse { .. })
    ));
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
            WorldGenerator::Bedrock { seed: 42 },
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

#[test]
fn bedrock_config_requires_the_build_time_pack_feature() {
    // Start from a generator that needs no build-time game files, so the first check
    // holds in every feature combination (the default Java-style world needs
    // `java-worldgen`).
    let mut config = UnastarConfig::default();
    config.world.generator = crate::world::WorldGenerator::SuperFlat;
    assert!(config.validate().is_ok());
    config.world.generator = crate::world::WorldGenerator::Bedrock { seed: 12345 };
    assert_eq!(
        config.validate().is_ok(),
        cfg!(feature = "bedrock-worldgen")
    );
}

#[test]
fn nethernet_is_optional_and_has_explicit_bounded_network_defaults() {
    let config: UnastarConfig = toml_edit::de::from_str(VOID_WORLD).unwrap();
    assert!(config.server.nethernet.is_none());
    assert!(config.server.online_mode);
    let config: UnastarConfig =
        toml_edit::de::from_str(&format!("{VOID_WORLD}[server.nethernet]\n")).unwrap();
    let endpoint = config.server.nethernet.as_ref().unwrap();
    assert_eq!(
        endpoint.bind_address,
        "127.0.0.1:18086".parse::<std::net::SocketAddr>().unwrap()
    );
    assert_eq!(endpoint.negotiation_timeout_seconds, 15);
    assert_eq!(endpoint.max_pending_connections, 64);
    assert!(endpoint.signing_key_path.is_none());
    assert!(config.server.online_mode);
    assert!(config.validate_network().is_ok());
    let roundtrip = toml_edit::ser::to_string(&config).unwrap();
    let loaded: UnastarConfig = toml_edit::de::from_str(&roundtrip).unwrap();
    assert_eq!(
        loaded.server.nethernet.unwrap().bind_address,
        endpoint.bind_address
    );
    for values in [
        "negotiation_timeout_seconds = 0",
        "negotiation_timeout_seconds = 121",
        "max_pending_connections = 0",
        "max_pending_connections = 4097",
    ] {
        let config: UnastarConfig =
            toml_edit::de::from_str(&format!("{VOID_WORLD}[server.nethernet]\n{values}\n"))
                .unwrap();
        assert!(config.validate_network().is_err(), "{values}");
    }
    for timeout in [0, 301] {
        let mut config = config.clone();
        config.server.handshake_timeout_seconds = timeout;
        assert!(config.validate_network().is_err());
    }
    assert!(
        toml_edit::de::from_str::<UnastarConfig>(&format!(
            "{VOID_WORLD}[server.nethernet]\nallow_identityless_client=true"
        ))
        .is_err()
    );
}

#[test]
fn ice_configuration_is_bounded_and_secrets_are_not_in_debug_output() {
    let mut config = UnastarConfig::default();
    let endpoint = config.server.nethernet.insert(NetherNetConfig::default());
    endpoint.relay_only = true;
    assert!(config.validate_network().is_err());
    let endpoint = config.server.nethernet.as_mut().unwrap();
    endpoint.ice_servers.push(IceServerConfig {
        urls: vec!["turn:relay.example.org:3478".into()],
        username: "local-test-user".into(),
        password: "local-test-password".into(),
    });
    assert!(!format!("{endpoint:?}").contains("local-test"));
    assert!(config.validate_network().is_ok());
    config.server.nethernet.as_mut().unwrap().ice_servers[0].urls[0] =
        "https://not-ice.example.org".into();
    assert!(config.validate_network().is_err());
    config.server.nethernet.as_mut().unwrap().ice_servers[0]
        .urls
        .clear();
    assert!(config.validate_network().is_err());
}

#[test]
fn configured_signing_key_path_roundtrips_and_rejects_empty_paths() {
    let config: UnastarConfig = toml_edit::de::from_str(&format!(
        "{VOID_WORLD}[server.nethernet]\nsigning_key_path = \"data/server.der\"\n"
    ))
    .unwrap();
    assert_eq!(
        config
            .server
            .nethernet
            .as_ref()
            .unwrap()
            .signing_key_path
            .as_deref(),
        Some(std::path::Path::new("data/server.der"))
    );
    assert!(config.validate_network().is_ok());
    let encoded = toml_edit::ser::to_string(&config).unwrap();
    let loaded: UnastarConfig = toml_edit::de::from_str(&encoded).unwrap();
    assert_eq!(
        loaded.server.nethernet.unwrap().signing_key_path,
        config.server.nethernet.unwrap().signing_key_path
    );
    for path in ["", "/", "."] {
        let mut config = UnastarConfig::default();
        config.server.nethernet = Some(NetherNetConfig {
            signing_key_path: Some(path.into()),
            ..Default::default()
        });
        assert!(config.validate_network().is_err());
    }
}
