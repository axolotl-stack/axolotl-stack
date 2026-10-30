//! Pig and sheep on the data-driven runtime: native AddActor equality,
//! tempting, shearing, eating grass, dyeing and loot, driven through the
//! real interaction, attack and tick paths.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::actor_flags;
use jolyne::valentine::types::{
    ActorRuntimeId, DataItemEntryPayload, EnumsActorEvent,
    EnumsItemUseOnActorInventoryTransactionActionType as ActionType,
    ItemUseOnActorInventoryTransaction,
};
use jolyne::valentine::{AddActorPacket, McpePacket, McpePacketData, actor_data_i_ds as ids};
use tokio::sync::mpsc;
use valentine::bedrock::codec::BedrockCodec;

use crate::entity::EntityRandom;
use crate::entity::components::{
    DroppedItem, Experience, GameMode, Health, HeldSlot, ItemEntityStack, MainInventory, Player,
    PlayerSession, Position, Rotation, RuntimeEntityId, RuntimeId,
};
use crate::entity::definition::attributes::ActorAttributes;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::properties::ActorProperties;
use crate::entity::mob::components::color::{ActorColor, set_color};
use crate::entity::mob::net::network::add_actor_packet;
use crate::entity::mob::*;
use crate::item::ItemStack;
use crate::server::game::{GameServer, ServerConfig, world_stack};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};

struct Harness {
    server: GameServer,
    player: Entity,
    rx: mpsc::Receiver<McpePacket>,
}

/// A grass floor at y = 63 over chunks -1..=1 × -1..=1, viewed by one
/// survival player standing on it at (3.5, 64, 3.5).
fn harness(seed: u64) -> Harness {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(seed));
    let (tx, rx) = mpsc::channel(1 << 16);
    let player = world
        .spawn((
            Player,
            Position(DVec3::new(
                3.5,
                64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                3.5,
            )),
            Rotation::default(),
            GameMode::Survival,
            MainInventory::default(),
            HeldSlot(0),
            Health::default(),
            Experience::default(),
            RuntimeEntityId(1),
            PlayerSession::new(1, "farmer".into(), None, None, tx),
        ))
        .id();
    for cx in -1..=1 {
        for cz in -1..=1 {
            let mut column = Chunk::new(cx, cz);
            for x in 0..16 {
                for z in 0..16 {
                    column.set_block(x, 63, z, *blocks::GRASS_BLOCK);
                }
            }
            let mut viewers = ChunkViewers::default();
            viewers.insert(player);
            let chunk = world.spawn((ChunkData::new(column), viewers)).id();
            world
                .resource_mut::<ChunkManager>()
                .insert(ChunkPosition::new(cx, cz), chunk);
        }
    }
    Harness { server, player, rx }
}

fn flag(world: &World, mob: Entity, index: u32) -> bool {
    world.get::<ActorFlags>(mob).unwrap().get(index).unwrap()
}

impl Harness {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    /// An adult of `identifier` with its feet at `feet` (the spawn event
    /// picks adults 95 % of the time).
    fn adult(&mut self, identifier: &str, feet: DVec3) -> Entity {
        let world = self.world();
        for _ in 0..200 {
            let mob = spawn_mob(world, identifier, feet, 0.0, SpawnCause::Spawned).unwrap();
            if !flag(world, mob, actor_flags::BABY) {
                return mob;
            }
            world.despawn(mob);
        }
        panic!("no adult {identifier} in 200 spawns");
    }

    fn hold(&mut self, item: &str, count: u8) {
        let stack = world_stack(self.world(), item, count).expect("vanilla item");
        let player = self.player;
        self.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, stack)
            .unwrap();
    }

    fn held(&mut self) -> ItemStack {
        let player = self.player;
        self.world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .clone()
    }

    fn interact(&mut self, target: Entity) {
        let runtime_id = self.world().get::<RuntimeId>(target).unwrap().0;
        let transaction = ItemUseOnActorInventoryTransaction {
            runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id as u64,
            },
            action_type: ActionType::Interact,
            slot: 0,
            ..Default::default()
        };
        let player = self.player;
        interaction::interact::player_interact(self.world(), player, target, &transaction);
        crate::entity::definition::apply_pending_definitions(self.world());
    }

    /// Leave `mob` one health point and punch it to death from two blocks west.
    fn kill(&mut self, mob: Entity) {
        let player = self.player;
        let world = self.world();
        world.get_mut::<Health>(mob).unwrap().current = 1.0;
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, ItemStack::empty())
            .unwrap();
        let feet = world.get::<Position>(mob).unwrap().0;
        world.get_mut::<Position>(player).unwrap().0 = DVec3::new(
            feet.x - 2.0,
            feet.y + crate::entity::PLAYER_EYE_HEIGHT,
            feet.z,
        );
        combat::player_attack(world, player, mob, &Default::default());
        assert!(world.get::<state::Dead>(mob).is_some());
    }

    /// Every dropped item stack, removing them from the world.
    fn take_drops(&mut self) -> Vec<(String, u8)> {
        let world = self.world();
        let drops: Vec<(Entity, String, u8)> = world
            .query_filtered::<(Entity, &ItemEntityStack), With<DroppedItem>>()
            .iter(world)
            .map(|(entity, stack)| (entity, stack.0.item_id().to_owned(), stack.0.count))
            .collect();
        for (entity, _, _) in &drops {
            world.despawn(*entity);
        }
        drops.into_iter().map(|(_, id, n)| (id, n)).collect()
    }

    fn packets(&mut self) -> Vec<McpePacketData> {
        std::iter::from_fn(|| self.rx.try_recv().ok().map(|p| p.data)).collect()
    }

    fn block(&mut self, x: i32, y: i32, z: i32) -> u32 {
        use crate::entity::systems::BlockSource;
        self.world().block_at(glam::IVec3::new(x, y, z)).unwrap()
    }
}

fn count(drops: &[(String, u8)], id: &str) -> u32 {
    drops
        .iter()
        .filter(|(item, _)| item == id)
        .map(|(_, n)| u32::from(*n))
        .sum()
}

fn color_index(world: &World, mob: Entity) -> Option<i8> {
    match world
        .get::<EntityMetadata>(mob)
        .unwrap()
        .get(ids::COLOR_INDEX)
    {
        Some(DataItemEntryPayload::DataItemBytePayload(byte)) => Some(byte.value),
        _ => None,
    }
}

fn native(bytes: &[u8]) -> AddActorPacket {
    assert_eq!(bytes[0], 0x0d);
    let mut input = &bytes[1..];
    let packet = AddActorPacket::decode(&mut input, ()).unwrap();
    assert!(input.is_empty());
    packet
}

fn ours(world: &World, identifier: &str, mob: Entity, native: &AddActorPacket) -> AddActorPacket {
    add_actor_packet(
        identifier,
        world.get::<RuntimeId>(mob).unwrap().0,
        world.get::<Position>(mob).unwrap().0,
        DVec3::new(0.0, f64::from(native.velocity.y), 0.0),
        world.get::<Rotation>(mob).unwrap(),
        world.get::<EntityMetadata>(mob).unwrap(),
        world.get::<ActorProperties>(mob).unwrap(),
        world.get::<ActorAttributes>(mob).unwrap(),
    )
}

#[test]
fn vanilla_pig_and_sheep_compile_to_spawnable_archetypes() {
    let mut h = harness(1);
    let registry = h.world().resource::<MobRegistry>();
    for identifier in ["minecraft:pig", "minecraft:sheep"] {
        assert!(
            registry.get(identifier).is_some(),
            "{identifier} unsupported: {:?}",
            registry.unsupported_reason(identifier)
        );
    }
}

#[test]
fn spawned_adult_pig_add_actor_matches_native_summon() {
    let mut h = harness(2);
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/pig.bin"
    ));
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let pig = h.adult("minecraft:pig", feet);
    let world = h.world();
    let ours = ours(world, "minecraft:pig", pig, &native);
    assert_eq!(ours.actor_type, native.actor_type);
    assert_eq!(ours.attributes_list, native.attributes_list);
    assert_eq!(ours.actor_data, native.actor_data);
    assert_eq!(ours.actor_links, native.actor_links);
    // climate_variant (slot 0) is the default outside cold/warm biomes;
    // sound_variant (slot 1) is a three-way draw in the spawn event.
    let props = &ours.synched_properties;
    assert!(props.float_entries_list.is_empty());
    assert_eq!(props.int_entries_list.len(), 2);
    assert_eq!(
        props.int_entries_list[0],
        native.synched_properties.int_entries_list[0]
    );
    assert_eq!(props.int_entries_list[1].property_index, 1);
    assert!((0..=2).contains(&props.int_entries_list[1].data));
}

#[test]
fn spawned_adult_sheep_add_actor_matches_native_summon() {
    let mut h = harness(3);
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/sheep.bin"
    ));
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let sheep = h.adult("minecraft:sheep", feet);
    let world = h.world();
    // The wool colour is a weighted draw in the spawn event (white 82 % in
    // temperate biomes): adopt the native draw, then compare everything.
    let native_colour = native
        .actor_data
        .data
        .iter()
        .find(|entry| entry.id == ids::COLOR_INDEX)
        .and_then(|entry| match &entry.payload {
            DataItemEntryPayload::DataItemBytePayload(byte) => Some(byte.value),
            _ => None,
        })
        .expect("native sheep colour");
    assert!((0..16).contains(&native_colour));
    set_color(
        &mut world.entity_mut(sheep),
        ActorColor(native_colour as u8),
    );
    let ours = ours(world, "minecraft:sheep", sheep, &native);
    assert_eq!(ours.actor_type, native.actor_type);
    assert_eq!(ours.attributes_list, native.attributes_list);
    assert_eq!(ours.actor_data, native.actor_data);
    assert_eq!(ours.actor_links, native.actor_links);
    assert_eq!(ours.synched_properties, native.synched_properties);
}

#[test]
fn spawned_sheep_wool_follows_the_temperate_colour_weights() {
    let mut h = harness(4);
    let world = h.world();
    let mut white = 0;
    for _ in 0..200 {
        let sheep = spawn_mob(
            world,
            "minecraft:sheep",
            DVec3::new(8.5, 64.0, 8.5),
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap();
        let colour = world.get::<ActorColor>(sheep).expect("coloured").0;
        assert!([0, 6, 7, 8, 12, 15].contains(&colour), "colour {colour}");
        assert_eq!(color_index(world, sheep), Some(colour as i8));
        white += usize::from(colour == 0);
        world.despawn(sheep);
    }
    assert!((130..=190).contains(&white), "{white} of 200 white");
}

#[test]
fn a_pig_follows_a_player_holding_a_carrot() {
    let mut h = harness(5);
    let pig = h.adult("minecraft:pig", DVec3::new(11.5, 64.0, 3.5));
    h.hold("minecraft:carrot", 1);
    let start = h.world().get::<Position>(pig).unwrap().0.x;
    for _ in 0..100 {
        h.server.tick();
    }
    let world = h.world();
    let end = world.get::<Position>(pig).unwrap().0;
    assert!(flag(world, pig, actor_flags::TEMPTED), "the pig is tempted");
    assert!(start - end.x > 4.0, "walked from x {start} to {end}");
}

#[test]
fn shearing_drops_wool_of_the_sheep_colour_and_wears_the_shears() {
    let mut h = harness(6);
    let sheep = h.adult("minecraft:sheep", DVec3::new(5.5, 64.0, 3.5));
    set_color(&mut h.world().entity_mut(sheep), ActorColor(14));
    h.server.tick();
    h.take_drops();
    h.hold("minecraft:shears", 1);
    h.interact(sheep);

    let drops = h.take_drops();
    let wool = count(&drops, "minecraft:red_wool");
    assert!((1..=3).contains(&wool), "{drops:?}");
    assert_eq!(drops.len(), 1, "only red wool: {drops:?}");
    let shears = h.held();
    assert_eq!(shears.item_id(), "minecraft:shears");
    assert_eq!(crate::item::enchant::durability_damage(&shears), 1);
    let world = h.world();
    assert!(flag(world, sheep, actor_flags::SHEARED));
    assert_eq!(
        world
            .get::<components::LootConfig>(sheep)
            .unwrap()
            .table
            .as_ref(),
        "entities/sheep_sheared"
    );

    // Past the 2.5 s cooldown a sheared sheep still cannot be sheared.
    for _ in 0..60 {
        h.server.tick();
    }
    h.take_drops();
    h.interact(sheep);
    assert!(h.take_drops().is_empty());
    assert_eq!(crate::item::enchant::durability_damage(&h.held()), 1);
}

#[test]
fn a_sheared_sheep_eats_grass_and_regrows_wool() {
    let mut h = harness(7);
    let sheep = h.adult("minecraft:sheep", DVec3::new(5.5, 64.0, 3.5));
    h.hold("minecraft:shears", 1);
    h.interact(sheep);
    assert!(flag(h.world(), sheep, actor_flags::SHEARED));
    let runtime_id = h.world().get::<RuntimeId>(sheep).unwrap().0 as u64;
    h.packets();

    let mut ate = false;
    // The eaten grass's particles and the dirt that replaces it name blocks
    // by their wire ids.
    let mut block_ids = 0;
    for _ in 0..20_000 {
        h.server.tick();
        let packets = h.packets();
        let ids = crate::server::game::types::block_network_ids(h.world());
        block_ids += crate::server::wire_audit::assert_block_ids_on_wire(ids, &packets);
        ate |= packets.iter().any(|packet| {
            matches!(packet, McpePacketData::ActorEventPacket(event)
                if event.target_runtime_id.actor_runtime_id == runtime_id
                    && event.event_id == EnumsActorEvent::EatGrass)
        });
        if !flag(h.world(), sheep, actor_flags::SHEARED) {
            break;
        }
    }
    assert!(ate, "viewers saw the sheep lower its head");
    assert!(block_ids >= 2, "the eating sent block ids: {block_ids}");
    let world = h.world();
    assert!(!flag(world, sheep, actor_flags::SHEARED), "wool regrew");
    assert!(
        world
            .get::<components::color::DyeableConfig>(sheep)
            .is_some()
    );
    assert_eq!(
        world
            .get::<components::LootConfig>(sheep)
            .unwrap()
            .table
            .as_ref(),
        "entities/sheep"
    );
    let dirt = (-16..32)
        .flat_map(|x| (-16..32).map(move |z| (x, z)))
        .filter(|&(x, z)| h.block(x, 63, z) == *blocks::DIRT)
        .count();
    assert_eq!(dirt, 1, "exactly the eaten grass block became dirt");
}

#[test]
fn dyeing_a_white_sheep_red_consumes_the_dye() {
    let mut h = harness(8);
    let sheep = h.adult("minecraft:sheep", DVec3::new(5.5, 64.0, 3.5));
    set_color(&mut h.world().entity_mut(sheep), ActorColor(0));
    h.hold("minecraft:red_dye", 2);
    h.interact(sheep);
    let world = h.world();
    assert_eq!(world.get::<ActorColor>(sheep), Some(&ActorColor(14)));
    assert_eq!(color_index(world, sheep), Some(14));
    assert_eq!(h.held().count, 1);
    // A dye of the sheep's own colour does nothing.
    h.interact(sheep);
    assert_eq!(h.held().count, 1);
}

#[test]
fn killed_pigs_and_sheep_drop_their_loot_by_colour_and_fire() {
    let mut h = harness(9);
    let feet = DVec3::new(6.5, 64.0, 3.5);

    let pig = h.adult("minecraft:pig", feet);
    h.kill(pig);
    let drops = h.take_drops();
    assert!(
        (1..=3).contains(&count(&drops, "minecraft:porkchop")),
        "{drops:?}"
    );

    let pig = h.adult("minecraft:pig", feet);
    crate::entity::mob::combat::fire::ignite(h.world(), pig, 100);
    h.kill(pig);
    let drops = h.take_drops();
    assert!(count(&drops, "minecraft:cooked_porkchop") >= 1, "{drops:?}");
    assert_eq!(count(&drops, "minecraft:porkchop"), 0);

    let sheep = h.adult("minecraft:sheep", feet);
    set_color(&mut h.world().entity_mut(sheep), ActorColor(11));
    h.kill(sheep);
    let drops = h.take_drops();
    assert_eq!(count(&drops, "minecraft:blue_wool"), 1, "{drops:?}");
    assert!(
        (1..=2).contains(&count(&drops, "minecraft:mutton")),
        "{drops:?}"
    );

    let sheep = h.adult("minecraft:sheep", feet);
    crate::entity::mob::combat::fire::ignite(h.world(), sheep, 100);
    h.kill(sheep);
    let drops = h.take_drops();
    assert!(count(&drops, "minecraft:cooked_mutton") >= 1, "{drops:?}");
    assert_eq!(count(&drops, "minecraft:mutton"), 0);

    let sheep = h.adult("minecraft:sheep", feet);
    h.hold("minecraft:shears", 1);
    h.interact(sheep);
    h.take_drops();
    h.kill(sheep);
    let drops = h.take_drops();
    assert!(count(&drops, "minecraft:mutton") >= 1, "{drops:?}");
    assert!(
        !drops.iter().any(|(item, _)| item.ends_with("_wool")),
        "a sheared sheep drops no wool: {drops:?}"
    );
}

#[test]
fn a_dyed_sheep_keeps_its_colour_through_a_save() {
    let mut h = harness(10);
    let sheep = h.adult("minecraft:sheep", DVec3::new(5.5, 64.0, 3.5));
    set_color(&mut h.world().entity_mut(sheep), ActorColor(0));
    h.hold("minecraft:lime_dye", 1);
    h.interact(sheep);
    let nbt = persistence::encode_for_tests(h.world(), sheep);
    h.world().despawn(sheep);
    let restored = persistence::restore_mob(h.world(), &nbt).unwrap();
    let world = h.world();
    assert_eq!(world.get::<ActorColor>(restored), Some(&ActorColor(5)));
    assert_eq!(color_index(world, restored), Some(5));
}
