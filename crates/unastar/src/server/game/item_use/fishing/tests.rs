use bevy_ecs::prelude::*;
use glam::DVec3;
use rand::SeedableRng;
use rand::rngs::StdRng;

use super::*;
use crate::entity::components::{
    GameMode, HeldSlot, ItemEntityStack, MainInventory, OffhandSlot, PlayerInput, PlayerSession,
    PlayerState, RuntimeEntityId,
};
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::entity::xp_orb::XpOrb;
use crate::item::enchant::{durability_damage, with_enchantments};
use crate::item::test_support::stack;
use crate::server::game::item_use::{ItemUseQueue, UseRequest};
use crate::server::game::{GameServer, ServerConfig};
use crate::world::ChunkManager;
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkPosition, ChunkViewers};

struct Pond {
    server: GameServer,
    player: Entity,
    _rx: tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
}

/// Chunk (0, 0): stone at y = 63 for z < 4, water at y = 61..=63 for
/// z >= 4 over stone at y = 60. A survival player stands at (8.5, 64, 1.5)
/// facing south (+z, towards the water) holding `hotbar`.
fn pond(hotbar: &[ItemStack]) -> Pond {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(crate::entity::EntityRandom::seeded(3));
    let mut column = Chunk::new(0, 0);
    for x in 0..16 {
        for z in 0..16 {
            if z < 4 {
                column.set_block(x, 63, z, *blocks::STONE);
            } else {
                column.set_block(x, 60, z, *blocks::STONE);
                for y in 61..=63 {
                    column.set_block(x, y, z, *blocks::WATER);
                }
            }
        }
    }
    let chunk = world.spawn(ChunkData::new(column)).id();
    world
        .resource_mut::<ChunkManager>()
        .insert(ChunkPosition::new(0, 0), chunk);
    let (tx, rx) = tokio::sync::mpsc::channel(1 << 16);
    let mut main = MainInventory::default();
    for (slot, item) in hotbar.iter().enumerate() {
        main.0.set_item(slot, item.clone()).unwrap();
    }
    let player = world
        .spawn((
            Player,
            Position(DVec3::new(8.5, 64.0 + PLAYER_EYE_HEIGHT, 1.5)),
            Rotation::default(),
            GameMode::Survival,
            main,
            OffhandSlot::default(),
            HeldSlot(0),
            Health::default(),
            PlayerInput::default(),
            PlayerState::default(),
            RuntimeEntityId(1),
            PlayerSession::new(1, "angler".into(), None, None, tx),
        ))
        .id();
    let mut viewers = ChunkViewers::default();
    viewers.insert(player);
    world.entity_mut(chunk).insert(viewers);
    Pond {
        server,
        player,
        _rx: rx,
    }
}

impl Pond {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    fn use_rod(&mut self) {
        let player = self.player;
        self.world()
            .resource_mut::<ItemUseQueue>()
            .0
            .push((player, UseRequest::UseAir));
        self.server.tick();
    }

    fn hook(&mut self) -> Option<Entity> {
        let player = self.player;
        self.world().get::<CastHook>(player).map(|c| c.0)
    }

    fn state(&mut self) -> FishingHook {
        let hook = self.hook().expect("a hook is out");
        *self.world().get::<FishingHook>(hook).unwrap()
    }

    /// Tick until `done` holds, at most `limit` ticks.
    fn until(&mut self, limit: usize, mut done: impl FnMut(&mut Self) -> bool) -> bool {
        for _ in 0..limit {
            if done(self) {
                return true;
            }
            self.server.tick();
        }
        done(self)
    }

    fn rod(&mut self) -> ItemStack {
        let player = self.player;
        self.world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .clone()
    }

    fn count<C: Component>(&mut self) -> usize {
        let world = self.world();
        world.query::<&C>().iter(world).count()
    }
}

#[test]
fn the_fishing_hook_definition_spawns() {
    let mut pond = pond(&[]);
    let player = pond.player;
    let world = pond.world();
    let registry = world.resource::<crate::entity::mob::MobRegistry>().clone();
    assert!(
        registry.get("minecraft:fishing_hook").is_some(),
        "{:?}",
        registry.unsupported_reason("minecraft:fishing_hook")
    );
    launch_with(
        world,
        player,
        ActorType::FishingHook,
        None,
        LaunchSpec::default(),
    )
    .unwrap();
}

#[test]
fn casting_spawns_an_owned_hook_and_reeling_removes_it() {
    let mut pond = pond(&[stack(FISHING_ROD, 1)]);
    pond.use_rod();
    let hook = pond.hook().expect("cast");
    let world = pond.world();
    assert_eq!(
        world.get::<MobKind>(hook).unwrap().0.identifier.as_ref(),
        "minecraft:fishing_hook"
    );
    assert!(matches!(
        world.get::<EntityMetadata>(hook).unwrap().get(ids::OWNER),
        Some(jolyne::valentine::types::DataItemEntryPayload::DataItemInt64Payload(p))
            if p.value == 1
    ));
    let velocity = world.get::<Velocity>(hook).unwrap().0;
    assert!(velocity.z > 0.5, "flies south: {velocity}");
    pond.use_rod();
    assert!(pond.hook().is_none());
    assert!(pond.world().get_entity(hook).is_err());
    // Reeled in mid-air: no wear.
    assert_eq!(durability_damage(&pond.rod()), 0);
}

#[test]
fn hook_lands_in_water_bites_and_reels_in_a_catch() {
    let rod = with_enchantments(&stack(FISHING_ROD, 1), &[(Enchantment::Lure, 1)]);
    let mut pond = pond(&[rod]);
    pond.use_rod();
    assert!(pond.until(60, |p| p.state().state == HookState::Bobbing));
    assert_eq!(pond.state().lure, 1);
    let hook = pond.hook().unwrap();
    // Floats at the surface (y = 64) once settled.
    pond.until(40, |_| false);
    let y = pond.world().get::<Position>(hook).unwrap().0.y;
    assert!((63.5..64.3).contains(&y), "bobbing at {y}");
    // Skip most of the wait, then the approach and bite run by themselves.
    pond.world()
        .get_mut::<FishingHook>(hook)
        .unwrap()
        .until_lured = 1;
    assert!(pond.until(120, |p| p.state().biting()));
    let (items, orbs) = (pond.count::<ItemEntityStack>(), pond.count::<XpOrb>());
    pond.use_rod();
    assert!(pond.hook().is_none());
    assert!(pond.count::<ItemEntityStack>() > items, "a catch appears");
    assert_eq!(pond.count::<XpOrb>(), orbs + 1);
    let world = pond.world();
    let mut catches = world.query::<(&ItemEntityStack, &Velocity)>();
    let (_, velocity) = catches.iter(world).next().unwrap();
    assert!(
        velocity.0.z < 0.0,
        "flung towards the angler: {}",
        velocity.0
    );
    assert_eq!(durability_damage(&pond.rod()), WEAR_CATCH);
}

#[test]
fn a_missed_bite_starts_a_new_wait() {
    let mut rng = StdRng::seed_from_u64(1);
    let mut state = FishingHook::new(Entity::PLACEHOLDER, 0, 0);
    bite_cycle(&mut state, &mut rng);
    assert!(WAIT_TICKS.contains(&state.until_lured));
    state.until_lured = 1;
    bite_cycle(&mut state, &mut rng);
    assert!(APPROACH_TICKS.contains(&state.until_hooked));
    state.until_hooked = 1;
    bite_cycle(&mut state, &mut rng);
    assert!(BITE_TICKS.contains(&state.nibble));
    for _ in 0..state.nibble {
        bite_cycle(&mut state, &mut rng);
    }
    assert!(!state.biting());
    assert_eq!((state.until_lured, state.until_hooked), (0, 0));
}

#[test]
fn lure_shortens_the_wait() {
    let mut rng = StdRng::seed_from_u64(9);
    let mut state = FishingHook::new(Entity::PLACEHOLDER, 3, 0);
    bite_cycle(&mut state, &mut rng);
    assert!((-200..=300).contains(&state.until_lured));
}

#[test]
fn switching_away_from_the_rod_removes_the_hook() {
    let mut pond = pond(&[stack(FISHING_ROD, 1)]);
    pond.use_rod();
    let hook = pond.hook().unwrap();
    let player = pond.player;
    pond.world().get_mut::<HeldSlot>(player).unwrap().0 = 1;
    pond.server.tick();
    assert!(pond.world().get_entity(hook).is_err());
    assert!(pond.hook().is_none());
}

#[test]
fn hooking_a_cow_pulls_it_and_costs_more_durability() {
    let mut pond = pond(&[stack(FISHING_ROD, 1)]);
    let cow = spawn_mob(
        pond.world(),
        "minecraft:cow",
        DVec3::new(8.5, 64.0, 4.5),
        0.0,
        SpawnCause::Spawned,
    )
    .unwrap();
    // Keep the cow on the stone shelf.
    pond.world()
        .entity_mut(cow)
        .insert(Position(DVec3::new(8.5, 64.0, 3.0)));
    pond.use_rod();
    assert!(pond.until(20, |p| p.state().state == HookState::Hooked(cow)));
    let hook = pond.hook().unwrap();
    let hook_y = pond.world().get::<Position>(hook).unwrap().0.y;
    let cow_y = pond.world().get::<Position>(cow).unwrap().0.y;
    assert!(hook_y > cow_y, "rides on the cow");
    let before = pond.world().get::<Velocity>(cow).unwrap().0;
    let hook_z = pond.world().get::<Position>(hook).unwrap().0.z;
    let player = pond.player;
    let wear = reel(pond.world(), player, hook);
    let after = pond.world().get::<Velocity>(cow).unwrap().0;
    assert_eq!(wear, WEAR_ACTOR);
    assert!(
        (after.z - before.z - (1.5 - hook_z) * PULL).abs() < 1e-9,
        "pulled north: {before} -> {after}"
    );
}

#[test]
fn luck_of_the_sea_shifts_catches_to_treasure() {
    let table = unastar_data::loot::get("gameplay/fishing").unwrap();
    let items = {
        let mut items = crate::registry::ItemRegistry::new();
        items.load_vanilla();
        items
    };
    let treasure = |luck: f32| {
        let ctx = LootContext {
            luck,
            items: Some(&items),
            ..LootContext::default()
        };
        (0..2000u64)
            .filter(|seed| {
                let drops = unastar_rules::loot::roll(
                    unastar_data::loot::vanilla(),
                    table,
                    &ctx,
                    &mut StdRng::seed_from_u64(*seed),
                )
                .unwrap();
                matches!(
                    drops[0].item_id.as_str(),
                    "minecraft:nautilus_shell"
                        | "minecraft:name_tag"
                        | "minecraft:saddle"
                        | "minecraft:bow"
                        | "minecraft:enchanted_book"
                        | "minecraft:book"
                )
            })
            .count()
    };
    let (plain, lucky) = (treasure(0.0), treasure(3.0));
    // Treasure without the fishing rod entry: 26/31 of 5 % (about 4.2 %)
    // without luck; with Luck of the Sea III the weights become 4/11/82, so
    // 26/31 of 11/97 (about 9.5 %).
    assert!((50..120).contains(&plain), "{plain}");
    assert!((140..240).contains(&lucky), "{lucky}");
}
