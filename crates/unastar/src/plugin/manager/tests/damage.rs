//! Actual component damage decisions and committed health loss through native hit/projectile lanes.

use super::*;
use crate::ecs::events::EventKind;
use crate::entity::components::{GameMode, Health, Position, Velocity};
use crate::entity::damage::DamageAttribution;
use crate::entity::mob::combat::{Hit, HurtTimer};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::player_life::hooks::PlayerDamage;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use crate::server::worlds::DimensionRouter;
use glam::DVec3;
use std::sync::{Arc, Mutex};
use unastar_data::actor_components::body::ActorDamageCause;

struct Harness {
    router: DimensionRouter,
    manager: Arc<Mutex<PluginManager>>,
    target: Entity,
    _packets: Vec<tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>>,
}
impl Harness {
    /// Install a real component before complete native joins in both world lifetimes.
    fn new(bodies: &[(&str, String)]) -> Self {
        let server = |dimension| {
            GameServer::with_config(ServerConfig {
                world: crate::world::WorldConfig {
                    dimension,
                    ..crate::world::WorldConfig::void_spawn_platform(1)
                },
                ..Default::default()
            })
        };
        let mut router = DimensionRouter::new(server(0));
        router.add(server(1)).unwrap();
        for (_, server) in router.iter_mut() {
            let world = server.ecs.world_mut();
            world.insert_resource(crate::entity::EntityRandom::seeded(17));
            let mut column = crate::world::chunk::Chunk::new(0, 0);
            for x in 0..16 {
                for z in 0..16 {
                    column.set_block(x, 63, z, *crate::world::chunk::blocks::STONE);
                }
            }
            let position = crate::world::ecs::ChunkPosition::new(0, 0);
            let chunk = world
                .spawn((crate::world::ecs::ChunkData::new(column), position))
                .id();
            world
                .resource_mut::<crate::world::ChunkManager>()
                .insert(position, chunk);
            world
                .resource_mut::<crate::world::rules::GameRules>()
                .do_mob_spawning = false;
        }
        let engine = engine();
        let manifest = PluginManifest {
            id: PluginId("example.damage".into()),
            name: "Damage".into(),
            version: "1".into(),
            api_version: PLUGIN_API_VERSION.into(),
            required: true,
            config: None,
            capabilities: [crate::plugin::manifest::PluginCapability::ReadPlayers].into(),
            subscriptions: [EventKind::PlayerDamage, EventKind::PlayerDamaged].into(),
            limits: PluginLimits::default(),
        };
        let pending = PendingPlugin {
            component: fixture::component(&engine, bodies),
            store: store(&engine, &manifest.limits),
            manifest,
        };
        let manager = Arc::new(Mutex::new(PluginManager {
            engine,
            plugins: Default::default(),
            pending: vec![pending],
            tick_prepared: false,
            tick_budget_valid: true,
            readiness: Default::default(),
            storage_registry: Default::default(),
        }));
        for (_, server) in router.iter_mut() {
            PluginManager::attach_to_world(manager.clone(), server.ecs.world_mut());
        }
        router.set_plugin_manager(manager.clone());
        router.initialize_plugins().unwrap();
        let (target, packets) = Self::spawn(&mut router, 43, DVec3::new(5.5, 65.62, 2.5));
        Self {
            router,
            manager,
            target,
            _packets: vec![packets],
        }
    }
    /// Trusted native ingress supplies canonical account/session facts, never a guest assertion.
    fn spawn(
        router: &mut DimensionRouter,
        session: u64,
        position: DVec3,
    ) -> (
        Entity,
        tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
    ) {
        let (outbound_tx, packets) = tokio::sync::mpsc::channel(8192);
        let entity = router
            .spawn_player_in(
                0,
                PlayerSpawnData {
                    session_id: session,
                    runtime_id: session as i64,
                    display_name: format!("Player{session}"),
                    xuid: Some(session.to_string()),
                    uuid: Some(uuid::Uuid::from_u128(session as u128).to_string()),
                    position,
                    outbound_tx,
                    saved_data: None,
                    chunk_radius: 0,
                },
            )
            .unwrap();
        (entity, packets)
    }
    /// Read the current exact-source world only for one native operation.
    fn world(&mut self) -> &mut bevy_ecs::prelude::World {
        self.router.world_mut(0).unwrap().ecs.world_mut()
    }
    /// Apply a real native player hit, returning actual health lost.
    fn hit(&mut self, damage: f32) -> f32 {
        let target = self.target;
        crate::entity::player_life::hurt::hurt_player(
            self.world(),
            target,
            &Hit {
                damage,
                cause: ActorDamageCause::EntityAttack,
                attacker: None,
                by_player: true,
                knockback_from: None,
                knockback_power: KnockbackPower::DEFAULT,
            },
        )
    }
    /// Read callback witnesses without guest reentry or budget changes.
    fn logs(&self) -> Vec<String> {
        self.manager
            .lock()
            .unwrap()
            .plugins
            .values()
            .next()
            .unwrap()
            .store
            .data()
            .test_logs
            .clone()
    }
}

/// Observe actual execution in the canonical guest through its bounded log import.
fn witness(value: u8) -> String {
    format!(
        "i32.const 1024 i32.const {value} i32.store8 i32.const 0 i32.const 1024 i32.const 1 call @log@"
    )
}

/// Construct a complete accepted response by copying current native tuning, then editing it.
fn tuning(edit: &str) -> String {
    let (offset, size) = fixture::field_layout("on-player-damage", "proposal", &["tuning"]);
    format!(
        "i32.const 512 i32.const 1 i32.store i32.const 520 local.get 0 i32.const {offset} i32.add i32.const {size} memory.copy {edit} i32.const 512"
    )
}

/// Verify native positive loss and incoming damage as separate canonical fields.
fn committed(incoming: f32, loss: f32) -> String {
    let (input, _) = fixture::field_layout("on-player-damaged", "damage", &["incoming-damage"]);
    let (actual, _) = fixture::field_layout("on-player-damaged", "damage", &["actual-health-loss"]);
    format!(
        "local.get 0 f32.load offset={input} f32.const {incoming} f32.ne if unreachable end local.get 0 f32.load offset={actual} f32.const {loss} f32.ne if unreachable end {}",
        witness(b'C')
    )
}

#[tokio::test]
async fn component_tuning_and_actual_health_loss_are_distinct_after_native_armour_multiplier() {
    let (base, _) = fixture::field_layout("on-player-damage", "proposal", &["tuning"]);
    let (post, _) = fixture::field_layout(
        "on-player-damage",
        "proposal",
        &["tuning", "post-armour-multiplier"],
    );
    let mut harness = Harness::new(&[
        (
            "on-player-damage",
            tuning(&format!(
                "i32.const 520 f32.const 8 f32.store i32.const {} f32.const 0.5 f32.store",
                520 + post - base
            )),
        ),
        ("on-player-damaged", committed(8.0, 4.0)),
    ]);
    assert_eq!(harness.hit(10.0), 4.0);
    assert_eq!(harness.logs(), ["C"]);
    let target = harness.target;
    assert_eq!(harness.world().get::<Health>(target).unwrap().current, 16.0);
}

#[tokio::test]
async fn cancellation_late_veto_and_immunity_never_emit_committed_loss() {
    for mode in 0..3 {
        let body = if mode == 0 {
            "i32.const 512 i32.const 0 i32.store i32.const 512".into()
        } else {
            tuning("")
        };
        let mut harness = Harness::new(&[
            ("on-player-damage", body),
            ("on-player-damaged", witness(b'C')),
        ]);
        let target = harness.target;
        if mode == 1 {
            harness
                .world()
                .add_observer(|mut event: bevy_ecs::prelude::On<PlayerDamage>| event.cancel());
        }
        if mode == 2 {
            harness
                .world()
                .entity_mut(target)
                .insert(GameMode::Creative);
        }
        assert_eq!(harness.hit(5.0), 0.0);
        assert!(harness.logs().is_empty());
        assert_eq!(harness.world().get::<Health>(target).unwrap().current, 20.0);
        assert!(harness.world().get::<HurtTimer>(target).is_none());
    }
}

#[tokio::test]
async fn timer_suppression_has_no_commit_but_delta_hit_reports_only_removed_health() {
    let mut harness = Harness::new(&[("on-player-damaged", witness(b'C'))]);
    assert_eq!(harness.hit(4.0), 4.0);
    assert_eq!(harness.hit(4.0), 0.0);
    assert_eq!(harness.hit(6.0), 2.0);
    assert_eq!(harness.logs(), ["C", "C"]);
    let target = harness.target;
    assert_eq!(harness.world().get::<Health>(target).unwrap().current, 14.0);
}

#[tokio::test]
async fn tuning_hurt_window_preserves_native_delta_deadline_and_knockback_fields() {
    let (base, _) = fixture::field_layout("on-player-damage", "proposal", &["tuning"]);
    let (ticks, _) =
        fixture::field_layout("on-player-damage", "proposal", &["tuning", "hurt-ticks"]);
    let (horizontal, _) = fixture::field_layout(
        "on-player-damage",
        "proposal",
        &["tuning", "knockback", "horizontal"],
    );
    let (committed_horizontal, _) =
        fixture::field_layout("on-player-damaged", "damage", &["knockback", "horizontal"]);
    let mut harness = Harness::new(&[
        (
            "on-player-damage",
            tuning(&format!(
                "i32.const {} i32.const 7 i32.store i32.const {} f64.const 0.25 f64.store",
                520 + ticks - base,
                520 + horizontal - base
            )),
        ),
        (
            "on-player-damaged",
            format!(
                "local.get 0 f64.load offset={committed_horizontal} f64.const 0.25 f64.ne if unreachable end {}",
                witness(b'C')
            ),
        ),
    ]);
    harness
        .world()
        .resource_mut::<crate::ecs::resources::TickCounter>()
        .current = 100;
    assert_eq!(harness.hit(4.0), 4.0);
    let target = harness.target;
    assert_eq!(
        harness.world().get::<HurtTimer>(target).unwrap().until_tick,
        107
    );
    harness
        .world()
        .resource_mut::<crate::ecs::resources::TickCounter>()
        .current = 102;
    assert_eq!(harness.hit(6.0), 2.0);
    assert_eq!(
        harness.world().get::<HurtTimer>(target).unwrap().until_tick,
        107
    );
    assert_eq!(harness.logs(), ["C", "C"]);
}

#[tokio::test]
async fn malformed_tuning_or_required_trap_rejects_before_health_and_applies_terminal_fence() {
    for body in [
        tuning("i32.const 520 f32.const nan f32.store"),
        "unreachable".into(),
    ] {
        let mut harness = Harness::new(&[
            ("on-player-damage", body),
            ("on-player-damaged", witness(b'C')),
        ]);
        assert_eq!(harness.hit(5.0), 0.0);
        assert!(harness.logs().is_empty());
        assert!(harness.manager.lock().unwrap().check_required().is_err());
        let target = harness.target;
        assert!(crate::server::game::inventory_effects::is_recovery_fenced(
            harness.world(),
            target
        ));
    }
}

#[tokio::test]
async fn real_arrow_hit_retains_launch_account_and_feet_after_departure_or_native_world_transfer() {
    for migrate in [false, true] {
        let (attacker, _) = fixture::field_layout("on-player-damage", "attacker", &[]);
        let (account, _) = fixture::field_layout(
            "on-player-damage",
            "proposal",
            &[
                "attribution",
                "owner",
                "some",
                "authenticated",
                "some",
                "xuid",
            ],
        );
        let (feet, _) = fixture::field_layout(
            "on-player-damage",
            "proposal",
            &["attribution", "owner", "some", "feet", "some", "x"],
        );
        let (lifetime, _) = fixture::field_layout(
            "on-player-damage",
            "proposal",
            &["attribution", "source-world-lifetime", "some"],
        );
        let proposal = format!(
            "local.get 0 i32.load8_u offset={attacker} if unreachable end local.get 0 i64.load offset={account} i64.const 42 i64.ne if unreachable end local.get 0 f64.load offset={feet} f64.const 2.5 f64.ne if unreachable end i32.const 0 local.get 0 i32.load offset={lifetime} local.get 0 i32.load offset={} call @log@ {}",
            lifetime + 4,
            tuning("i32.const 520 f32.const 3 f32.store")
        );
        let mut harness = Harness::new(&[
            ("on-player-damage", proposal),
            ("on-player-damaged", committed(3.0, 3.0)),
        ]);
        let (owner, packets) = Harness::spawn(&mut harness.router, 42, DVec3::new(2.5, 65.62, 2.5));
        harness._packets.push(packets);
        let projectile = crate::entity::projectile::launch_with(
            harness.world(),
            owner,
            unastar_data::actor_identifiers::ActorType::Arrow,
            None,
            crate::entity::projectile::LaunchSpec {
                throw_sound: false,
                ..Default::default()
            },
        )
        .unwrap();
        harness.world().entity_mut(projectile).insert((
            Position(DVec3::new(3.5, 64.9, 2.5)),
            Velocity(DVec3::ZERO),
            crate::entity::projectile::Stuck { since: 0 },
        ));
        let origin = *harness
            .world()
            .get::<DamageAttribution>(projectile)
            .unwrap();
        assert!(origin.world_lifetime().is_some());
        if migrate {
            crate::server::worlds::request_transfer_checked(
                harness.world(),
                owner,
                1,
                DVec3::new(4.5, 64.0, 4.5),
                0,
                "transfer".into(),
                |_, _, _| true,
            )
            .unwrap();
            for _ in 0..100 {
                harness.router.tick().await.unwrap();
                if harness
                    .router
                    .locate_session(42)
                    .is_some_and(|(dimension, _)| dimension == 1)
                {
                    break;
                }
            }
            assert_eq!(harness.router.locate_session(42).unwrap().0, 1);
        } else {
            harness.router.despawn_player(42);
        }
        assert!(origin.live_owner(harness.world()).is_none());
        harness
            .world()
            .entity_mut(projectile)
            .remove::<crate::entity::projectile::Stuck>()
            .insert(Velocity(DVec3::new(3.0, 0.0, 0.0)));
        harness.router.tick().await.unwrap();
        assert_eq!(
            harness.logs(),
            [origin.world_lifetime().unwrap().to_string(), "C".into()]
        );
        let target = harness.target;
        assert_eq!(harness.world().get::<Health>(target).unwrap().current, 17.0);
    }
}
