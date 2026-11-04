//! Leads and name tags on mobs through the real item-use-on-actor path and
//! the scheduled tick.

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::types::{
    ActorRuntimeId, DataItemEntryPayload,
    EnumsItemUseOnActorInventoryTransactionActionType as ActionType,
    ItemUseOnActorInventoryTransaction,
};
use jolyne::valentine::{actor_data_i_ds as ids, actor_flags};

use crate::entity::EntityRandom;
use crate::entity::components::{
    GameMode, HeldSlot, ItemEntityStack, MainInventory, Player, PlayerSession, Position, Rotation,
    RuntimeEntityId, RuntimeId,
};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::state::{CustomName, Leashed, Persistent};
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::item::ItemStack;
use crate::item::enchant::with_custom_name;
use crate::server::game::{GameServer, ServerConfig, world_stack};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};

struct Field {
    server: GameServer,
    player: Entity,
    _rx: tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
}

/// A grass floor at y = 63 over chunk (0, 0) with one player on it.
fn field(mode: GameMode) -> Field {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(17));
    let mut column = Chunk::new(0, 0);
    for x in 0..16 {
        for z in 0..16 {
            column.set_block(x, 63, z, *blocks::GRASS_BLOCK);
        }
    }
    let chunk = world.spawn(ChunkData::new(column)).id();
    world
        .resource_mut::<ChunkManager>()
        .insert(ChunkPosition::new(0, 0), chunk);
    let (tx, rx) = tokio::sync::mpsc::channel(4096);
    let player = world
        .spawn((
            Player,
            Position(DVec3::new(
                3.5,
                64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                3.5,
            )),
            Rotation::default(),
            mode,
            MainInventory::default(),
            HeldSlot(0),
            RuntimeEntityId(1),
            PlayerSession::new(1, "shepherd".into(), None, None, tx),
        ))
        .id();
    let mut viewers = ChunkViewers::default();
    viewers.insert(player);
    world.entity_mut(chunk).insert(viewers);
    Field {
        server,
        player,
        _rx: rx,
    }
}

impl Field {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    fn hold(&mut self, stack: ItemStack) {
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

    fn cow(&mut self) -> Entity {
        spawn_mob(
            self.world(),
            "minecraft:cow",
            DVec3::new(5.5, 64.0, 3.5),
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap()
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
        crate::entity::mob::interaction::interact::player_interact(
            self.world(),
            player,
            target,
            &transaction,
        );
    }

    fn move_player(&mut self, x: f64, z: f64) {
        let player = self.player;
        self.world().get_mut::<Position>(player).unwrap().0 =
            DVec3::new(x, 64.0 + crate::entity::PLAYER_EYE_HEIGHT, z);
    }

    fn leads_on_ground(&mut self) -> usize {
        let world = self.world();
        world
            .query::<&ItemEntityStack>()
            .iter(world)
            .filter(|stack| stack.0.item_id() == "minecraft:lead")
            .count()
    }
}

fn leash_holder(world: &World, mob: Entity) -> Option<i64> {
    match world.get::<EntityMetadata>(mob)?.get(ids::LEASH_HOLDER)? {
        DataItemEntryPayload::DataItemInt64Payload(row) => Some(row.value),
        _ => None,
    }
}

fn leashed_flag(world: &World, mob: Entity) -> bool {
    world
        .get::<ActorFlags>(mob)
        .unwrap()
        .get(actor_flags::LEASHED)
        .unwrap()
}

#[test]
fn a_lead_leashes_a_cow_to_the_player_and_is_used_up() {
    let mut field = field(GameMode::Survival);
    let lead = world_stack(field.world(), "minecraft:lead", 2).unwrap();
    field.hold(lead);
    let cow = field.cow();
    field.interact(cow);
    let player = field.player;
    let world = field.world();
    assert_eq!(world.get::<Leashed>(cow), Some(&Leashed { holder: player }));
    assert_eq!(leash_holder(world, cow), Some(1));
    assert!(leashed_flag(world, cow));
    assert!(world.get::<Persistent>(cow).is_some());
    assert_eq!(field.held().count, 1);

    // Interacting again (with anything) takes the lead off and drops it.
    field.interact(cow);
    let world = field.world();
    assert!(world.get::<Leashed>(cow).is_none());
    // Measured on BDS 1.26.51: an unleashed mob's holder is -1.
    assert_eq!(leash_holder(world, cow), Some(-1));
    assert!(!leashed_flag(world, cow));
    assert_eq!(field.leads_on_ground(), 1);
    assert_eq!(field.held().count, 1);
}

#[test]
fn a_stretched_lead_pulls_and_then_breaks() {
    let mut field = field(GameMode::Creative);
    let lead = world_stack(field.world(), "minecraft:lead", 1).unwrap();
    field.hold(lead);
    let cow = field.cow();
    field.interact(cow);
    // Creative keeps the lead.
    assert_eq!(field.held().count, 1);
    let start = field.world().get::<Position>(cow).unwrap().0.x;
    // 8 blocks east of the cow: past the hard distance (6), within 10. The
    // pull adds 0.4 blocks per tick eastwards before the physics step.
    field.move_player(start + 8.0, 3.5);
    field.server.tick();
    let moved = field.world().get::<Position>(cow).unwrap().0.x - start;
    assert!(moved > 0.2, "pulled east by {moved}");
    assert!(field.world().get::<Leashed>(cow).is_some());

    let now = field.world().get::<Position>(cow).unwrap().0.x;
    field.move_player(now + 11.0, 3.5);
    field.server.tick();
    assert!(field.world().get::<Leashed>(cow).is_none(), "broke");
    assert_eq!(field.leads_on_ground(), 1);
}

#[test]
fn a_named_name_tag_names_the_cow_and_is_used_up() {
    let mut field = field(GameMode::Survival);
    let tag = world_stack(field.world(), "minecraft:name_tag", 2).unwrap();
    let cow = field.cow();

    // An unnamed tag does nothing.
    field.hold(tag.clone());
    field.interact(cow);
    assert!(field.world().get::<CustomName>(cow).is_none());
    assert_eq!(field.held().count, 2);

    field.hold(with_custom_name(&tag, Some("Daisy")));
    field.interact(cow);
    let world = field.world();
    assert_eq!(
        world.get::<CustomName>(cow),
        Some(&CustomName("Daisy".into()))
    );
    assert!(matches!(
        world.get::<EntityMetadata>(cow).unwrap().get(ids::NAME),
        Some(DataItemEntryPayload::DataItemStringPayload(row)) if &*row.value == "Daisy"
    ));
    assert!(world.get::<Persistent>(cow).is_some());
    assert_eq!(field.held().count, 1);
}
