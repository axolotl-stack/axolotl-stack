//! Real components share a single router cycle across isolated worlds.

use super::*;
use crate::ecs::{EntityLogicSet, TickCounter};
use crate::entity::components::{GameMode, Health};
use crate::entity::mob::combat::Hit;
use crate::entity::mob::components::KnockbackPower;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use crate::server::worlds::{
    DimensionRouter, RouterTickHandler, WorldDirectory, queue_router_request,
};
use bevy_ecs::prelude::*;
use std::sync::{Arc, Mutex};
use unastar_data::actor_components::body::ActorDamageCause;

const KEEP_DAMAGE: &str =
    "i32.const 32 i32.const 1 i32.store i32.const 36 local.get 4 f32.store i32.const 32";
const KEEP_POSITION: &str = "i32.const 64 i32.const 1 i32.store i32.const 72 local.get 1 f64.store i32.const 80 local.get 2 f64.store i32.const 88 local.get 3 f64.store i32.const 64";

/// Connected fixture transport, retained for the complete native cycle.
type Client = (
    Entity,
    tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
);

/// Three real worlds, each with a complete connected player and an independent clock.
fn router() -> (DimensionRouter, Vec<Client>) {
    let config = |dimension| ServerConfig {
        world: crate::world::WorldConfig {
            dimension,
            ..crate::world::WorldConfig::void_spawn_platform(1)
        },
        ..ServerConfig::default()
    };
    let mut router = DimensionRouter::new(GameServer::with_config(config(0)));
    for dimension in [1, 2] {
        let world = router.create_world(config(dimension)).unwrap();
        router.add(world).unwrap();
    }
    let mut clients = Vec::new();
    for (dimension, server) in router.iter_mut() {
        server.ecs.world_mut().resource_mut::<TickCounter>().current = if dimension == 0 {
            42
        } else {
            dimension as u64 * 500
        };
        let (tx, rx) = tokio::sync::mpsc::channel(65536);
        let player = server
            .spawn_player(PlayerSpawnData {
                session_id: dimension as u64 + 1,
                runtime_id: i64::from(dimension) + 1,
                display_name: format!("Player{dimension}"),
                xuid: Some((dimension + 1).to_string()),
                uuid: Some(uuid::Uuid::from_u128(dimension as u128 + 1).to_string()),
                position: glam::DVec3::new(0.5, 65.62, 0.5),
                outbound_tx: tx,
                saved_data: None,
                chunk_radius: 0,
            })
            .unwrap();
        server
            .ecs
            .world_mut()
            .entity_mut(player)
            .insert(GameMode::Survival);
        clients.push((player, rx));
    }
    router.refresh_directory();
    (router, clients)
}

/// Load one genuine component, shared by every world without cloning guest state.
fn attach(
    router: &mut DimensionRouter,
    kinds: &[crate::ecs::events::EventKind],
    damage: &str,
    tick: &str,
) -> Arc<Mutex<PluginManager>> {
    let engine = engine();
    let manifest = PluginManifest {
        id: PluginId("example.global".into()),
        name: "Global".into(),
        version: "1".into(),
        api_version: PLUGIN_API_VERSION.into(),
        required: false,
        config: None,
        capabilities: [crate::plugin::manifest::PluginCapability::ReadPlayers].into(),
        subscriptions: kinds.iter().copied().collect(),
        limits: PluginLimits {
            fuel_per_tick: 10_000,
            ..Default::default()
        },
    };
    let pending = PendingPlugin {
        component: component_with_tick(&engine, true, damage, KEEP_POSITION, tick),
        store: store(&engine, &manifest.limits),
        manifest,
    };
    let manager = Arc::new(Mutex::new(PluginManager {
        engine,
        plugins: HashMap::new(),
        pending: vec![pending],
        tick_prepared: false,
        tick_budget_valid: true,
        readiness: Default::default(),
        storage_registry: Default::default(),
        command_roots: Default::default(),
        ui_registry: Default::default(),
        view_registry: Default::default(),
    }));
    for (_, server) in router.iter_mut() {
        PluginManager::attach_to_world(manager.clone(), server.ecs.world_mut());
    }
    router.set_plugin_manager(manager.clone());
    manager
}

/// Read actions from actual host imports without running a second guest callback.
fn actions(router: &mut DimensionRouter, dimension: i32) -> Vec<PluginAction> {
    router
        .world_mut(dimension)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<crate::ecs::events::ActionQueue>()
        .drain()
}

#[tokio::test]
async fn component_tick_runs_once_with_the_global_player_directory_and_primary_clock() {
    let (mut router, _clients) = router();
    let manager = attach(
        &mut router,
        &[crate::ecs::events::EventKind::Tick],
        KEEP_DAMAGE,
        r#"
        local.get 0 call $tick i64.ne if unreachable end
        i32.const 128 i32.const 128 i32.load i32.const 1 i32.add i32.store
        i32.const 1024 i32.const 128 i32.load i32.const 48 i32.add i32.store8
        i32.const 0 i32.const 1024 i32.const 1 call @log@
        i32.const 256 call @players@
        i32.const 256 i32.load8_u if unreachable end
        i32.const 1025 i32.const 264 i32.load i32.const 48 i32.add i32.store8
        i32.const 0 i32.const 1025 i32.const 1 call @log@
    "#,
    );
    for expected in [1, 2] {
        router.tick().await.unwrap();
        let logs = {
            let mut manager = manager.lock().unwrap();
            std::mem::take(
                &mut manager
                    .plugins
                    .get_mut(&PluginId("example.global".into()))
                    .unwrap()
                    .store
                    .data_mut()
                    .test_logs,
            )
        };
        assert_eq!(logs, [expected.to_string(), "3".into()]);
        assert!(actions(&mut router, 0).is_empty());
        assert!(actions(&mut router, 1).is_empty());
        assert!(actions(&mut router, 2).is_empty());
    }
    let mut manager = manager.lock().unwrap();
    let plugin = manager
        .plugins
        .get_mut(&PluginId("example.global".into()))
        .unwrap();
    let names = with_world(
        &mut plugin.store,
        router.world_mut(0).unwrap().ecs.world_mut(),
        |store| {
            let context = store.data_mut();
            Host::players(context)
                .unwrap()
                .into_iter()
                .map(|handle| {
                    wit_bindings::unastar::plugin::types::HostPlayerHandle::identity(
                        context, handle,
                    )
                    .unwrap()
                    .name
                })
                .collect::<Vec<_>>()
        },
    );
    assert_eq!(names, ["Player0", "Player1", "Player2"]);
    assert!(!manager.tick_prepared);
}

#[derive(Resource)]
struct Target(Entity);

/// One ordinary native damage operation during each world's simulation.
fn hit(world: &mut World) {
    let target = world.resource::<Target>().0;
    apply_hit(world, target);
}

/// Override damage avoids an unrelated hurt window masking the next cycle's result.
fn apply_hit(world: &mut World, target: Entity) {
    crate::entity::player_life::hurt_player(
        world,
        target,
        &Hit {
            damage: 1.0,
            cause: ActorDamageCause::Override,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: KnockbackPower::DEFAULT,
        },
    );
}

/// Deferred global work must retain the same budget after the aggregate callback.
struct QueuedHit;
impl RouterTickHandler for QueuedHit {
    fn on_tick(&self, router: &mut DimensionRouter, directory: &WorldDirectory) {
        let target = directory
            .players()
            .find(|player| player.dimension() == 2)
            .unwrap()
            .clone();
        assert!(queue_router_request(
            router.world_mut(2).unwrap().ecs.world_mut(),
            move |router, _| {
                let world = router.world_mut(2).unwrap().ecs.world_mut();
                assert!(target.matches(world));
                apply_hit(world, target.player());
            }
        ));
    }
}

/// Observe the actual component budget after each synchronous callback returns.
struct RecordFuel {
    manager: Arc<Mutex<PluginManager>>,
    remaining: Arc<Mutex<Vec<u64>>>,
}
impl crate::entity::player_life::hooks::PlayerLifecycleHandler for RecordFuel {
    fn on_damage(
        &self,
        _world: &mut World,
        _event: &mut crate::entity::player_life::hooks::PlayerDamage,
    ) {
        let manager = self.manager.lock().unwrap();
        let fuel = manager.plugins[&PluginId("example.global".into())]
            .store
            .get_fuel()
            .unwrap();
        self.remaining.lock().unwrap().push(fuel);
    }
}

#[tokio::test]
async fn component_fuel_is_not_replenished_between_worlds_or_deferred_router_work() {
    let (mut router, clients) = router();
    let manager = attach(
        &mut router,
        &[crate::ecs::events::EventKind::PlayerDamage],
        &format!(
            r#"
            i32.const 128 i32.const 50 i32.store
            (loop $burn
                i32.const 128 i32.const 128 i32.load i32.const 1 i32.sub i32.store
                i32.const 128 i32.load br_if $burn)
            {KEEP_DAMAGE}
        "#
        ),
        "",
    );
    let remaining = Arc::new(Mutex::new(Vec::new()));
    for ((_, server), (player, _)) in router.iter_mut().zip(&clients) {
        server.ecs.world_mut().insert_resource(Target(*player));
        server
            .ecs
            .world_mut()
            .resource_mut::<crate::entity::player_life::hooks::PlayerLifecycleHooks>()
            .register(RecordFuel {
                manager: manager.clone(),
                remaining: remaining.clone(),
            });
        server
            .ecs
            .schedule_mut()
            .add_systems(hit.in_set(EntityLogicSet));
    }
    router.register_tick_handler(QueuedHit);
    router.tick().await.unwrap();
    let health = |router: &DimensionRouter| {
        router
            .iter()
            .zip(&clients)
            .map(|((_, server), (player, _))| {
                server.ecs.world().get::<Health>(*player).unwrap().current
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(health(&router), [19.0, 19.0, 18.0]);
    router.tick().await.unwrap();
    assert_eq!(health(&router), [18.0, 18.0, 16.0]);
    let remaining = remaining.lock().unwrap();
    assert_eq!(remaining.len(), 8);
    assert!(
        remaining[..4].windows(2).all(|pair| pair[1] < pair[0]),
        "{remaining:?}"
    );
    assert!(
        remaining[4..].windows(2).all(|pair| pair[1] < pair[0]),
        "{remaining:?}"
    );
    assert_eq!(
        remaining[0], remaining[4],
        "the next cycle renews exactly once"
    );
    assert!(!manager.lock().unwrap().tick_prepared);
}

/// Require a real already-attached component without creating another guest instance.
fn require(manager: &Arc<Mutex<PluginManager>>) {
    let mut manager = manager.lock().unwrap();
    let id = manager.pending[0].manifest.id.0.clone();
    manager.pending[0].manifest.required = true;
    manager.readiness.require(&id);
}

#[tokio::test]
async fn required_tick_trap_fences_all_worlds_and_never_recovers_on_next_fuel_cycle() {
    use crate::server::game::{inventory_effects, player_state};
    let (mut router, clients) = router();
    let manager = attach(
        &mut router,
        &[crate::ecs::events::EventKind::Tick],
        KEEP_DAMAGE,
        "unreachable",
    );
    require(&manager);
    assert!(matches!(
        router.tick().await,
        Err(crate::server::worlds::WorldTickError::RequiredComponentUnavailable(_))
    ));
    let clocks: Vec<_> = router
        .iter()
        .map(|(_, server)| server.ecs.world().resource::<TickCounter>().current)
        .collect();
    for (dimension, server) in router.iter_mut() {
        let player = clients[dimension as usize].0;
        let world = server.ecs.world_mut();
        assert!(inventory_effects::is_recovery_fenced(world, player));
        assert!(player_state::capture(world, player).is_err());
        assert!(world.get::<Health>(player).is_none());
        assert!(
            world
                .resource_mut::<crate::ecs::events::ActionQueue>()
                .drain()
                .is_empty()
        );
        assert!(!super::super::readiness::admission_allowed(world));
    }
    assert!(router.tick().await.is_err());
    assert_eq!(
        clocks,
        router
            .iter()
            .map(|(_, server)| server.ecs.world().resource::<TickCounter>().current)
            .collect::<Vec<_>>()
    );
    assert_eq!(router.save_all_players().await, 0);
}

#[tokio::test]
async fn required_damage_trap_vetoes_before_commit_and_fences_the_exact_player() {
    use crate::server::game::inventory_effects;
    let (mut router, clients) = router();
    let manager = attach(
        &mut router,
        &[crate::ecs::events::EventKind::PlayerDamage],
        "unreachable",
        "",
    );
    require(&manager);
    router.initialize_plugins().unwrap();
    let world = router.world_mut(1).unwrap().ecs.world_mut();
    let player = clients[1].0;
    assert_eq!(
        crate::entity::player_life::hurt_player(
            world,
            player,
            &Hit {
                damage: 5.0,
                cause: ActorDamageCause::Override,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: KnockbackPower::DEFAULT,
            }
        ),
        0.0
    );
    assert!(inventory_effects::is_recovery_fenced(world, player));
    assert!(
        world
            .get::<crate::entity::mob::combat::HurtTimer>(player)
            .is_none()
    );
    assert!(router.tick().await.is_err());
    assert!(inventory_effects::is_recovery_fenced(
        router.world(2).unwrap().ecs.world(),
        clients[2].0
    ));
}

#[tokio::test]
async fn required_legitimate_damage_veto_preserves_readiness_and_player_state() {
    let (mut router, clients) = router();
    let manager = attach(
        &mut router,
        &[crate::ecs::events::EventKind::PlayerDamage],
        "i32.const 32 i32.const 0 i32.store i32.const 32",
        "",
    );
    require(&manager);
    router.initialize_plugins().unwrap();
    let world = router.world_mut(1).unwrap().ecs.world_mut();
    let player = clients[1].0;
    apply_hit(world, player);
    assert_eq!(world.get::<Health>(player).unwrap().current, 20.0);
    assert!(super::super::readiness::admission_allowed(world));
    router.tick().await.unwrap();
}

#[path = "scoped.rs"]
mod scoped_tests;
