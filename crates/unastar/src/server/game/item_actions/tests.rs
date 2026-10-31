//! Native item authorization and committed accounting regressions.

use super::*;
use crate::entity::components::{
    HeldSlot, Hunger, MainInventory, PlayerInput, PlayerState, Rotation, RuntimeEntityId,
};
use crate::item::test_support::stack;
use crate::server::game::item_use::{self, ItemUseQueue, ItemUsing, UseRequest};
use bevy_ecs::system::RunSystemOnce;
use jolyne::valentine::{McpePacket, McpePacketData};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::mpsc;

/// Minimal authoritative item actor with the same resources the normal path reads.
fn actor(item: &str, count: u8) -> (World, Entity, mpsc::Receiver<McpePacket>) {
    let mut world = World::new();
    let mut registry = crate::registry::ItemRegistry::new();
    registry.load_vanilla();
    world.insert_resource(crate::server::game::types::ItemRegistryResource(Arc::new(
        registry,
    )));
    world.insert_resource(crate::server::game::types::ServerConfigResource(
        Default::default(),
    ));
    world.init_resource::<crate::ecs::resources::TickCounter>();
    world.insert_resource(crate::entity::EntityRandom::seeded(7));
    world.init_resource::<ItemActionHooks>();
    world.init_resource::<ItemUseQueue>();
    world.init_resource::<crate::entity::projectile::ThrowQueue>();
    let (tx, rx) = mpsc::channel(256);
    let mut inventory = MainInventory::default();
    inventory.0.set_item(0, stack(item, count)).unwrap();
    let player = world
        .spawn((
            GameMode::Survival,
            inventory,
            HeldSlot(0),
            Health::new(20.0),
            Hunger {
                food_level: 10,
                saturation: 0.0,
                exhaustion: 0.0,
            },
            PlayerInput::default(),
            PlayerState::default(),
            Position(glam::DVec3::new(0.5, 65.62, 0.5)),
            Rotation::default(),
            RuntimeEntityId(1),
            PlayerSession::new(1, "Actor".into(), Some("123".into()), None, tx),
        ))
        .id();
    (world, player, rx)
}

/// Queue one ordinary use request without bypassing the item processing path.
fn request(world: &mut World, player: Entity, request: UseRequest) {
    world
        .resource_mut::<ItemUseQueue>()
        .0
        .push((player, request));
    item_use::apply_item_use(world);
}

/// Count only successful committed notifications.
struct CountCommitted(Arc<AtomicUsize>);
impl ItemActionHandler for CountCommitted {
    fn on_item_committed(&self, _world: &mut World, _event: &PlayerItemActionCommitted) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
/// Reject a selected stage.
struct Reject(ItemActionKind);
impl ItemActionHandler for Reject {
    fn on_item_action(&self, _world: &mut World, event: &mut PlayerItemAction) {
        if event.kind() == self.0 {
            event.cancel();
        }
    }
}

#[test]
fn vetoed_air_use_starts_no_animation_cost_or_committed_notification() {
    let (mut world, player, mut rx) = actor("minecraft:apple", 2);
    let count = Arc::new(AtomicUsize::new(0));
    world
        .resource_mut::<ItemActionHooks>()
        .register(Reject(ItemActionKind::Use));
    world
        .resource_mut::<ItemActionHooks>()
        .register(CountCommitted(count.clone()));
    request(&mut world, player, UseRequest::UseAir);
    assert!(world.get::<ItemUsing>(player).is_none());
    assert_eq!(
        world
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count,
        2
    );
    assert_eq!(world.get::<Hunger>(player).unwrap().food_level, 10);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(rx.try_recv().is_err());
}

#[test]
fn automatic_consume_has_its_own_veto_before_food_items_or_packets_change() {
    let (mut world, player, mut rx) = actor("minecraft:apple", 2);
    request(&mut world, player, UseRequest::UseAir);
    while rx.try_recv().is_ok() {}
    world
        .resource_mut::<ItemActionHooks>()
        .register(Reject(ItemActionKind::Consume));
    world
        .resource_mut::<crate::ecs::resources::TickCounter>()
        .current = 40;
    world.run_system_once(item_use::tick_using).unwrap();
    assert_eq!(
        world
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count,
        2
    );
    assert_eq!(world.get::<Hunger>(player).unwrap().food_level, 10);
    assert!(rx.try_recv().is_err());
}

/// Ask the engine to handle a custom item while leaving consumption to it.
struct Custom;
impl ItemActionHandler for Custom {
    fn on_item_action(&self, _world: &mut World, event: &mut PlayerItemAction) {
        if event.kind() == ItemActionKind::Use {
            event.handle(HandledItemAction {
                action: "custom.action".into(),
                consume: 1,
            });
        }
    }
}

#[test]
fn custom_handling_consumes_once_then_reports_actual_committed_state() {
    let (mut world, player, _) = actor("minecraft:snowball", 2);
    world.resource_mut::<ItemActionHooks>().register(Custom);
    let committed = Arc::new(AtomicUsize::new(0));
    let record = committed.clone();
    world.add_observer(
        move |event: On<PlayerItemActionCommitted>, inventory: Query<&MainInventory>| {
            record.fetch_add(1, Ordering::SeqCst);
            assert_eq!(
                event.outcome,
                ItemActionOutcome::Custom {
                    action: "custom.action".into(),
                    count: 1
                }
            );
            assert_eq!(
                inventory
                    .get(event.player)
                    .unwrap()
                    .0
                    .item(0)
                    .unwrap()
                    .count,
                1
            );
        },
    );
    request(&mut world, player, UseRequest::UseAir);
    assert_eq!(
        world
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count,
        1
    );
    assert!(
        world
            .resource::<crate::entity::projectile::ThrowQueue>()
            .0
            .is_empty()
    );
    assert_eq!(committed.load(Ordering::SeqCst), 1);
}

#[test]
fn late_observer_veto_prevents_custom_consumption_and_committed_callbacks() {
    let (mut world, player, mut rx) = actor("minecraft:snowball", 2);
    let count = Arc::new(AtomicUsize::new(0));
    world.resource_mut::<ItemActionHooks>().register(Custom);
    world
        .resource_mut::<ItemActionHooks>()
        .register(CountCommitted(count.clone()));
    world.add_observer(|mut event: On<PlayerItemAction>| event.cancel());
    request(&mut world, player, UseRequest::UseAir);
    assert_eq!(
        world
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count,
        2
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
    assert!(rx.try_recv().is_err());
}

#[test]
fn changed_held_slot_or_connection_cannot_consume_the_pre_callback_stack() {
    for connection in [false, true] {
        let (mut world, player, mut rx) = actor("minecraft:snowball", 2);
        world.resource_mut::<ItemActionHooks>().register(Custom);
        world.add_observer(
            move |event: On<PlayerItemAction>,
                  mut slots: Query<&mut HeldSlot>,
                  mut sessions: Query<&mut PlayerSession>| {
                if connection {
                    sessions.get_mut(event.player()).unwrap().session_id = 99;
                } else {
                    slots.get_mut(event.player()).unwrap().0 = 1;
                }
            },
        );
        request(&mut world, player, UseRequest::UseAir);
        assert_eq!(
            world
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .count,
            2
        );
        assert!(rx.try_recv().is_err());
    }
}

#[test]
fn ordinary_consumption_reports_after_food_and_held_stack_commit() {
    let (mut world, player, _) = actor("minecraft:apple", 2);
    let consumed = Arc::new(AtomicUsize::new(0));
    let record = consumed.clone();
    world.add_observer(
        move |event: On<PlayerItemActionCommitted>,
              food: Query<&Hunger>,
              inventory: Query<&MainInventory>| {
            if matches!(event.outcome, ItemActionOutcome::Consumed { count: 1 }) {
                record.fetch_add(1, Ordering::SeqCst);
                assert!(food.get(event.player).unwrap().food_level > 10);
                assert_eq!(
                    inventory
                        .get(event.player)
                        .unwrap()
                        .0
                        .item(0)
                        .unwrap()
                        .count,
                    1
                );
            }
        },
    );
    request(&mut world, player, UseRequest::UseAir);
    request(&mut world, player, UseRequest::Consume);
    assert_eq!(consumed.load(Ordering::SeqCst), 1);
    assert_eq!(
        world
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count,
        1
    );
}

#[test]
fn failed_projectile_launch_preserves_the_original_item_and_emits_no_launch_commit() {
    let (mut world, player, _) = actor("minecraft:snowball", 2);
    world.entity_mut(player).remove::<Rotation>();
    let count = Arc::new(AtomicUsize::new(0));
    world
        .resource_mut::<ItemActionHooks>()
        .register(CountCommitted(count.clone()));
    request(&mut world, player, UseRequest::UseAir);
    crate::entity::projectile::apply_throws(&mut world);
    assert_eq!(
        world
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count,
        2
    );
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[test]
fn food_policy_stops_exhaustion_drain_regeneration_and_starvation_independently() {
    use crate::server::game::hunger::{self, FoodTimer, PlayerFoodPolicy};
    let (mut world, player, _) = actor("minecraft:apple", 2);
    world.entity_mut(player).insert((
        FoodTimer::default(),
        PlayerFoodPolicy {
            exhaustion: false,
            food_drain: false,
            natural_regeneration: false,
            starvation: false,
        },
    ));
    world.get_mut::<Hunger>(player).unwrap().exhaustion = 10.0;
    world.get_mut::<Hunger>(player).unwrap().food_level = 20;
    world.get_mut::<Health>(player).unwrap().current = 10.0;
    hunger::exhaust_player(&mut world, player, |_| 4.0);
    for _ in 0..100 {
        world.run_system_once(hunger::tick_hunger).unwrap();
    }
    assert_eq!(world.get::<Hunger>(player).unwrap().food_level, 20);
    assert_eq!(world.get::<Hunger>(player).unwrap().exhaustion, 0.0);
    world
        .get_mut::<PlayerFoodPolicy>(player)
        .unwrap()
        .exhaustion = true;
    world.get_mut::<Hunger>(player).unwrap().exhaustion = 10.0;
    world.run_system_once(hunger::tick_hunger).unwrap();
    assert_eq!(world.get::<Hunger>(player).unwrap().food_level, 20);
    assert_eq!(world.get::<Hunger>(player).unwrap().exhaustion, 6.0);
    assert_eq!(world.get::<Health>(player).unwrap().current, 10.0);
    world.get_mut::<Hunger>(player).unwrap().food_level = 0;
    for _ in 0..100 {
        world.run_system_once(hunger::tick_hunger).unwrap();
    }
    assert_eq!(world.get::<Health>(player).unwrap().current, 10.0);
}

#[test]
fn consumed_notification_follows_the_normal_completion_packet() {
    let (mut world, player, mut rx) = actor("minecraft:apple", 1);
    request(&mut world, player, UseRequest::UseAir);
    request(&mut world, player, UseRequest::Consume);
    assert!(
        std::iter::from_fn(|| rx.try_recv().ok())
            .any(|packet| matches!(packet.data, McpePacketData::CompletedUsingItemPacket(_)))
    );
}

#[test]
fn successful_ordinary_throw_reports_only_after_spawn_and_item_cost_commit() {
    use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let (tx, _rx) = mpsc::channel(1 << 16);
    let player = server
        .spawn_player(PlayerSpawnData {
            session_id: 1,
            display_name: "Actor".into(),
            xuid: Some("123".into()),
            uuid: Some(uuid::Uuid::from_u128(1).to_string()),
            runtime_id: 1,
            position: glam::DVec3::new(0.5, 17.62, 0.5),
            outbound_tx: tx,
            saved_data: None,
            chunk_radius: 2,
        })
        .unwrap();
    let committed = Arc::new(AtomicUsize::new(0));
    let record = committed.clone();
    server.ecs.world_mut().add_observer(
        move |event: On<PlayerItemActionCommitted>,
              entities: Query<Entity>,
              inventory: Query<&MainInventory>| {
            if let ItemActionOutcome::Launched { projectile } = event.outcome {
                assert!(entities.get(projectile).is_ok());
                assert_eq!(
                    inventory
                        .get(event.player)
                        .unwrap()
                        .0
                        .item(0)
                        .unwrap()
                        .count,
                    1
                );
                record.fetch_add(1, Ordering::SeqCst);
            }
        },
    );
    server
        .ecs
        .world_mut()
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, stack("minecraft:snowball", 2))
        .unwrap();
    request(server.ecs.world_mut(), player, UseRequest::UseAir);
    assert_eq!(committed.load(Ordering::SeqCst), 0);
    crate::entity::projectile::apply_throws(server.ecs.world_mut());
    assert_eq!(committed.load(Ordering::SeqCst), 1);
}

#[test]
fn effect_callbacks_cannot_clobber_consumption_or_lose_accepted_source_identity() {
    use crate::entity::projectile::hooks::EffectApplication;
    let (mut world, player, _) = actor("minecraft:golden_apple", 2);
    let first = Arc::new(AtomicBool::new(true));
    let swap = first.clone();
    world.add_observer(
        move |event: On<EffectApplication>, mut inventory: Query<&mut MainInventory>| {
            if swap.swap(false, Ordering::SeqCst) {
                let mut inventory = inventory.get_mut(event.target()).unwrap();
                assert_eq!(inventory.0.item(0).unwrap().count, 1);
                inventory
                    .0
                    .set_item(0, stack("minecraft:stone", 3))
                    .unwrap();
            }
        },
    );
    let consumed = Arc::new(AtomicUsize::new(0));
    let record = consumed.clone();
    world.add_observer(move |event: On<PlayerItemActionCommitted>| {
        if let ItemActionOutcome::Consumed { count: 1 } = event.outcome {
            assert_eq!(event.stack.item_id(), "minecraft:golden_apple");
            assert_eq!(event.identity().xuid(), Some("123"));
            assert_eq!(event.identity().session_id(), Some(1));
            record.fetch_add(1, Ordering::SeqCst);
        }
    });
    request(&mut world, player, UseRequest::UseAir);
    request(&mut world, player, UseRequest::Consume);
    let inventory = world.get::<MainInventory>(player).unwrap();
    assert_eq!(
        (
            inventory.0.item(0).unwrap().item_id(),
            inventory.0.item(0).unwrap().count
        ),
        ("minecraft:stone", 3)
    );
    assert_eq!(consumed.load(Ordering::SeqCst), 1);
}

/// Complete automatic consumption using the engine-owned custom action path.
struct ConsumeCustom;
impl ItemActionHandler for ConsumeCustom {
    fn on_item_action(&self, _world: &mut World, event: &mut PlayerItemAction) {
        if event.kind() == ItemActionKind::Consume {
            event.handle(HandledItemAction {
                action: "consume.custom".into(),
                consume: 1,
            });
        }
    }
}

#[test]
fn custom_automatic_consumption_finishes_once_across_repeated_ticks() {
    let (mut world, player, _) = actor("minecraft:apple", 3);
    world
        .resource_mut::<ItemActionHooks>()
        .register(ConsumeCustom);
    request(&mut world, player, UseRequest::UseAir);
    world
        .resource_mut::<crate::ecs::resources::TickCounter>()
        .current = 40;
    world.run_system_once(item_use::tick_using).unwrap();
    assert!(world.get::<ItemUsing>(player).is_none());
    assert_eq!(item_use::held(&world, player).unwrap().1.count, 2);
    world
        .resource_mut::<crate::ecs::resources::TickCounter>()
        .current = 80;
    world.run_system_once(item_use::tick_using).unwrap();
    assert_eq!(item_use::held(&world, player).unwrap().1.count, 2);
}
