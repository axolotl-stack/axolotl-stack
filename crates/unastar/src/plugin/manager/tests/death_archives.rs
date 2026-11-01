//! Real component archives capture native death before drops, without retaining a live player.

use super::*;
use crate::ecs::events::EventKind;
use crate::entity::components::*;
use crate::entity::mob::combat::Hit;
use crate::entity::mob::components::KnockbackPower;
use crate::entity::player_life::hooks::{
    PlayerDeath, PlayerLifecycleHandler, PlayerLifecycleHooks,
};
use crate::plugin::{archive_store, manifest::PluginCapability};
use crate::server::game::{
    GameServer, PlayerSpawnData, ServerConfig, player_state::PlayerSnapshot,
};
use crate::server::worlds::DimensionRouter;
use bevy_ecs::prelude::*;
use glam::DVec3;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use unastar_data::actor_components::body::ActorDamageCause;
use wasmtime::component::Component;

/// Compile the owning SDK component separately from the native Cargo artifact lock.
fn guest() -> &'static PathBuf {
    static GUEST: OnceLock<PathBuf> = OnceLock::new();
    GUEST.get_or_init(|| {
        let package = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| package.join("../../target"))
            .join("guest-death-archive-fixture");
        let output =
            std::process::Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
                .args([
                    "build",
                    "--locked",
                    "--release",
                    "--target",
                    "wasm32-wasip2",
                    "--manifest-path",
                ])
                .arg(package.join("tests/fixtures/death-archive-component/Cargo.toml"))
                .env("CARGO_TARGET_DIR", &target)
                .output()
                .unwrap();
        assert!(
            output.status.success(),
            "death archive fixture build failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        target.join("wasm32-wasip2/release/death_archive_component_fixture.wasm")
    })
}

#[derive(Resource, Default)]
struct Observed {
    snapshot: Option<Arc<PlayerSnapshot>>,
    keep: bool,
}

/// Earlier native handlers verify committed drop ordering and may remove the source entirely.
struct BeforeBridge {
    remove: bool,
}
impl PlayerLifecycleHandler for BeforeBridge {
    fn on_death(&self, world: &mut World, event: &PlayerDeath) {
        let raw = event
            .state
            .as_ref()
            .ok()
            .map(|capture| capture.snapshot.clone());
        if let Some(raw) = &raw {
            assert!(raw.dead);
            assert_eq!(raw.data.health, 0.0);
            assert_eq!(raw.data.inventory[0].count, 7);
            assert_eq!(raw.data.experience, 8);
            assert_eq!(
                world.get::<MainInventory>(event.player).unwrap().0.slots()[0].count,
                if event.keep_inventory { 7 } else { 0 }
            );
            assert_eq!(
                world.get::<Experience>(event.player).unwrap().level,
                if event.keep_inventory { 8 } else { 0 }
            );
            if !event.keep_inventory {
                assert!(
                    world
                        .query_filtered::<&ItemEntityStack, With<DroppedItem>>()
                        .iter(world)
                        .next()
                        .is_some()
                );
            }
        }
        world.insert_resource(Observed {
            snapshot: raw,
            keep: event.keep_inventory,
        });
        if self.remove {
            world
                .resource_mut::<crate::server::game::SessionEntityMap>()
                .remove(42);
            world.despawn(event.player);
            if let Some(attacker) = event.cause.attacker {
                world.despawn(attacker);
            }
        }
    }
}

struct Harness {
    router: DimensionRouter,
    manager: Arc<Mutex<PluginManager>>,
    player: Option<Entity>,
    dimension: i32,
    _packets: Vec<tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>>,
}
impl Harness {
    /// Start an ordinary required component with a real native world and optional actual actor.
    fn new(directory: &Path, mode: u8, remove: bool, spawn: bool) -> Self {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        let world = server.ecs.world_mut();
        world.insert_resource(crate::world::WorldIdentity {
            id: uuid::Uuid::from_u128(9),
            name: "example-overworld".into(),
            dimension: 0,
        });
        world.insert_resource(crate::entity::EntityRandom::seeded(17));
        world.init_resource::<Observed>();
        world
            .resource_mut::<crate::world::rules::GameRules>()
            .show_death_messages = false;
        world
            .resource_mut::<crate::world::rules::GameRules>()
            .do_mob_spawning = false;
        world
            .resource_mut::<PlayerLifecycleHooks>()
            .register(BeforeBridge { remove });
        let mut column = crate::world::chunk::Chunk::new(0, 0);
        for x in 0..16 {
            for z in 0..16 {
                column.set_block(x, 15, z, *crate::world::chunk::blocks::STONE);
            }
        }
        let position = crate::world::ecs::ChunkPosition::new(0, 0);
        let chunk = world
            .spawn((crate::world::ecs::ChunkData::new(column), position))
            .id();
        world
            .resource_mut::<crate::world::ChunkManager>()
            .insert(position, chunk);
        let mut router = DimensionRouter::new(server);
        let engine = engine();
        let limits = PluginLimits {
            max_owned_resources: if mode == 3 { 96 } else { 4096 },
            callback_timeout_ms: 1000,
            ..Default::default()
        };
        let mut context = store(&engine, &limits);
        context.data_mut().configuration = vec![mode];
        let mut capabilities = std::collections::HashSet::from([
            PluginCapability::ReadPlayers,
            PluginCapability::ReadInventory,
            PluginCapability::ItemValues,
        ]);
        if mode != 4 {
            capabilities.insert(PluginCapability::PlayerStateArchives);
        }
        let manifest = PluginManifest {
            id: PluginId(
                if mode == 13 {
                    "example.other"
                } else {
                    "example.death-archive"
                }
                .into(),
            ),
            name: "Death archive".into(),
            version: "1".into(),
            api_version: PLUGIN_API_VERSION.into(),
            required: true,
            config: None,
            capabilities,
            subscriptions: [EventKind::PlayerDeath, EventKind::Tick].into(),
            limits,
        };
        let pending = PendingPlugin {
            component: Component::from_file(&engine, guest()).unwrap(),
            store: context,
            manifest,
        };
        let manager = Arc::new(Mutex::new(PluginManager {
            engine,
            pending: Vec::new(),
            plugins: Default::default(),
            tick_prepared: false,
            tick_budget_valid: true,
            readiness: Default::default(),
            storage_registry: Default::default(),
            archive_registry: Default::default(),
            command_roots: Default::default(),
            ui_registry: Default::default(),
            view_registry: Default::default(),
        }));
        {
            let mut manager = manager.lock().unwrap();
            manager
                .configure_archives(&crate::config::PluginArchiveConfig {
                    directory: directory.join("archives"),
                    max_value_bytes: if mode == 12 { 128 } else { 1024 * 1024 },
                    ..Default::default()
                })
                .unwrap();
            let mut pending = pending;
            pending.store.data_mut().archive_registry = manager.archive_registry.clone();
            manager.pending.push(pending);
        }
        PluginManager::attach_to_world(
            manager.clone(),
            router.world_mut(0).unwrap().ecs.world_mut(),
        );
        router.set_plugin_manager(manager.clone());
        router.initialize_plugins().unwrap();
        let mut result = Self {
            router,
            manager,
            player: None,
            dimension: 0,
            _packets: Vec::new(),
        };
        if spawn {
            let actor = result.spawn(42);
            result.player = Some(actor);
            result.prepare(actor);
        }
        result
    }
    /// Spawn a genuine distinct account and transport, not an actor reconstructed from archive data.
    fn spawn(&mut self, session: u64) -> Entity {
        let (outbound_tx, packets) = tokio::sync::mpsc::channel(8192);
        let actor = self
            .router
            .world_mut(self.dimension)
            .unwrap()
            .spawn_player(PlayerSpawnData {
                session_id: session,
                runtime_id: session as i64,
                display_name: format!("Actor{session}"),
                xuid: Some(session.to_string()),
                uuid: Some(uuid::Uuid::from_u128(session.into()).to_string()),
                position: DVec3::new(5.5, 17.62, 4.5),
                outbound_tx,
                saved_data: None,
                chunk_radius: 0,
            })
            .unwrap();
        self._packets.push(packets);
        actor
    }
    /// Populate actual inventories and raw life fields, including metadata invisible to projections.
    fn prepare(&mut self, actor: Entity) {
        let mut tag = zuri_nbt::tag::Compound::default();
        let mut display = zuri_nbt::tag::Compound::default();
        display.0.insert(
            "Name".into(),
            zuri_nbt::NBTTag::String(zuri_nbt::tag::String("Observed".into())),
        );
        display.0.insert(
            "Lore".into(),
            zuri_nbt::NBTTag::List(zuri_nbt::tag::List(vec![zuri_nbt::NBTTag::String(
                zuri_nbt::tag::String("Original line".into()),
            )])),
        );
        tag.0
            .insert("display".into(), zuri_nbt::NBTTag::Compound(display));
        tag.0.insert(
            "custom".into(),
            zuri_nbt::NBTTag::Long(zuri_nbt::tag::Long(908)),
        );
        let sword = crate::item::enchant::with_enchantment_ids(
            &crate::item::enchant::with_item_tag(
                &crate::item::test_support::stack("minecraft:diamond_sword", 1),
                tag,
            ),
            [(9, 3)],
        );
        let world = self
            .router
            .world_mut(self.dimension)
            .unwrap()
            .ecs
            .world_mut();
        world.resource_mut::<crate::ecs::TickCounter>().current = 1000;
        world
            .get_mut::<MainInventory>(actor)
            .unwrap()
            .0
            .set_item(0, crate::item::test_support::stack("minecraft:stone", 7))
            .unwrap();
        world
            .get_mut::<MainInventory>(actor)
            .unwrap()
            .0
            .set_item(5, sword)
            .unwrap();
        world.get_mut::<HeldSlot>(actor).unwrap().0 = 5;
        world
            .get_mut::<ArmourInventory>(actor)
            .unwrap()
            .set_helmet(crate::item::test_support::stack(
                "minecraft:diamond_helmet",
                1,
            ))
            .unwrap();
        world
            .entity_mut(actor)
            .insert(OffhandSlot(crate::item::test_support::stack(
                "minecraft:shield",
                1,
            )));
        world
            .get_mut::<PlayerUi>(actor)
            .unwrap()
            .0
            .set_item(2, crate::item::test_support::stack("minecraft:paper", 4))
            .unwrap();
        world
            .get_mut::<EnderChestInventory>(actor)
            .unwrap()
            .0
            .set_item(4, crate::item::test_support::stack("minecraft:diamond", 23))
            .unwrap();
        world.entity_mut(actor).insert((
            Hunger {
                food_level: 9,
                saturation: 2.0,
                exhaustion: 1.0,
            },
            Experience {
                level: 8,
                progress: 0.25,
            },
            Age(101),
            AirSupply {
                current_ticks: 17,
                max_ticks: 300,
            },
            OnFire {
                ticks_remaining: 40,
            },
            Velocity(DVec3::new(0.25, 0.5, -0.25)),
        ));
        world.get_mut::<Effects>(actor).unwrap().active.insert(
            EffectType(1),
            ActiveEffect {
                level: 2,
                duration_ticks: 93,
                ambient: true,
                show_particles: false,
                infinite: false,
            },
        );
        world
            .entity_mut(actor)
            .insert(crate::entity::freezing::FreezingState::from_strength(0.3).unwrap());
        crate::command::exec::set_mode_override(world, actor, Some(GameMode::Creative));
        crate::server::game::flight::set_override(world, actor, Some(true));
    }
    /// Use ordinary lethal native damage, including costs, drops and committed callbacks.
    fn kill(&mut self, attacker: Option<Entity>) {
        let player = self.player.unwrap();
        let world = self
            .router
            .world_mut(self.dimension)
            .unwrap()
            .ecs
            .world_mut();
        assert!(
            crate::entity::player_life::hurt::hurt_player(
                world,
                player,
                &Hit {
                    damage: 100.0,
                    cause: ActorDamageCause::Override,
                    attacker,
                    by_player: attacker.is_some(),
                    knockback_from: None,
                    knockback_power: KnockbackPower::DEFAULT
                }
            ) > 0.0
        );
    }
    /// Read actual guest observations without dispatching another callback.
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
    /// Read the module-owned native archive through the same validating backend used on restart.
    fn loaded(&self) -> Option<archive_store::Loaded> {
        let manager = self.manager.lock().unwrap();
        let plugin = manager.plugins.values().next().unwrap();
        let namespace = plugin.store.data().archive_namespace.as_ref()?.clone();
        let mut namespace = namespace.lock().unwrap();
        let reference = namespace.lookup("committed-death").unwrap()?;
        Some(namespace.read(&reference).unwrap())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_death_archive_preserves_exact_pre_drop_raw_state_and_native_metadata() {
    let root = tempfile::tempdir().unwrap();
    let mut h = Harness::new(root.path(), 0, false, true);
    h.kill(None);
    assert!(h.logs().iter().any(|line| line == "archived"));
    let loaded = h.loaded().unwrap();
    let world = h.router.world(0).unwrap().ecs.world();
    assert_eq!(
        &loaded.record.state,
        world
            .resource::<Observed>()
            .snapshot
            .as_ref()
            .unwrap()
            .as_ref()
    );
    assert!(loaded.record.death.is_some());
    assert_eq!(
        loaded.record.death.as_ref().unwrap().keep_inventory,
        world.resource::<Observed>().keep
    );
    assert_eq!(loaded.record.state.data.experience, 8);
    assert_eq!(world.get::<Experience>(h.player.unwrap()).unwrap().level, 0);
    assert!(
        world
            .get::<MainInventory>(h.player.unwrap())
            .unwrap()
            .0
            .slots()[5]
            .is_empty()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn early_source_removal_keeps_owned_death_and_authenticated_attribution_archivable() {
    let root = tempfile::tempdir().unwrap();
    let mut h = Harness::new(root.path(), 0, true, true);
    let attacker = h.spawn(43);
    h.kill(Some(attacker));
    assert!(
        h.router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get_entity(h.player.unwrap())
            .is_err()
    );
    assert!(
        h.router
            .world(0)
            .unwrap()
            .ecs
            .world()
            .get_entity(attacker)
            .is_err()
    );
    let loaded = h.loaded().unwrap();
    assert_eq!(
        loaded
            .record
            .death
            .as_ref()
            .unwrap()
            .attribution
            .owner
            .as_ref()
            .unwrap()
            .authenticated
            .as_ref()
            .unwrap()
            .xuid,
        43
    );
    assert_eq!(loaded.record.owner.authenticated_xuid, Some(42));
    assert!(h.logs().iter().any(|line| line == "archived"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owned_death_capture_can_archive_from_later_global_tick_after_actual_departure() {
    let root = tempfile::tempdir().unwrap();
    let mut h = Harness::new(root.path(), 1, false, true);
    h.kill(None);
    assert!(h.loaded().is_none());
    h.router.despawn_player(42);
    h.router.tick().await.unwrap();
    assert!(h.logs().iter().any(|line| line == "archived-after-removal"));
    assert!(h.loaded().is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn immutable_death_ack_survives_callback_trap_and_reopens_without_any_live_player() {
    for mode in [0, 2] {
        let root = tempfile::tempdir().unwrap();
        let mut h = Harness::new(root.path(), mode, true, true);
        h.kill(None);
        assert!(h.loaded().is_some());
        drop(h);
        let reopened = Harness::new(root.path(), 10, false, false);
        assert!(reopened.logs().iter().any(|line| line == "reopened"));
        assert!(
            reopened
                .router
                .world(0)
                .unwrap()
                .ecs
                .world()
                .try_query::<&Player>()
                .is_none_or(|mut q| q
                    .iter(reopened.router.world(0).unwrap().ecs.world())
                    .next()
                    .is_none())
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn death_archive_observes_keep_inventory_and_immediate_respawn_without_normalizing_history() {
    for keep in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut h = Harness::new(root.path(), 0, false, true);
        {
            let world = h.router.world_mut(0).unwrap().ecs.world_mut();
            let mut rules = world.resource_mut::<crate::world::rules::GameRules>();
            rules.keep_inventory = keep;
            rules.do_immediate_respawn = true;
        }
        h.kill(None);
        let loaded = h.loaded().unwrap();
        assert!(loaded.record.state.dead);
        assert_eq!(loaded.record.state.data.health, 0.0);
        assert_eq!(loaded.record.death.as_ref().unwrap().keep_inventory, keep);
        assert!(
            h.router
                .world(0)
                .unwrap()
                .ecs
                .world()
                .get::<Health>(h.player.unwrap())
                .unwrap()
                .current
                > 0.0
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn incomplete_or_over_limit_death_capture_stays_typed_and_never_creates_an_archive() {
    for mode in [3, 4, 5, 6, 7, 12, 14] {
        let root = tempfile::tempdir().unwrap();
        let mut h = Harness::new(root.path(), mode, false, true);
        let actor = h.player.unwrap();
        let world = h.router.world_mut(0).unwrap().ecs.world_mut();
        match mode {
            5 => {
                world.entity_mut(actor).remove::<EnderChestInventory>();
            }
            6 => {
                let mut stack = crate::item::test_support::stack("minecraft:diamond", 1);
                stack.nbt = Some(vec![0; 16 * 1024 * 1024].into());
                world
                    .get_mut::<EnderChestInventory>(actor)
                    .unwrap()
                    .0
                    .set_item(0, stack)
                    .unwrap();
            }
            7 => {
                world.remove_resource::<crate::world::WorldIdentity>();
            }
            14 => {
                world.entity_mut(actor).insert(AirSupply {
                    current_ticks: 301,
                    max_ticks: 300,
                });
            }
            _ => (),
        }
        h.kill(None);
        assert!(h.loaded().is_none());
        assert!(
            h.logs().iter().any(|line| line
                == match mode {
                    3 | 6 => "limited",
                    4 => "denied",
                    12 => "quota",
                    14 => "malformed",
                    _ => "unavailable",
                }),
            "mode{mode}: {:?}",
            h.logs()
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_module_cannot_discover_or_read_the_committed_death_namespace() {
    let root = tempfile::tempdir().unwrap();
    let mut h = Harness::new(root.path(), 0, false, true);
    h.kill(None);
    assert!(h.loaded().is_some());
    drop(h);
    let other = Harness::new(root.path(), 13, false, false);
    assert!(other.logs().iter().any(|line| line == "isolated"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn uncertain_death_archive_installation_reconciles_the_same_genuine_capture_id() {
    let root = tempfile::tempdir().unwrap();
    let mut h = Harness::new(root.path(), 16, true, true);
    {
        let mut manager = h.manager.lock().unwrap();
        let namespace = manager
            .archive_registry
            .lock()
            .unwrap()
            .namespace("example.death-archive")
            .unwrap();
        namespace
            .lock()
            .unwrap()
            .inject_fault(crate::plugin::storage::Fault::AfterRename);
        manager
            .plugins
            .values_mut()
            .next()
            .unwrap()
            .store
            .data_mut()
            .archive_namespace = Some(namespace);
    }
    h.kill(None);
    assert!(h.logs().iter().any(|line| line == "reconciled"));
    let original_id = h.loaded().unwrap().record.death.unwrap().capture_id;
    drop(h);
    let reopened = Harness::new(root.path(), 10, false, false);
    assert_eq!(
        reopened.loaded().unwrap().record.death.unwrap().capture_id,
        original_id
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn archive_demand_follows_the_shared_component_into_secondary_worlds_and_expires_on_unload() {
    let root = tempfile::tempdir().unwrap();
    let mut h = Harness::new(root.path(), 0, false, false);
    let mut config = crate::world::WorldConfig::void_spawn_platform(1);
    config.dimension = 1;
    let mut realm = GameServer::with_config(ServerConfig {
        world: config,
        ..Default::default()
    });
    let world = realm.ecs.world_mut();
    world.insert_resource(crate::world::WorldIdentity {
        id: uuid::Uuid::from_u128(10),
        name: "example-secondary".into(),
        dimension: 1,
    });
    world.init_resource::<Observed>();
    world
        .resource_mut::<crate::world::rules::GameRules>()
        .show_death_messages = false;
    world
        .resource_mut::<crate::world::rules::GameRules>()
        .do_mob_spawning = false;
    world
        .resource_mut::<PlayerLifecycleHooks>()
        .register(BeforeBridge { remove: false });
    h.router.add(realm).unwrap();
    h.dimension = 1;
    let actor = h.spawn(42);
    h.player = Some(actor);
    h.prepare(actor);
    h.kill(None);
    let loaded = h.loaded().unwrap();
    assert_eq!(loaded.record.world.dimension, 1);
    assert_eq!(loaded.record.state.data.dimension, 1);
    assert_eq!(
        loaded.record.world.uuid,
        uuid::Uuid::from_u128(10).to_string()
    );
    assert!(h.logs().iter().any(|line| line == "archived"));
    h.manager.lock().unwrap().plugins.clear();
    let other = h.spawn(43);
    h.player = Some(other);
    h.prepare(other);
    h.kill(None);
    assert!(
        h.router
            .world(1)
            .unwrap()
            .ecs
            .world()
            .resource::<Observed>()
            .snapshot
            .is_none(),
        "expired subscriber cannot request native full-state copies"
    );
}
