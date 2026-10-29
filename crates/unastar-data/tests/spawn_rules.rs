use unastar_data::spawn_rules::{self, body};

#[test]
fn every_vanilla_spawn_rule_decodes() {
    let decoded = spawn_rules::decode_vanilla();
    assert!(decoded.len() >= 60, "{} vanilla spawn rules", decoded.len());
    let failures: Vec<String> = decoded
        .iter()
        .filter_map(|result| result.as_ref().err().map(ToString::to_string))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
    assert_eq!(spawn_rules::vanilla().len(), decoded.len());
}

#[test]
fn cow_rule_decodes_to_typed_conditions() {
    let cow = spawn_rules::vanilla()
        .iter()
        .find(|file| file.identifier() == "minecraft:cow")
        .expect("cow spawn rule");
    let rules = &cow.document.spawn_rules;
    assert_eq!(rules.description.population_control, "animal");
    let [condition] = rules.conditions.as_slice() else {
        panic!("one cow condition");
    };
    assert!(condition.spawns_on_surface.is_some());
    assert_eq!(condition.weight.as_ref().map(|w| w.default), Some(8));
    let brightness = condition.brightness_filter.as_ref().expect("brightness");
    assert_eq!((brightness.min, brightness.max), (7, 15));
    match &condition.herd {
        body::SpawnBiomeConditionsHerd::SpawnHerd(herd) => {
            assert_eq!((herd.min_size, herd.max_size), (2, 3));
        }
        other => panic!("cow herd {other:?}"),
    }
    match &condition.spawns_on_block_filter {
        body::SpawnBiomeConditionsSpawnsOnBlockFilter::BlockDescriptor(
            body::BlockDescriptor::String(name),
        ) => assert_eq!(name, "minecraft:grass_block"),
        other => panic!("cow block filter {other:?}"),
    }
    // Absent components keep their published defaults.
    assert!(condition.distance_filter.is_none());
    assert_eq!(condition.spawns_above_block_filter.distance, 1);
}
