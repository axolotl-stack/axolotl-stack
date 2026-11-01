//! Actual Rust component calls retain native lifetime, actor lineage and fall observations.

use super::*;
use crate::entity::components::{ActiveEffect, FallDistance, PlayerUuid};
use crate::entity::player_life::environment::PlayerFall;
use crate::server::game::{inventory_effects as inventory, player_events, player_state};
use bevy_ecs::prelude::World;
use bevy_ecs::schedule::IntoScheduleConfigs;
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

#[path = "vitals.rs"]
mod vitals;

/// Compile a real SDK component without contending for the outer native artifact directory.
fn guest() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/mechanics-extension/Cargo.toml");
        let target = tempfile::tempdir().unwrap();
        let mut command = Command::new(env!("CARGO"));
        command
            .args([
                "build",
                "--locked",
                "--target",
                "wasm32-wasip2",
                "--manifest-path",
            ])
            .arg(manifest)
            .env("CARGO_TARGET_DIR", target.path())
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS");
        if let Some(toolchain) = option_env!("RUSTUP_TOOLCHAIN") {
            command.env("RUSTUP_TOOLCHAIN", toolchain);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "mechanics component build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read(
            target
                .path()
                .join("wasm32-wasip2/debug/mechanics_extension_fixture.wasm"),
        )
        .unwrap()
    })
}

impl Harness {
    /// The same real native actor and production loader execute the Rust fixture.
    fn extended(permitted: bool) -> Self {
        Self::component_factory(
            permitted,
            &[PluginCapability::Commands, PluginCapability::ScheduleTasks],
            true,
            false,
            |engine| wasmtime::component::Component::new(engine, guest()).unwrap(),
        )
    }
    /// Exercise the native command request boundary, including valid handled cancellation.
    fn command(&mut self, command: &str) {
        assert!(commands::dispatch(
            self.server.ecs.world_mut(),
            self.player,
            command,
            None
        ));
    }
}

/// Dispatch already accepted real world tasks using the actual registered-world router.
fn tasks(
    harness: Harness,
) -> (
    crate::server::worlds::DimensionRouter,
    Arc<Mutex<PluginManager>>,
    Entity,
) {
    let player = harness.player;
    let manager = harness.manager;
    let mut router = crate::server::worlds::DimensionRouter::new(harness.server);
    router.set_plugin_manager(manager.clone());
    manager.lock().unwrap().drain_scheduled_tasks(&mut router);
    players::apply_mechanics(router.world_mut(0).unwrap().ecs.world_mut());
    (router, manager, player)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn infinite_native_effects_survive_expiry_keep_pulsing_and_encode_permanent_duration() {
    let mut harness = Harness::extended(true);
    harness.command("probe infinite");
    assert!(
        harness
            .server
            .ecs
            .world()
            .get::<Effects>(harness.player)
            .is_none_or(|effects| !effects.has(EffectType::SPEED))
    );
    harness.apply();
    harness.command("probe regeneration");
    harness.apply();
    harness
        .server
        .ecs
        .world_mut()
        .get_mut::<Health>(harness.player)
        .unwrap()
        .current = 10.0;
    let mut schedule = bevy_ecs::schedule::Schedule::default();
    schedule.add_systems(
        (
            crate::entity::systems::effects::tick_effects,
            crate::server::game::effects::apply_player_effects,
        )
            .chain(),
    );
    for _ in 0..200 {
        schedule.run(harness.server.ecs.world_mut());
    }
    let effect = harness
        .server
        .ecs
        .world()
        .get::<Effects>(harness.player)
        .unwrap()
        .get(EffectType::SPEED)
        .unwrap();
    assert!(effect.infinite && !effect.is_expired());
    assert_eq!(effect.duration_ticks, u32::MAX - 200);
    assert_eq!(
        harness
            .server
            .ecs
            .world()
            .get::<Health>(harness.player)
            .unwrap()
            .current,
        20.0
    );
    harness.sync();
    let packets = std::iter::from_fn(|| harness.packets.try_recv().ok())
        .filter_map(|packet| match packet.data {
            McpePacketData::MobEffectPacket(effect) => Some(effect),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        packets
            .iter()
            .any(|effect| effect.effect_id == 1 && effect.effect_duration_ticks == -1)
    );
    let snapshot = player_state::capture(harness.server.ecs.world(), harness.player).unwrap();
    player_state::restore(harness.server.ecs.world_mut(), harness.player, &snapshot).unwrap();
    assert!(std::iter::from_fn(|| harness.packets.try_recv().ok()).any(|packet| matches!(packet.data, McpePacketData::MobEffectPacket(effect) if effect.effect_id == 1 && effect.effect_duration_ticks == -1)));
    harness.command("probe remove");
    harness.apply();
    assert!(
        !harness
            .server
            .ecs
            .world()
            .get::<Effects>(harness.player)
            .unwrap()
            .has(EffectType::SPEED)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn infinite_application_respects_foreign_precedence_and_instant_validation() {
    let mut harness = Harness::extended(true);
    harness
        .server
        .ecs
        .world_mut()
        .get_mut::<Effects>(harness.player)
        .unwrap()
        .apply(EffectType::SPEED, ActiveEffect::new(4, 1000));
    harness.command("probe infinite");
    harness.apply();
    let effect = harness
        .server
        .ecs
        .world()
        .get::<Effects>(harness.player)
        .unwrap()
        .get(EffectType::SPEED)
        .unwrap();
    assert_eq!((effect.level, effect.infinite), (4, false));
    harness.command("probe strong");
    harness.apply();
    harness.command("probe finite");
    harness.apply();
    let effect = harness
        .server
        .ecs
        .world()
        .get::<Effects>(harness.player)
        .unwrap()
        .get(EffectType::SPEED)
        .unwrap();
    assert_eq!((effect.level, effect.infinite), (6, true));
    let health = harness
        .server
        .ecs
        .world()
        .get::<Health>(harness.player)
        .unwrap();
    let health = (health.current, health.max);
    harness.command("probe instant");
    harness.apply();
    assert!(harness.logs().iter().any(|log| log == "instant-rejected"));
    let current = harness
        .server
        .ecs
        .world()
        .get::<Health>(harness.player)
        .unwrap();
    assert_eq!((current.current, current.max), health);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn infinite_permission_and_trap_paths_never_publish_effects() {
    let mut denied = Harness::extended(false);
    denied.command("probe denied");
    denied.apply();
    assert!(denied.logs().iter().any(|log| log == "denied"));
    assert!(
        !denied
            .server
            .ecs
            .world()
            .get::<Effects>(denied.player)
            .unwrap()
            .has(EffectType::SPEED)
    );
    let mut trapped = Harness::extended(true);
    trapped.command("probe trap");
    trapped.apply();
    assert!(
        trapped
            .server
            .ecs
            .world()
            .get::<Effects>(trapped.player)
            .is_none_or(|effects| !effects.has(EffectType::SPEED))
    );
    assert!(trapped.manager.lock().unwrap().readiness.check().is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_player_fall_history_precedes_standalone_facts_and_missing_is_not_zero() {
    let mut harness = Harness::extended(true);
    harness
        .server
        .ecs
        .world_mut()
        .entity_mut(harness.player)
        .insert((
            PlayerFall {
                fall: FallDistance {
                    distance: 3.25,
                    landed: None,
                },
                landing: None,
            },
            FallDistance {
                distance: 9.0,
                landed: None,
            },
        ));
    harness.command("probe fall");
    assert!(harness.logs().iter().any(|log| log == "fall:3.25"));
    harness
        .server
        .ecs
        .world_mut()
        .entity_mut(harness.player)
        .remove::<PlayerFall>();
    harness.command("probe fall");
    assert!(harness.logs().iter().any(|log| log == "fall:9"));
    harness
        .server
        .ecs
        .world_mut()
        .entity_mut(harness.player)
        .remove::<FallDistance>();
    harness.command("probe fall");
    assert!(harness.logs().iter().any(|log| log == "fall:none"));
    harness
        .server
        .ecs
        .world_mut()
        .entity_mut(harness.player)
        .insert(PlayerFall {
            fall: FallDistance {
                distance: f64::NAN,
                landed: None,
            },
            landing: None,
        });
    harness.command("probe fall");
    assert!(harness.logs().iter().any(|log| log == "fall:unavailable"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legitimate_retained_actor_accepts_infinite_mechanics_from_a_fresh_world_task() {
    let mut harness = Harness::extended(true);
    harness.command("probe remember");
    let original = harness.player;
    let uuid = harness
        .server
        .ecs
        .world()
        .get::<PlayerUuid>(original)
        .unwrap()
        .0;
    harness.server.despawn_player(42);
    assert!(
        harness
            .server
            .ecs
            .world()
            .get::<player_events::DetachedPlayer>(original)
            .is_some()
    );
    assert!(
        harness
            .server
            .ecs
            .world()
            .get::<PlayerSession>(original)
            .is_none()
    );
    let (router, manager, player) = tasks(harness);
    assert_eq!(player, original);
    let world = router.world(0).unwrap().ecs.world();
    assert_eq!(world.get::<PlayerUuid>(player).unwrap().0, uuid);
    assert!(
        world
            .get::<Effects>(player)
            .unwrap()
            .get(EffectType::RESISTANCE)
            .unwrap()
            .infinite
    );
    assert_eq!(
        crate::command::exec::base_game_mode(world, player),
        Some(GameMode::Adventure)
    );
    assert_eq!(
        crate::command::exec::mode_override(world, player),
        Some(GameMode::Creative)
    );
    assert_eq!(
        crate::server::game::flight::override_value(world, player),
        Some(true)
    );
    assert_eq!(
        world.get::<Velocity>(player).unwrap().0,
        glam::DVec3::new(0.25, 0.5, -0.25)
    );
    assert!(manager.lock().unwrap().readiness.check().is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retained_dead_incomplete_recovery_and_quarantined_actors_cannot_receive_mechanics() {
    for mode in ["dead", "incomplete", "recovery", "quarantine"] {
        let mut harness = Harness::extended(true);
        harness.command("probe remember");
        let directory = tempfile::tempdir().unwrap();
        if mode == "recovery" {
            let source =
                inventory::RecoverySource::capture(harness.server.ecs.world(), harness.player)
                    .unwrap();
            inventory::fence_for_recovery(harness.server.ecs.world_mut(), &source).unwrap();
        }
        if mode == "quarantine" {
            harness.server.ecs.world_mut().insert_resource(
                crate::server::game::types::PlayerProviderResource(Arc::new(
                    crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap(),
                )),
            );
            inventory::install(&mut harness.server).unwrap();
            let expected =
                inventory::InventoryState::capture(harness.server.ecs.world(), harness.player)
                    .unwrap();
            let mut replacement = expected.clone();
            replacement.held_slot = 1;
            assert!(matches!(
                inventory::submit(
                    harness.server.ecs.world_mut(),
                    harness.player,
                    inventory::InventoryEffect {
                        id: "sample:hold".into(),
                        expected,
                        replacement
                    }
                )
                .unwrap(),
                inventory::Submission::Queued
            ));
        }
        harness.server.despawn_player(42);
        if mode == "dead" {
            harness
                .server
                .ecs
                .world_mut()
                .get_mut::<Health>(harness.player)
                .unwrap()
                .current = 0.0;
        }
        if mode == "incomplete" {
            harness
                .server
                .ecs
                .world_mut()
                .entity_mut(harness.player)
                .remove::<Health>();
        }
        let (router, manager, player) = tasks(harness);
        assert!(
            router
                .world(0)
                .unwrap()
                .ecs
                .world()
                .get::<Effects>(player)
                .is_none_or(|effects| !effects.has(EffectType::RESISTANCE)),
            "{mode}"
        );
        let manager = manager.lock().unwrap();
        let logs = &manager
            .plugins
            .values()
            .next()
            .unwrap()
            .store
            .data()
            .test_logs;
        assert!(
            logs.iter()
                .any(|log| log == "retained-unavailable" || log == "stale-retained"),
            "{mode}: {logs:?}"
        );
        assert!(manager.readiness.check().is_ok(), "{mode}");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retained_lineage_changes_between_staging_and_apply_cannot_retarget_a_new_actor() {
    let mut harness = Harness::extended(true);
    harness.command("probe remember");
    harness.server.despawn_player(42);
    let player = harness.player;
    let manager = harness.manager;
    let mut router = crate::server::worlds::DimensionRouter::new(harness.server);
    router.set_plugin_manager(manager.clone());
    manager.lock().unwrap().drain_scheduled_tasks(&mut router);
    let world = router.world_mut(0).unwrap().ecs.world_mut();
    world
        .entity_mut(player)
        .insert(PlayerUuid(uuid::Uuid::from_u128(99)));
    players::apply_mechanics(world);
    assert!(
        !world
            .get::<Effects>(player)
            .unwrap()
            .has(EffectType::RESISTANCE)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_retained_source_and_infinite_lifetime_reach_native_authorization_without_a_proxy() {
    use crate::entity::projectile::hooks::{EffectApplication, ProjectileHandler, ProjectileHooks};
    type ObservedEffects = Vec<(Entity, Option<Entity>, bool)>;
    struct Seen(Arc<Mutex<ObservedEffects>>);
    impl ProjectileHandler for Seen {
        fn on_effect(&self, _: &mut World, effect: &mut EffectApplication) {
            self.0
                .lock()
                .unwrap()
                .push((effect.target(), effect.source(), effect.is_infinite()));
        }
    }
    let mut harness = Harness::extended(true);
    harness.command("probe remember");
    let source = harness.player;
    let (outbound_tx, _receiver) = tokio::sync::mpsc::channel(256);
    let destination = harness
        .server
        .spawn_player(PlayerSpawnData {
            session_id: 84,
            runtime_id: 84,
            display_name: "Destination".into(),
            xuid: Some("84".into()),
            uuid: Some(uuid::Uuid::from_u128(84).to_string()),
            position: glam::DVec3::new(2.5, 17.62, 0.5),
            outbound_tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    assert!(commands::dispatch(
        harness.server.ecs.world_mut(),
        destination,
        "probe destination",
        None
    ));
    let seen = Arc::new(Mutex::new(Vec::new()));
    harness
        .server
        .ecs
        .world_mut()
        .init_resource::<ProjectileHooks>();
    harness
        .server
        .ecs
        .world_mut()
        .resource_mut::<ProjectileHooks>()
        .register(Seen(seen.clone()));
    harness.server.despawn_player(42);
    let (router, _, retained) = tasks(harness);
    assert_eq!(retained, source);
    assert!(
        seen.lock()
            .unwrap()
            .contains(&(destination, Some(source), true))
    );
    let world = router.world(0).unwrap().ecs.world();
    assert!(
        world
            .get::<Effects>(destination)
            .unwrap()
            .get(EffectType::SPEED)
            .unwrap()
            .infinite
    );
    assert!(world.get::<PlayerSession>(source).is_none());
    assert!(world.get::<player_events::DetachedPlayer>(source).is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn infinite_native_veto_keeps_effect_absent_after_successful_guest_staging() {
    use crate::entity::projectile::hooks::{EffectApplication, ProjectileHandler, ProjectileHooks};
    struct Veto;
    impl ProjectileHandler for Veto {
        fn on_effect(&self, _: &mut World, effect: &mut EffectApplication) {
            assert!(effect.is_infinite());
            effect.cancel();
        }
    }
    let mut harness = Harness::extended(true);
    harness
        .server
        .ecs
        .world_mut()
        .init_resource::<ProjectileHooks>();
    harness
        .server
        .ecs
        .world_mut()
        .resource_mut::<ProjectileHooks>()
        .register(Veto);
    harness.command("probe infinite");
    harness.apply();
    assert!(
        harness
            .logs()
            .iter()
            .any(|message| message == "infinite-staged")
    );
    assert!(
        !harness
            .server
            .ecs
            .world()
            .get::<Effects>(harness.player)
            .unwrap()
            .has(EffectType::SPEED)
    );
}
