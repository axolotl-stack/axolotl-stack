//! Actual SDK guards precede writes and movement, including native-chosen safe positions.

use super::*;
use crate::entity::components::Health;
use crate::server::game::travel_authorization::{Authorizer, Authorizers};
use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_real_component_observes_the_destination_without_an_owned_request_echo() {
    for destination in [0, 1] {
        let mut h = Harness::guarded(0, destination);
        {
            let mut manager = h.manager.lock().unwrap();
            let manifest = PluginManifest {
                id: PluginId("example.observer".into()),
                name: "Observer".into(),
                version: "1".into(),
                api_version: PLUGIN_API_VERSION.into(),
                required: false,
                config: None,
                capabilities: [PluginCapability::ReadPlayers].into(),
                subscriptions: [EventKind::PlayerTravel].into(),
                limits: PluginLimits::default(),
            };
            let mut store = store(&manager.engine, &manifest.limits);
            store.data_mut().configuration = vec![6, destination];
            let component = wasmtime::component::Component::new(&manager.engine, guest()).unwrap();
            manager.pending.push(PendingPlugin {
                component,
                store,
                manifest,
            });
        }
        h.router.tick().await.unwrap();
        h.router.tick().await.unwrap();
        h.queue();
        h.guarded_finish().await;
        assert_eq!(h.logs(), ["guard", "result:moved"]);
        let manager = h.manager.lock().unwrap();
        assert_eq!(
            manager.plugins[&PluginId("example.observer".into())]
                .store
                .data()
                .test_logs,
            ["guard"]
        );
        assert_eq!(h.provider.writes.load(Ordering::SeqCst), 1);
    }
}

/// Compile the real scoped Handler independently of the native artifact directory.
fn guest() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let manifest =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/travel-guard/Cargo.toml");
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
            "travel guard component: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::fs::read(
            target
                .path()
                .join("wasm32-wasip2/debug/travel_guard_fixture.wasm"),
        )
        .unwrap()
    })
}

impl Harness {
    /// Keep the ordinary loader, real native actors and storage paths for the Rust component.
    fn guarded(mode: u8, destination: u8) -> Self {
        Self::component_factory(true, true, mode == 4, &[mode, destination], |engine| {
            wasmtime::component::Component::new(engine, guest()).unwrap()
        })
    }
    /// A proposal log is not completion; wait for the original request's terminal result.
    async fn guarded_finish(&mut self) {
        for _ in 0..12 {
            self.router.tick().await.unwrap();
            if self.logs().iter().any(|line| line.starts_with("result:")) {
                return;
            }
        }
        panic!("guarded native operation did not complete");
    }
    /// Set real loaded support so the native safe-position search must use a different height.
    fn elevated_support(&mut self, dimension: i32, x: u8) {
        let world = self.router.world_mut(dimension).unwrap().ecs.world_mut();
        let position = crate::world::ecs::ChunkPosition::new(0, 0);
        let chunk = world
            .resource::<crate::world::ChunkManager>()
            .get(&position)
            .unwrap();
        world
            .get_mut::<crate::world::ecs::ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block(x, 64, 8, *crate::world::chunk::blocks::STONE);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_selected_destinations_are_authorized_before_same_and_cross_world_writes() {
    for destination in [0, 1] {
        let mut h = Harness::guarded(0, destination);
        h.queue();
        h.guarded_finish().await;
        assert_eq!(h.logs(), ["guard", "result:moved"]);
        assert_eq!(h.provider.writes.load(Ordering::SeqCst), 1);
        let world = h
            .router
            .world_mut(i32::from(destination))
            .unwrap()
            .ecs
            .world_mut();
        let positions = world
            .query::<&Position>()
            .iter(world)
            .map(|position| position.0)
            .collect::<Vec<_>>();
        assert!(positions.contains(&DVec3::new(8.5, 65.62, 8.5)));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_same_and_cross_world_vetoes_do_not_persist_or_move() {
    for destination in [0, 1] {
        let mut h = Harness::guarded(1, destination);
        let original = h.world().get::<Position>(h.player).unwrap().0;
        h.queue();
        h.guarded_finish().await;
        assert_eq!(h.logs(), ["guard", "result:rejected"]);
        assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
        assert_eq!(h.world().get::<Position>(h.player).unwrap().0, original);
        assert!(h.world().get::<Health>(h.player).unwrap().current > 0.0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn final_guard_sees_the_real_chosen_safe_y_before_any_persistence() {
    for destination in [0, 1] {
        let mut h = Harness::guarded(2, destination);
        h.elevated_support(i32::from(destination), 8);
        h.queue();
        h.guarded_finish().await;
        assert_eq!(h.logs(), ["guard", "result:rejected"]);
        assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
        assert_eq!(
            h.world().get::<Position>(h.player).unwrap().0,
            DVec3::new(4.5, 65.62, 4.5)
        );
    }
}

/// Ordinary native handlers are allowed to select a new destination before final authorization.
struct NativeRewrite;
impl crate::entity::projectile::hooks::ProjectileHandler for NativeRewrite {
    fn on_teleport(
        &self,
        _: &mut World,
        event: &mut crate::entity::projectile::hooks::PlayerTeleport,
    ) {
        event.destination = DVec3::new(9.5, 65.0, 8.5);
    }
}
impl crate::server::worlds::WorldTransferHandler for NativeRewrite {
    fn on_transfer(
        &self,
        _: &mut World,
        _: &mut World,
        event: &mut crate::server::worlds::PlayerWorldTransfer,
    ) {
        event.feet = DVec3::new(9.5, 65.0, 8.5);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ordinary_native_redirects_happen_before_the_component_destination_guard() {
    for destination in [0, 1] {
        let mut h = Harness::guarded(2, destination);
        h.elevated_support(i32::from(destination), 9);
        h.world_mut()
            .get_resource_or_init::<crate::entity::projectile::hooks::ProjectileHooks>()
            .register(NativeRewrite);
        h.world_mut()
            .get_resource_or_init::<crate::server::worlds::WorldTransferHooks>()
            .register(NativeRewrite);
        h.queue();
        h.guarded_finish().await;
        assert_eq!(h.logs(), ["guard", "result:rejected"]);
        assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
    }
}

/// A later internal mutation must not inherit authorization of the original selected position.
struct LateRewrite;
impl Authorizer for LateRewrite {
    fn teleport(
        &self,
        _: &mut World,
        event: &mut crate::entity::projectile::hooks::PlayerTeleport,
    ) {
        event.destination.x += 1.0;
    }
    fn transfer(
        &self,
        _: &mut World,
        _: &mut World,
        event: &mut crate::server::worlds::PlayerWorldTransfer,
    ) {
        event.feet.x += 1.0;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_later_internal_rewrite_invalidates_the_recorded_exact_approval() {
    for destination in [0, 1] {
        let mut h = Harness::guarded(0, destination);
        h.world_mut()
            .get_resource_or_init::<Authorizers>()
            .register(LateRewrite);
        h.queue();
        h.guarded_finish().await;
        assert_eq!(h.logs(), ["guard", "result:rejected"]);
        assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn optional_and_required_guard_traps_reject_before_any_native_write() {
    for mode in [3, 4] {
        let mut h = Harness::guarded(mode, 0);
        h.queue();
        let mut failed = false;
        for _ in 0..3 {
            failed |= h.router.tick().await.is_err();
        }
        assert_eq!(failed, mode == 4);
        assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
        if mode == 4 {
            assert!(crate::server::game::inventory_effects::is_recovery_fenced(
                h.world(),
                h.player
            ));
        } else {
            assert_eq!(
                h.world().get::<Position>(h.player).unwrap().0,
                DVec3::new(4.5, 65.62, 4.5)
            );
        }
    }
}

/// Drop the actual originating instance after its callback, without resetting readiness.
struct DropOwner(Arc<Mutex<PluginManager>>);
impl Authorizer for DropOwner {
    fn teleport(&self, _: &mut World, _: &mut crate::entity::projectile::hooks::PlayerTeleport) {
        self.0.lock().unwrap().plugins.clear();
    }
    fn transfer(
        &self,
        _: &mut World,
        _: &mut World,
        _: &mut crate::server::worlds::PlayerWorldTransfer,
    ) {
        self.0.lock().unwrap().plugins.clear();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unloaded_component_cannot_leave_its_destination_approval_authorized() {
    for destination in [0, 1] {
        let mut h = Harness::guarded(0, destination);
        let owner = h.manager.clone();
        h.world_mut()
            .get_resource_or_init::<Authorizers>()
            .register(DropOwner(owner));
        h.queue();
        for _ in 0..3 {
            h.router.tick().await.unwrap();
        }
        assert!(h.manager.lock().unwrap().plugins.is_empty());
        assert_eq!(h.provider.writes.load(Ordering::SeqCst), 0);
        assert_eq!(
            h.world().get::<Position>(h.player).unwrap().0,
            DVec3::new(4.5, 65.62, 4.5)
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_native_pearl_collision_uses_the_same_final_guard_and_pays_fall_damage_only_if_moved()
{
    use crate::entity::components::Velocity;
    use crate::entity::projectile::{self, LaunchSpec};
    for denied in [false, true] {
        let mut h = Harness::guarded(if denied { 7 } else { 6 }, 0);
        let player = h.player;
        let (actor, climate) = projectile::thrown_by("minecraft:ender_pearl").unwrap();
        let pearl = projectile::launch_with(
            h.world_mut(),
            player,
            actor,
            climate,
            LaunchSpec {
                throw_sound: false,
                ..LaunchSpec::default()
            },
        )
        .unwrap();
        h.world_mut()
            .entity_mut(pearl)
            .insert((Position(DVec3::new(8.5, 64.5, 8.5)), Velocity(DVec3::NEG_Y)));
        let mut schedule = bevy_ecs::schedule::Schedule::default();
        schedule.add_systems(projectile::tick_projectiles);
        schedule.run(h.world_mut());
        assert_eq!(h.logs(), ["guard"]);
        assert_eq!(
            h.provider.writes.load(Ordering::SeqCst),
            0,
            "an ordinary pearl is not a durable relocation request"
        );
        let position = h.world().get::<Position>(h.player).unwrap().0;
        let health = h.world().get::<Health>(h.player).unwrap().current;
        if denied {
            assert_eq!(position, DVec3::new(4.5, 65.62, 4.5));
            assert_eq!(health, 20.0);
        } else {
            assert!(position.distance(DVec3::new(8.5, 65.62, 8.5)) < 0.01);
            assert_eq!(health, 15.0);
        }
        assert!(
            h.world().get_entity(pearl).is_err(),
            "the native impact still consumes the projectile"
        );
    }
}
