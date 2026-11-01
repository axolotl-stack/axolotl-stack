use super::*;
use crate::config::SpawnLocation;

fn configured() -> UnastarConfig {
    let mut config = UnastarConfig {
        world: WorldConfig::void_spawn_platform(1),
        ..Default::default()
    };
    config.secondary_worlds.push(SecondaryWorldConfig {
        name: "lower".into(),
        data_dir: "worlds/lower".into(),
        adopt_existing: false,
        world: WorldConfig {
            dimension: 1,
            ..WorldConfig::void_spawn_platform(1)
        },
        spawn_rules: vec![SpawnRule {
            name: "fallback".into(),
            always_at_location: true,
            previous_position: true,
            location: Some(SpawnLocation::default()),
        }],
    });
    config
}

#[test]
fn configured_dimension_schema_roundtrips_names_directories_and_per_world_spawn_policy() {
    let config = configured();
    config.validate().unwrap();
    let text = toml_edit::ser::to_string(&config).unwrap();
    let decoded: UnastarConfig = toml_edit::de::from_str(&text).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.world_storage.name, "default");
    assert_eq!(
        decoded.world_storage.data_dir,
        PathBuf::from("worlds/default")
    );
    assert_eq!(decoded.secondary_worlds[0].world.dimension, 1);
    assert_eq!(decoded.spawn_rules_for(1).unwrap()[0].name, "fallback");
    assert!(decoded.spawn_rules_for(2).is_none());
    let mut unknown = serde_json::to_value(&config).unwrap();
    unknown["secondary_worlds"][0]["data_directory"] = "wrong".into();
    assert!(serde_json::from_value::<UnastarConfig>(unknown).is_err());
}

#[test]
fn configured_dimension_conflicts_malformed_spawns_and_missing_shared_accounts_fail_validation() {
    let base = configured();
    for kind in 0..10 {
        let mut config = base.clone();
        match kind {
            0 => config.secondary_worlds[0].world.dimension = 0,
            1 => config.secondary_worlds[0].world.dimension = 3,
            2 => config.secondary_worlds[0].name = "DEFAULT".into(),
            3 => config.secondary_worlds[0].name = " lower".into(),
            4 => config.secondary_worlds[0].data_dir = PathBuf::new(),
            5 => config.secondary_worlds[0].spawn_rules.clear(),
            6 => config.secondary_worlds[0].spawn_rules[0].always_at_location = false,
            7 => {
                config.secondary_worlds[0].spawn_rules[0]
                    .location
                    .as_mut()
                    .unwrap()
                    .x = f32::NAN
            }
            8 => {
                config.secondary_worlds[0].spawn_rules[0]
                    .location
                    .as_mut()
                    .unwrap()
                    .y = 128.0
            }
            9 => config.players.leveldb_enabled = false,
            _ => unreachable!(),
        }
        assert!(config.validate().is_err(), "accepted invalid case {kind}");
    }
    let mut config = base.clone();
    config.secondary_worlds[0].world.generator = WorldGenerator::JavaTerrain {
        seed: 7,
        preset: crate::world::generator::JavaDimension::End,
    };
    assert!(config.validate().is_err());
    let mut config = base;
    config
        .secondary_worlds
        .push(config.secondary_worlds[0].clone());
    assert!(config.validate().is_err());
}

#[test]
fn configured_rejoin_selects_the_saved_worlds_rules_in_their_actual_order() {
    let mut config = configured();
    config.spawn_rules[0].previous_position = false;
    assert_eq!(
        crate::server::connect::select_join_dimension(&config, 0, Some(1), None),
        1
    );
    config.secondary_worlds[0].spawn_rules[0].previous_position = false;
    assert_eq!(
        crate::server::connect::select_join_dimension(&config, 0, Some(1), None),
        0
    );
    config.secondary_worlds[0].spawn_rules[0].previous_position = true;
    assert_eq!(
        crate::server::connect::select_join_dimension(&config, 0, None, Some(1)),
        1
    );
}
