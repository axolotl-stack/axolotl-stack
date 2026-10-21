//! Consumer-visible identity checks against pinned BDS 1.26.51 measurements.

use unastar_data::{biomes, blocks, items};

#[test]
fn block_lookup_uses_1_26_51_palette_ordinals() {
    let stone = blocks::get("minecraft:stone").unwrap();
    assert_eq!(stone.default_state_id, 3317);
    assert_eq!(blocks::by_runtime_id(3317), Some(stone));
    // The Bedrock identifier for the oak door is wooden_door.
    let door = blocks::get("minecraft:wooden_door").unwrap();
    assert_eq!((door.min_state_id, door.max_state_id), (7380, 7411));
    assert_eq!(blocks::by_runtime_id(7411), Some(door));
    assert_ne!(blocks::by_runtime_id(7412), Some(door));
    assert_eq!(
        blocks::get("minecraft:air").unwrap().default_state_id,
        17025
    );
    assert!(blocks::by_runtime_id(22091).is_none());
}

#[test]
fn block_physics_use_native_measurements_not_prismarine_scaling() {
    let stone = blocks::get("minecraft:stone").unwrap();
    assert_eq!(stone.hardness, 1.5);
    assert_eq!(stone.resistance, 6.0);
    assert_eq!(
        blocks::get("minecraft:cyan_terracotta").unwrap().resistance,
        1.25
    );
    assert_eq!(stone.filter_light, 15);
    assert!(!stone.is_transparent);
    assert!(
        blocks::get("minecraft:wooden_door")
            .unwrap()
            .tags
            .contains(&"wood")
    );
}

#[test]
fn native_items_preserve_signed_network_ids_and_measured_stacks() {
    let button = items::get("minecraft:acacia_button").unwrap();
    assert_eq!(button.network_id, -140);
    assert_eq!(
        items::get("minecraft:honey_bottle").unwrap().max_stack_size,
        16
    );
    let properties: serde_json::Value =
        serde_json::from_str(button.native_properties_json).unwrap();
    assert_eq!(properties["maxDamage"], 0);
}

#[test]
fn new_native_biomes_keep_their_wire_ids() {
    assert_eq!(biomes::get("minecraft:sulfur_caves").unwrap().id, 194);
    assert_eq!(biomes::get("minecraft:dappled_forest").unwrap().id, 195);
}
