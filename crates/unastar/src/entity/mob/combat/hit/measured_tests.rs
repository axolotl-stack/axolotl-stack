//! Player melee hits against the BDS 1.26.51 attack sweep.
//!
//! `unastar-data/data/measured/attack_damage.json` records, per held stack
//! (empty hand, every tool tier, trident, mace, spears, Sharpness I-V on a
//! diamond sword), the damage BDS dealt to a fresh pig with one attack, and
//! for the survival sweep the durability damage each hit added to the held
//! stack. Every key is replayed here through [`player_attack`] on a fresh
//! pig (health raised so no hit kills it) by a survival player.

use std::collections::BTreeMap;

use bevy_ecs::prelude::*;
use glam::DVec3;
use serde::Deserialize;

use super::*;
use crate::entity::components::{HeldSlot, MainInventory, Rotation, RuntimeEntityId};
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::item::enchant::{durability_damage, with_enchantments};
use crate::server::game::{GameServer, ServerConfig};
use crate::world::ChunkManager;
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkPosition, ChunkViewers};

const MEASURED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../unastar-data/data/measured/attack_damage.json"
));

#[derive(Deserialize)]
struct Row {
    tool_item: Option<ToolItem>,
    damage: Vec<Option<f32>>,
    #[serde(default)]
    wear: Option<Vec<Option<i32>>>,
}

#[derive(Deserialize)]
struct ToolItem {
    item: String,
    #[serde(default)]
    enchants: BTreeMap<String, u8>,
}

fn held(row: &Row) -> Option<ItemStack> {
    let tool = row.tool_item.as_ref()?;
    let stack = crate::item::test_support::stack(&tool.item, 1);
    let enchants: Vec<_> = tool
        .enchants
        .iter()
        .map(|(name, level)| {
            let enchantment = name
                .parse::<Enchantment>()
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            (enchantment, *level)
        })
        .collect();
    Some(if enchants.is_empty() {
        stack
    } else {
        with_enchantments(&stack, enchants)
    })
}

/// A stone floor over chunk (0, 0) and one survival player.
fn world() -> (GameServer, Entity) {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(7));
    let mut column = Chunk::new(0, 0);
    for x in 0..16 {
        for z in 0..16 {
            column.set_block(x, 63, z, *blocks::STONE);
        }
    }
    let chunk = world.spawn(ChunkData::new(column)).id();
    world
        .resource_mut::<ChunkManager>()
        .insert(ChunkPosition::new(0, 0), chunk);
    let (tx, _rx) = tokio::sync::mpsc::channel(1 << 16);
    let player = world
        .spawn((
            Player,
            Position(DVec3::new(
                4.5,
                64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                8.5,
            )),
            Rotation::default(),
            GameMode::Survival,
            MainInventory::default(),
            HeldSlot(0),
            Health::default(),
            RuntimeEntityId(1),
            PlayerSession::new(1, "attacker".into(), None, None, tx),
        ))
        .id();
    let mut viewers = ChunkViewers::default();
    viewers.insert(player);
    world.entity_mut(chunk).insert(viewers);
    (server, player)
}

/// One hit with `stack` on a fresh pig: the health it lost and the
/// durability damage the hit added to the stack.
fn hit(world: &mut World, player: Entity, stack: Option<&ItemStack>) -> (f32, i32) {
    let stack = stack.cloned().unwrap_or_else(ItemStack::empty);
    let before = durability_damage(&stack);
    world
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, stack)
        .unwrap();
    let feet = DVec3::new(8.5, 64.0, 8.5);
    let pig = spawn_mob(world, "minecraft:pig", feet, 0.0, SpawnCause::Spawned).unwrap();
    *world.get_mut::<Health>(pig).unwrap() = Health {
        current: 100.0,
        max: 100.0,
    };
    world.get_mut::<Position>(player).unwrap().0 = DVec3::new(
        feet.x - 1.5,
        feet.y + crate::entity::PLAYER_EYE_HEIGHT,
        feet.z,
    );
    // The sweep spaced its spear hits past the attack cooldown
    // (`--gap 20`); no tick passes here, so forget the last one.
    world
        .entity_mut(player)
        .remove::<crate::item::cooldown::ItemCooldowns>();
    player_attack(world, player, pig, &Default::default());
    let lost = 100.0 - world.get::<Health>(pig).unwrap().current;
    world.despawn(pig);
    let after = world
        .get::<MainInventory>(player)
        .unwrap()
        .0
        .item(0)
        .unwrap();
    (lost, durability_damage(after) - before)
}

#[test]
fn measured_melee_hits_match_bds() {
    let rows: BTreeMap<String, Row> =
        serde_json::from_str(MEASURED).expect("measured attack table decodes");
    let (mut server, player) = world();
    let world = server.ecs.world_mut();
    let mut wrong = Vec::new();
    let mut wear_compared = 0;
    for (key, row) in &rows {
        let stack = held(row);
        let (lost, wear) = hit(world, player, stack.as_ref());
        for measured in &row.damage {
            if *measured != Some(lost) {
                wrong.push(format!("{key}: deals {lost}, BDS {measured:?}"));
            }
        }
        // The first trial of a sweep row is the warm-up attack BDS ignores
        // after a held-item change; the measured hits follow it.
        for measured in row.wear.iter().flat_map(|wear| wear.iter().skip(1)) {
            wear_compared += 1;
            if *measured != Some(wear) {
                wrong.push(format!("{key}: a hit wears {wear}, BDS {measured:?}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert!(rows.len() > 40, "only {} keys", rows.len());
    assert!(
        wear_compared > 100,
        "only {wear_compared} hits with measured wear"
    );
}
