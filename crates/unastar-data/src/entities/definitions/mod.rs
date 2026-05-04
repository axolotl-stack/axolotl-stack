//! Generated entity definitions.
pub mod allay;
pub mod area_effect_cloud;
pub mod armadillo;
pub mod armor_stand;
pub mod arrow;
pub mod axolotl;
pub mod bat;
pub mod bee;
pub mod blaze;
pub mod boat;
pub mod bogged;
pub mod breeze;
pub mod breeze_wind_charge_projectile;
pub mod camel;
pub mod camel_husk;
pub mod cat;
pub mod cave_spider;
pub mod chest_boat;
pub mod chest_minecart;
pub mod chicken;
pub mod cod;
pub mod command_block_minecart;
pub mod copper_golem;
pub mod cow;
pub mod creaking;
pub mod creeper;
pub mod dolphin;
pub mod donkey;
pub mod dragon_fireball;
pub mod drowned;
pub mod egg;
pub mod elder_guardian;
pub mod ender_crystal;
pub mod ender_dragon;
pub mod ender_pearl;
pub mod enderman;
pub mod endermite;
pub mod evocation_illager;
pub mod eye_of_ender_signal;
pub mod fireball;
pub mod fireworks_rocket;
pub mod fishing_hook;
pub mod fox;
pub mod frog;
pub mod ghast;
pub mod glow_squid;
pub mod goat;
pub mod guardian;
pub mod happy_ghast;
pub mod hoglin;
pub mod hopper_minecart;
pub mod horse;
pub mod husk;
pub mod iron_golem;
pub mod lightning_bolt;
pub mod lingering_potion;
pub mod llama;
pub mod llama_spit;
pub mod magma_cube;
pub mod minecart;
pub mod mooshroom;
pub mod mule;
pub mod nautilus;
pub mod npc;
pub mod ocelot;
pub mod ominous_item_spawner;
pub mod panda;
pub mod parched;
pub mod parrot;
pub mod phantom;
pub mod pig;
pub mod piglin;
pub mod piglin_brute;
pub mod pillager;
pub mod player;
pub mod polar_bear;
pub mod pufferfish;
pub mod rabbit;
pub mod ravager;
pub mod salmon;
pub mod sheep;
pub mod shulker;
pub mod shulker_bullet;
pub mod silverfish;
pub mod skeleton;
pub mod skeleton_horse;
pub mod slime;
pub mod small_fireball;
pub mod sniffer;
pub mod snow_golem;
pub mod snowball;
pub mod spider;
pub mod splash_potion;
pub mod squid;
pub mod stray;
pub mod strider;
pub mod tadpole;
pub mod thrown_trident;
pub mod tnt;
pub mod tnt_minecart;
pub mod trader_llama;
pub mod tripod_camera;
pub mod tropicalfish;
pub mod turtle;
pub mod vex;
pub mod villager;
pub mod villager_v2;
pub mod vindicator;
pub mod wandering_trader;
pub mod warden;
pub mod wind_charge_projectile;
pub mod witch;
pub mod wither;
pub mod wither_skeleton;
pub mod wither_skull;
pub mod wither_skull_dangerous;
pub mod wolf;
pub mod xp_bottle;
pub mod xp_orb;
pub mod zoglin;
pub mod zombie;
pub mod zombie_horse;
pub mod zombie_nautilus;
pub mod zombie_pigman;
pub mod zombie_villager;
pub mod zombie_villager_v2;
/// Generated behavior-pack entity metadata.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityDefinitionData {
    /// Internal registry ID assigned by deterministic codegen order.
    ///
    /// This is not a Bedrock packet/runtime entity ID.
    pub id: u32,
    /// Namespaced entity identifier.
    pub identifier: &'static str,
    /// Vanilla spawn category, when present.
    pub spawn_category: Option<&'static str>,
    /// Whether this entity can spawn naturally.
    pub is_spawnable: bool,
    /// Whether this entity can be summoned by commands.
    pub is_summonable: bool,
    /// Sourced Bedrock runtime entity ID, when present in the input data.
    pub runtime_id: Option<u32>,
    /// Collision box height from `minecraft:collision_box`, when present.
    pub height: Option<f32>,
    /// Collision box width from `minecraft:collision_box`, when present.
    pub width: Option<f32>,
}
/// All generated entity metadata, sorted by identifier-derived module name.
pub const ALL_ENTITIES: [EntityDefinitionData; 126usize] = [
    EntityDefinitionData {
        id: 0u32,
        identifier: "minecraft:allay",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.6f32),
        width: Some(0.35f32),
    },
    EntityDefinitionData {
        id: 1u32,
        identifier: "minecraft:area_effect_cloud",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(1.8f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 2u32,
        identifier: "minecraft:armadillo",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.65f32),
        width: Some(0.7f32),
    },
    EntityDefinitionData {
        id: 3u32,
        identifier: "minecraft:armor_stand",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.975f32),
        width: Some(0.5f32),
    },
    EntityDefinitionData {
        id: 4u32,
        identifier: "minecraft:arrow",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.25f32),
        width: Some(0.25f32),
    },
    EntityDefinitionData {
        id: 5u32,
        identifier: "minecraft:axolotl",
        spawn_category: Some("axolotls"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.42f32),
        width: Some(0.75f32),
    },
    EntityDefinitionData {
        id: 6u32,
        identifier: "minecraft:bat",
        spawn_category: Some("ambient"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.9f32),
        width: Some(0.5f32),
    },
    EntityDefinitionData {
        id: 7u32,
        identifier: "minecraft:bee",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.5f32),
        width: Some(0.55f32),
    },
    EntityDefinitionData {
        id: 8u32,
        identifier: "minecraft:blaze",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.8f32),
        width: Some(0.5f32),
    },
    EntityDefinitionData {
        id: 9u32,
        identifier: "minecraft:boat",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.455f32),
        width: Some(1.4f32),
    },
    EntityDefinitionData {
        id: 10u32,
        identifier: "minecraft:bogged",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 11u32,
        identifier: "minecraft:breeze",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.77f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 12u32,
        identifier: "minecraft:breeze_wind_charge_projectile",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(0.3125f32),
        width: Some(0.3125f32),
    },
    EntityDefinitionData {
        id: 13u32,
        identifier: "minecraft:camel",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(2.375f32),
        width: Some(1.7f32),
    },
    EntityDefinitionData {
        id: 14u32,
        identifier: "minecraft:camel_husk",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(2.375f32),
        width: Some(1.7f32),
    },
    EntityDefinitionData {
        id: 15u32,
        identifier: "minecraft:cat",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.7f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 16u32,
        identifier: "minecraft:cave_spider",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.5f32),
        width: Some(0.7f32),
    },
    EntityDefinitionData {
        id: 17u32,
        identifier: "minecraft:chest_boat",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.455f32),
        width: Some(1.4f32),
    },
    EntityDefinitionData {
        id: 18u32,
        identifier: "minecraft:chest_minecart",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.7f32),
        width: Some(0.98f32),
    },
    EntityDefinitionData {
        id: 19u32,
        identifier: "minecraft:chicken",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.8f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 20u32,
        identifier: "minecraft:cod",
        spawn_category: Some("water_ambient"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.3f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 21u32,
        identifier: "minecraft:command_block_minecart",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.7f32),
        width: Some(0.98f32),
    },
    EntityDefinitionData {
        id: 22u32,
        identifier: "minecraft:copper_golem",
        spawn_category: None,
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.98f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 23u32,
        identifier: "minecraft:cow",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.3f32),
        width: Some(0.9f32),
    },
    EntityDefinitionData {
        id: 24u32,
        identifier: "minecraft:creaking",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(2.7f32),
        width: Some(0.9f32),
    },
    EntityDefinitionData {
        id: 25u32,
        identifier: "minecraft:creeper",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.8f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 26u32,
        identifier: "minecraft:dolphin",
        spawn_category: Some("water_creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.6f32),
        width: Some(0.9f32),
    },
    EntityDefinitionData {
        id: 27u32,
        identifier: "minecraft:donkey",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.6f32),
        width: Some(1.4f32),
    },
    EntityDefinitionData {
        id: 28u32,
        identifier: "minecraft:dragon_fireball",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(0.31f32),
        width: Some(0.31f32),
    },
    EntityDefinitionData {
        id: 29u32,
        identifier: "minecraft:drowned",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 30u32,
        identifier: "minecraft:egg",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.25f32),
        width: Some(0.25f32),
    },
    EntityDefinitionData {
        id: 31u32,
        identifier: "minecraft:elder_guardian",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.99f32),
        width: Some(1.99f32),
    },
    EntityDefinitionData {
        id: 32u32,
        identifier: "minecraft:ender_crystal",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(2f32),
        width: Some(2f32),
    },
    EntityDefinitionData {
        id: 33u32,
        identifier: "minecraft:ender_dragon",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(4f32),
        width: Some(13f32),
    },
    EntityDefinitionData {
        id: 34u32,
        identifier: "minecraft:ender_pearl",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(0.25f32),
        width: Some(0.25f32),
    },
    EntityDefinitionData {
        id: 35u32,
        identifier: "minecraft:enderman",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(2.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 36u32,
        identifier: "minecraft:endermite",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.3f32),
        width: Some(0.4f32),
    },
    EntityDefinitionData {
        id: 37u32,
        identifier: "minecraft:evocation_illager",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 38u32,
        identifier: "minecraft:eye_of_ender_signal",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(0.25f32),
        width: Some(0.25f32),
    },
    EntityDefinitionData {
        id: 39u32,
        identifier: "minecraft:fireball",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(1f32),
        width: Some(1f32),
    },
    EntityDefinitionData {
        id: 40u32,
        identifier: "minecraft:fireworks_rocket",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.25f32),
        width: Some(0.25f32),
    },
    EntityDefinitionData {
        id: 41u32,
        identifier: "minecraft:fishing_hook",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(0.15f32),
        width: Some(0.15f32),
    },
    EntityDefinitionData {
        id: 42u32,
        identifier: "minecraft:fox",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.7f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 43u32,
        identifier: "minecraft:frog",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.55f32),
        width: Some(0.5f32),
    },
    EntityDefinitionData {
        id: 44u32,
        identifier: "minecraft:ghast",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(4f32),
        width: Some(4.02f32),
    },
    EntityDefinitionData {
        id: 45u32,
        identifier: "minecraft:glow_squid",
        spawn_category: Some("underground_water_creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.8f32),
        width: Some(0.8f32),
    },
    EntityDefinitionData {
        id: 46u32,
        identifier: "minecraft:goat",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.3f32),
        width: Some(0.9f32),
    },
    EntityDefinitionData {
        id: 47u32,
        identifier: "minecraft:guardian",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.85f32),
        width: Some(0.85f32),
    },
    EntityDefinitionData {
        id: 48u32,
        identifier: "minecraft:happy_ghast",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(4f32),
        width: Some(4f32),
    },
    EntityDefinitionData {
        id: 49u32,
        identifier: "minecraft:hoglin",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.8f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 50u32,
        identifier: "minecraft:hopper_minecart",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.7f32),
        width: Some(0.98f32),
    },
    EntityDefinitionData {
        id: 51u32,
        identifier: "minecraft:horse",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.6f32),
        width: Some(1.4f32),
    },
    EntityDefinitionData {
        id: 52u32,
        identifier: "minecraft:husk",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 53u32,
        identifier: "minecraft:iron_golem",
        spawn_category: None,
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(2.9f32),
        width: Some(1.4f32),
    },
    EntityDefinitionData {
        id: 54u32,
        identifier: "minecraft:lightning_bolt",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.8f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 55u32,
        identifier: "minecraft:lingering_potion",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(0.25f32),
        width: Some(0.25f32),
    },
    EntityDefinitionData {
        id: 56u32,
        identifier: "minecraft:llama",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.87f32),
        width: Some(0.9f32),
    },
    EntityDefinitionData {
        id: 57u32,
        identifier: "minecraft:llama_spit",
        spawn_category: Some("misc"),
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(0.31f32),
        width: Some(0.31f32),
    },
    EntityDefinitionData {
        id: 58u32,
        identifier: "minecraft:magma_cube",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(2.08f32),
        width: Some(2.08f32),
    },
    EntityDefinitionData {
        id: 59u32,
        identifier: "minecraft:minecart",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.7f32),
        width: Some(0.98f32),
    },
    EntityDefinitionData {
        id: 60u32,
        identifier: "minecraft:mooshroom",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.3f32),
        width: Some(0.9f32),
    },
    EntityDefinitionData {
        id: 61u32,
        identifier: "minecraft:mule",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.6f32),
        width: Some(1.4f32),
    },
    EntityDefinitionData {
        id: 62u32,
        identifier: "minecraft:nautilus",
        spawn_category: None,
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.8f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 63u32,
        identifier: "minecraft:npc",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(2.1f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 64u32,
        identifier: "minecraft:ocelot",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.7f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 65u32,
        identifier: "minecraft:ominous_item_spawner",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(1.8f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 66u32,
        identifier: "minecraft:panda",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.25f32),
        width: Some(1.3f32),
    },
    EntityDefinitionData {
        id: 67u32,
        identifier: "minecraft:parched",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 68u32,
        identifier: "minecraft:parrot",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1f32),
        width: Some(0.5f32),
    },
    EntityDefinitionData {
        id: 69u32,
        identifier: "minecraft:phantom",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.5f32),
        width: Some(0.9f32),
    },
    EntityDefinitionData {
        id: 70u32,
        identifier: "minecraft:pig",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.9f32),
        width: Some(0.9f32),
    },
    EntityDefinitionData {
        id: 71u32,
        identifier: "minecraft:piglin",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 72u32,
        identifier: "minecraft:piglin_brute",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 73u32,
        identifier: "minecraft:pillager",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 74u32,
        identifier: "minecraft:player",
        spawn_category: Some("creature"),
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(1.8f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 75u32,
        identifier: "minecraft:polar_bear",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.4f32),
        width: Some(1.4f32),
    },
    EntityDefinitionData {
        id: 76u32,
        identifier: "minecraft:pufferfish",
        spawn_category: Some("water_ambient"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.8f32),
        width: Some(0.8f32),
    },
    EntityDefinitionData {
        id: 77u32,
        identifier: "minecraft:rabbit",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.67f32),
        width: Some(0.67f32),
    },
    EntityDefinitionData {
        id: 78u32,
        identifier: "minecraft:ravager",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(2.2f32),
        width: Some(1.95f32),
    },
    EntityDefinitionData {
        id: 79u32,
        identifier: "minecraft:salmon",
        spawn_category: Some("water_ambient"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.5f32),
        width: Some(0.5f32),
    },
    EntityDefinitionData {
        id: 80u32,
        identifier: "minecraft:sheep",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.3f32),
        width: Some(0.9f32),
    },
    EntityDefinitionData {
        id: 81u32,
        identifier: "minecraft:shulker",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.8f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 82u32,
        identifier: "minecraft:shulker_bullet",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(0.625f32),
        width: Some(0.625f32),
    },
    EntityDefinitionData {
        id: 83u32,
        identifier: "minecraft:silverfish",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.3f32),
        width: Some(0.4f32),
    },
    EntityDefinitionData {
        id: 84u32,
        identifier: "minecraft:skeleton",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 85u32,
        identifier: "minecraft:skeleton_horse",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.8f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 86u32,
        identifier: "minecraft:slime",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(2.08f32),
        width: Some(2.08f32),
    },
    EntityDefinitionData {
        id: 87u32,
        identifier: "minecraft:small_fireball",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(0.31f32),
        width: Some(0.31f32),
    },
    EntityDefinitionData {
        id: 88u32,
        identifier: "minecraft:sniffer",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.75f32),
        width: Some(1.9f32),
    },
    EntityDefinitionData {
        id: 89u32,
        identifier: "minecraft:snow_golem",
        spawn_category: None,
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.8f32),
        width: Some(0.4f32),
    },
    EntityDefinitionData {
        id: 90u32,
        identifier: "minecraft:snowball",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.25f32),
        width: Some(0.25f32),
    },
    EntityDefinitionData {
        id: 91u32,
        identifier: "minecraft:spider",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.9f32),
        width: Some(1.4f32),
    },
    EntityDefinitionData {
        id: 92u32,
        identifier: "minecraft:splash_potion",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.25f32),
        width: Some(0.25f32),
    },
    EntityDefinitionData {
        id: 93u32,
        identifier: "minecraft:squid",
        spawn_category: Some("water_creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.8f32),
        width: Some(0.8f32),
    },
    EntityDefinitionData {
        id: 94u32,
        identifier: "minecraft:stray",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 95u32,
        identifier: "minecraft:strider",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.7f32),
        width: Some(0.9f32),
    },
    EntityDefinitionData {
        id: 96u32,
        identifier: "minecraft:tadpole",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.6f32),
        width: Some(0.8f32),
    },
    EntityDefinitionData {
        id: 97u32,
        identifier: "minecraft:thrown_trident",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(0.35f32),
        width: Some(0.25f32),
    },
    EntityDefinitionData {
        id: 98u32,
        identifier: "minecraft:tnt",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.98f32),
        width: Some(0.98f32),
    },
    EntityDefinitionData {
        id: 99u32,
        identifier: "minecraft:tnt_minecart",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.7f32),
        width: Some(0.98f32),
    },
    EntityDefinitionData {
        id: 100u32,
        identifier: "minecraft:trader_llama",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.87f32),
        width: Some(0.9f32),
    },
    EntityDefinitionData {
        id: 101u32,
        identifier: "minecraft:tripod_camera",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(1.8f32),
        width: Some(0.75f32),
    },
    EntityDefinitionData {
        id: 102u32,
        identifier: "minecraft:tropicalfish",
        spawn_category: Some("water_ambient"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.4f32),
        width: Some(0.4f32),
    },
    EntityDefinitionData {
        id: 103u32,
        identifier: "minecraft:turtle",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.8f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 104u32,
        identifier: "minecraft:vex",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.8f32),
        width: Some(0.4f32),
    },
    EntityDefinitionData {
        id: 105u32,
        identifier: "minecraft:villager",
        spawn_category: None,
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 106u32,
        identifier: "minecraft:villager_v2",
        spawn_category: None,
        is_spawnable: true,
        is_summonable: false,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 107u32,
        identifier: "minecraft:vindicator",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 108u32,
        identifier: "minecraft:wandering_trader",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 109u32,
        identifier: "minecraft:warden",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(2.9f32),
        width: Some(0.9f32),
    },
    EntityDefinitionData {
        id: 110u32,
        identifier: "minecraft:wind_charge_projectile",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.3125f32),
        width: Some(0.3125f32),
    },
    EntityDefinitionData {
        id: 111u32,
        identifier: "minecraft:witch",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 112u32,
        identifier: "minecraft:wither",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(3f32),
        width: Some(1f32),
    },
    EntityDefinitionData {
        id: 113u32,
        identifier: "minecraft:wither_skeleton",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(2.01f32),
        width: Some(0.72f32),
    },
    EntityDefinitionData {
        id: 114u32,
        identifier: "minecraft:wither_skull",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(0.15f32),
        width: Some(0.15f32),
    },
    EntityDefinitionData {
        id: 115u32,
        identifier: "minecraft:wither_skull_dangerous",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: false,
        runtime_id: None,
        height: Some(0.15f32),
        width: Some(0.15f32),
    },
    EntityDefinitionData {
        id: 116u32,
        identifier: "minecraft:wolf",
        spawn_category: Some("creature"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.8f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 117u32,
        identifier: "minecraft:xp_bottle",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.25f32),
        width: Some(0.25f32),
    },
    EntityDefinitionData {
        id: 118u32,
        identifier: "minecraft:xp_orb",
        spawn_category: None,
        is_spawnable: false,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.25f32),
        width: Some(0.25f32),
    },
    EntityDefinitionData {
        id: 119u32,
        identifier: "minecraft:zoglin",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.8f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 120u32,
        identifier: "minecraft:zombie",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 121u32,
        identifier: "minecraft:zombie_horse",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.6f32),
        width: Some(1.4f32),
    },
    EntityDefinitionData {
        id: 122u32,
        identifier: "minecraft:zombie_nautilus",
        spawn_category: None,
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(0.95f32),
        width: Some(0.875f32),
    },
    EntityDefinitionData {
        id: 123u32,
        identifier: "minecraft:zombie_pigman",
        spawn_category: None,
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 124u32,
        identifier: "minecraft:zombie_villager",
        spawn_category: Some("monster"),
        is_spawnable: true,
        is_summonable: true,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
    EntityDefinitionData {
        id: 125u32,
        identifier: "minecraft:zombie_villager_v2",
        spawn_category: None,
        is_spawnable: true,
        is_summonable: false,
        runtime_id: None,
        height: Some(1.9f32),
        width: Some(0.6f32),
    },
];
/// Look up generated entity metadata by namespaced identifier.
pub fn get(identifier: &str) -> Option<&'static EntityDefinitionData> {
    ALL_ENTITIES
        .iter()
        .find(|entity| entity.identifier == identifier)
}
pub use allay::spawn_allay;
pub use area_effect_cloud::spawn_area_effect_cloud;
pub use armadillo::spawn_armadillo;
pub use armor_stand::spawn_armor_stand;
pub use arrow::spawn_arrow;
pub use axolotl::spawn_axolotl;
pub use bat::spawn_bat;
pub use bee::spawn_bee;
pub use blaze::spawn_blaze;
pub use boat::spawn_boat;
pub use bogged::spawn_bogged;
pub use breeze::spawn_breeze;
pub use breeze_wind_charge_projectile::spawn_breeze_wind_charge_projectile;
pub use camel::spawn_camel;
pub use camel_husk::spawn_camel_husk;
pub use cat::spawn_cat;
pub use cave_spider::spawn_cave_spider;
pub use chest_boat::spawn_chest_boat;
pub use chest_minecart::spawn_chest_minecart;
pub use chicken::spawn_chicken;
pub use cod::spawn_cod;
pub use command_block_minecart::spawn_command_block_minecart;
pub use copper_golem::spawn_copper_golem;
pub use cow::spawn_cow;
pub use creaking::spawn_creaking;
pub use creeper::spawn_creeper;
pub use dolphin::spawn_dolphin;
pub use donkey::spawn_donkey;
pub use dragon_fireball::spawn_dragon_fireball;
pub use drowned::spawn_drowned;
pub use egg::spawn_egg;
pub use elder_guardian::spawn_elder_guardian;
pub use ender_crystal::spawn_ender_crystal;
pub use ender_dragon::spawn_ender_dragon;
pub use ender_pearl::spawn_ender_pearl;
pub use enderman::spawn_enderman;
pub use endermite::spawn_endermite;
pub use evocation_illager::spawn_evocation_illager;
pub use eye_of_ender_signal::spawn_eye_of_ender_signal;
pub use fireball::spawn_fireball;
pub use fireworks_rocket::spawn_fireworks_rocket;
pub use fishing_hook::spawn_fishing_hook;
pub use fox::spawn_fox;
pub use frog::spawn_frog;
pub use ghast::spawn_ghast;
pub use glow_squid::spawn_glow_squid;
pub use goat::spawn_goat;
pub use guardian::spawn_guardian;
pub use happy_ghast::spawn_happy_ghast;
pub use hoglin::spawn_hoglin;
pub use hopper_minecart::spawn_hopper_minecart;
pub use horse::spawn_horse;
pub use husk::spawn_husk;
pub use iron_golem::spawn_iron_golem;
pub use lightning_bolt::spawn_lightning_bolt;
pub use lingering_potion::spawn_lingering_potion;
pub use llama::spawn_llama;
pub use llama_spit::spawn_llama_spit;
pub use magma_cube::spawn_magma_cube;
pub use minecart::spawn_minecart;
pub use mooshroom::spawn_mooshroom;
pub use mule::spawn_mule;
pub use nautilus::spawn_nautilus;
pub use npc::spawn_npc;
pub use ocelot::spawn_ocelot;
pub use ominous_item_spawner::spawn_ominous_item_spawner;
pub use panda::spawn_panda;
pub use parched::spawn_parched;
pub use parrot::spawn_parrot;
pub use phantom::spawn_phantom;
pub use pig::spawn_pig;
pub use piglin::spawn_piglin;
pub use piglin_brute::spawn_piglin_brute;
pub use pillager::spawn_pillager;
pub use player::spawn_player;
pub use polar_bear::spawn_polar_bear;
pub use pufferfish::spawn_pufferfish;
pub use rabbit::spawn_rabbit;
pub use ravager::spawn_ravager;
pub use salmon::spawn_salmon;
pub use sheep::spawn_sheep;
pub use shulker::spawn_shulker;
pub use shulker_bullet::spawn_shulker_bullet;
pub use silverfish::spawn_silverfish;
pub use skeleton::spawn_skeleton;
pub use skeleton_horse::spawn_skeleton_horse;
pub use slime::spawn_slime;
pub use small_fireball::spawn_small_fireball;
pub use sniffer::spawn_sniffer;
pub use snow_golem::spawn_snow_golem;
pub use snowball::spawn_snowball;
pub use spider::spawn_spider;
pub use splash_potion::spawn_splash_potion;
pub use squid::spawn_squid;
pub use stray::spawn_stray;
pub use strider::spawn_strider;
pub use tadpole::spawn_tadpole;
pub use thrown_trident::spawn_thrown_trident;
pub use tnt::spawn_tnt;
pub use tnt_minecart::spawn_tnt_minecart;
pub use trader_llama::spawn_trader_llama;
pub use tripod_camera::spawn_tripod_camera;
pub use tropicalfish::spawn_tropicalfish;
pub use turtle::spawn_turtle;
pub use vex::spawn_vex;
pub use villager::spawn_villager;
pub use villager_v2::spawn_villager_v2;
pub use vindicator::spawn_vindicator;
pub use wandering_trader::spawn_wandering_trader;
pub use warden::spawn_warden;
pub use wind_charge_projectile::spawn_wind_charge_projectile;
pub use witch::spawn_witch;
pub use wither::spawn_wither;
pub use wither_skeleton::spawn_wither_skeleton;
pub use wither_skull::spawn_wither_skull;
pub use wither_skull_dangerous::spawn_wither_skull_dangerous;
pub use wolf::spawn_wolf;
pub use xp_bottle::spawn_xp_bottle;
pub use xp_orb::spawn_xp_orb;
pub use zoglin::spawn_zoglin;
pub use zombie::spawn_zombie;
pub use zombie_horse::spawn_zombie_horse;
pub use zombie_nautilus::spawn_zombie_nautilus;
pub use zombie_pigman::spawn_zombie_pigman;
pub use zombie_villager::spawn_zombie_villager;
pub use zombie_villager_v2::spawn_zombie_villager_v2;
