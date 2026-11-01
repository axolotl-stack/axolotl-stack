//! Real components observe committed deaths and owned pre-drop slots across actor removal.

use super::*;
use crate::ecs::events::EventKind;
use crate::entity::components::{
    ArmourInventory, Health, HeldSlot, MainInventory, OffhandSlot, PlayerUi, Position, Velocity,
};
use crate::entity::mob::combat::Hit;
use crate::entity::mob::components::KnockbackPower;
use crate::entity::player_life::hooks::{
    PlayerDeath, PlayerLifecycleHandler, PlayerLifecycleHooks,
};
use crate::plugin::manifest::PluginCapability;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use crate::server::worlds::DimensionRouter;
use bevy_ecs::prelude::*;
use glam::DVec3;
use std::sync::{Arc, Mutex};
use unastar_data::actor_components::body::ActorDamageCause;

struct Harness {
    router: DimensionRouter,
    manager: Arc<Mutex<PluginManager>>,
    player: Entity,
    original: crate::item::ItemStack,
    _packets: Vec<tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>>,
}
impl Harness {
    /// Initialize a real component and ordinary player inventory before lethal native damage.
    fn new(bodies: &[(&str, String)], maximum: u32, items: bool, remove_first: bool) -> Self {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        let world = server.ecs.world_mut();
        world.insert_resource(crate::entity::EntityRandom::seeded(17));
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
        world
            .resource_mut::<crate::world::rules::GameRules>()
            .show_death_messages = false;
        world
            .resource_mut::<crate::world::rules::GameRules>()
            .do_mob_spawning = false;
        if remove_first {
            world
                .resource_mut::<PlayerLifecycleHooks>()
                .register(RemoveBeforeBridge);
        }
        let mut router = DimensionRouter::new(server);
        let engine = engine();
        let mut capabilities = std::collections::HashSet::from([
            PluginCapability::ReadPlayers,
            PluginCapability::ScheduleTasks,
        ]);
        if items {
            capabilities.insert(PluginCapability::ItemValues);
        }
        let manifest = PluginManifest {
            id: PluginId("example.death".into()),
            name: "Death observations".into(),
            version: "1".into(),
            api_version: PLUGIN_API_VERSION.into(),
            required: true,
            config: None,
            capabilities,
            subscriptions: [EventKind::PlayerDeath].into(),
            limits: PluginLimits {
                max_owned_resources: maximum,
                ..Default::default()
            },
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
            command_roots: Default::default(),
            ui_registry: Default::default(),
            view_registry: Default::default(),
        }));
        PluginManager::attach_to_world(
            manager.clone(),
            router.world_mut(0).unwrap().ecs.world_mut(),
        );
        router.set_plugin_manager(manager.clone());
        router.initialize_plugins().unwrap();
        let (player, packets) = Self::spawn(&mut router, 42, DVec3::new(5.5, 17.62, 4.5));
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
        let original = crate::item::enchant::with_enchantment_ids(
            &crate::item::enchant::with_item_tag(
                &crate::item::test_support::stack("minecraft:diamond_sword", 1),
                tag,
            ),
            [(9, 3)],
        );
        let world = router.world_mut(0).unwrap().ecs.world_mut();
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, crate::item::test_support::stack("minecraft:stone", 7))
            .unwrap();
        world
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(5, original.clone())
            .unwrap();
        world.get_mut::<HeldSlot>(player).unwrap().0 = 5;
        world
            .get_mut::<ArmourInventory>(player)
            .unwrap()
            .set_helmet(crate::item::test_support::stack(
                "minecraft:diamond_helmet",
                1,
            ))
            .unwrap();
        world
            .entity_mut(player)
            .insert(OffhandSlot(crate::item::test_support::stack(
                "minecraft:shield",
                1,
            )));
        world
            .get_mut::<PlayerUi>(player)
            .unwrap()
            .0
            .set_item(2, crate::item::test_support::stack("minecraft:paper", 4))
            .unwrap();
        Self {
            router,
            manager,
            player,
            original,
            _packets: vec![packets],
        }
    }
    /// Trusted native spawn creates actual session and account identity.
    fn spawn(
        router: &mut DimensionRouter,
        session: u64,
        position: DVec3,
    ) -> (
        Entity,
        tokio::sync::mpsc::Receiver<jolyne::valentine::McpePacket>,
    ) {
        let (outbound_tx, packets) = tokio::sync::mpsc::channel(8192);
        let player = router
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
        (player, packets)
    }
    /// One authoritative world borrow for an engine operation.
    fn world(&mut self) -> &mut World {
        self.router.world_mut(0).unwrap().ecs.world_mut()
    }
    /// Native lethal damage uses ordinary armour, drops and optional immediate respawn.
    fn kill(&mut self) {
        let player = self.player;
        assert!(
            crate::entity::player_life::hurt::hurt_player(
                self.world(),
                player,
                &Hit {
                    damage: 100.0,
                    cause: ActorDamageCause::Override,
                    attacker: None,
                    by_player: false,
                    knockback_from: None,
                    knockback_power: KnockbackPower::DEFAULT
                }
            ) > 0.0
        );
    }
    /// Read guest witnesses without invoking another export.
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
    /// Inspect the exact host-owned detached values retained by the real guest.
    fn values(&self) -> Vec<crate::item::ItemStack> {
        self.manager
            .lock()
            .unwrap()
            .plugins
            .values_mut()
            .next()
            .unwrap()
            .store
            .data_mut()
            .test_item_values()
    }
}

/// Earlier native callbacks may remove the source, but owned evidence remains valid.
struct RemoveBeforeBridge;
impl PlayerLifecycleHandler for RemoveBeforeBridge {
    fn on_death(&self, world: &mut World, event: &PlayerDeath) {
        world
            .resource_mut::<crate::server::game::SessionEntityMap>()
            .remove(42);
        world.despawn(event.player);
    }
}

/// A real guest log proves the callback executed, independent of player packet delivery.
fn witness(value: u8) -> String {
    format!(
        "i32.const 1024 i32.const {value} i32.store8 i32.const 0 i32.const 1024 i32.const 1 call @log@"
    )
}

/// Check complete native slot shapes and retain the sword's actual owned component resource.
fn record_slots() -> String {
    let (identity, _) = fixture::field_layout("on-player-death", "death", &["identity"]);
    let (account, _) = fixture::field_layout(
        "on-player-death",
        "death",
        &["identity", "ok", "authenticated-xuid", "some"],
    );
    let (inventory, _) = fixture::field_layout("on-player-death", "death", &["inventory"]);
    let (main, _) = fixture::field_layout("on-player-death", "death", &["inventory", "ok", "main"]);
    let (armour, _) =
        fixture::field_layout("on-player-death", "death", &["inventory", "ok", "armour"]);
    let (ui, _) = fixture::field_layout("on-player-death", "death", &["inventory", "ok", "ui"]);
    let (held, _) = fixture::field_layout(
        "on-player-death",
        "death",
        &["inventory", "ok", "held-slot"],
    );
    format!(
        "local.get 0 i32.load8_u offset={identity} if unreachable end local.get 0 i64.load offset={account} i64.const 42 i64.ne if unreachable end local.get 0 i32.load8_u offset={inventory} if unreachable end local.get 0 i32.load offset={} i32.const 36 i32.ne if unreachable end local.get 0 i32.load offset={} i32.const 4 i32.ne if unreachable end local.get 0 i32.load offset={} i32.const 54 i32.ne if unreachable end local.get 0 i32.load8_u offset={held} i32.const 5 i32.ne if unreachable end i32.const 768 local.get 0 i32.load offset={main} i32.load offset=20 i32.store {}",
        main + 4,
        armour + 4,
        ui + 4,
        witness(b'D')
    )
}

/// Schedule a fresh world transaction that survives the source actor's removal.
fn schedule() -> String {
    "local.get 0 i32.load i32.const 128 call @[method]world-tx.world@ i32.const 128 i32.load8_u if unreachable end i32.const 132 i32.load i64.const 1 i32.const 160 call @[method]world-handle.schedule@ i32.const 160 i32.load8_u if unreachable end i32.const 164 i32.load call @task_drop@ i32.const 132 i32.load call @world-handle_drop@".into()
}

#[tokio::test]
async fn committed_death_retains_complete_slot_metadata_and_values_after_actor_removal() {
    let task = format!(
        "i32.const 768 i32.load i32.const 96 call @[method]stack.count@ i32.const 96 i32.load8_u if unreachable end i32.const 97 i32.load8_u i32.const 1 i32.ne if unreachable end i32.const 768 i32.load call @stack_drop@ {}",
        witness(b'T')
    );
    let mut harness = Harness::new(
        &[
            (
                "on-player-death",
                format!("{} {}", record_slots(), schedule()),
            ),
            ("on-world-task", task),
        ],
        4096,
        true,
        false,
    );
    harness.kill();
    assert_eq!(harness.logs(), ["D"]);
    let original = harness.original.clone();
    assert!(
        harness
            .values()
            .iter()
            .any(|value| value.item_id() == original.item_id() && value.nbt == original.nbt)
    );
    let player = harness.player;
    assert!(
        harness
            .world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .slots()
            .iter()
            .all(|stack| stack.is_empty())
    );
    harness.router.despawn_player(42);
    harness.router.tick().await.unwrap();
    assert_eq!(harness.logs(), ["D", "T"]);
    assert!(
        !harness
            .values()
            .iter()
            .any(|value| value.item_id() == original.item_id())
    );
}

#[tokio::test]
async fn earlier_native_removal_does_not_erase_captured_identity_or_inventory() {
    let mut harness = Harness::new(&[("on-player-death", record_slots())], 4096, true, true);
    harness.kill();
    assert_eq!(harness.logs(), ["D"]);
    let player = harness.player;
    assert!(harness.world().get_entity(player).is_err());
    assert!(
        harness
            .values()
            .iter()
            .any(|value| value.item_id() == "minecraft:diamond_sword")
    );
}

#[tokio::test]
async fn keep_inventory_and_immediate_respawn_still_observe_original_death_once() {
    for keep in [false, true] {
        let mut harness = Harness::new(&[("on-player-death", record_slots())], 4096, true, false);
        let rules = &mut *harness
            .world()
            .resource_mut::<crate::world::rules::GameRules>();
        rules.keep_inventory = keep;
        rules.do_immediate_respawn = true;
        harness.kill();
        assert_eq!(harness.logs(), ["D"]);
        let player = harness.player;
        assert!(!harness.world().get::<Health>(player).unwrap().is_dead());
        assert_eq!(
            harness
                .world()
                .get::<MainInventory>(player)
                .unwrap()
                .0
                .item(0)
                .unwrap()
                .is_empty(),
            !keep
        );
        assert!(
            harness
                .values()
                .iter()
                .any(|value| value.item_id() == "minecraft:diamond_sword")
        );
    }
}

#[tokio::test]
async fn missing_capability_or_small_quota_never_delivers_partial_item_resources() {
    let (inventory, _) = fixture::field_layout("on-player-death", "death", &["inventory"]);
    for (items, maximum) in [(false, 4096), (true, 8), (true, 95)] {
        let mut harness = Harness::new(
            &[(
                "on-player-death",
                format!(
                    "local.get 0 i32.load8_u offset={inventory} i32.const 1 i32.ne if unreachable end {}",
                    witness(b'E')
                ),
            )],
            maximum,
            items,
            false,
        );
        harness.kill();
        assert_eq!(harness.logs(), ["E"]);
        let mut manager = harness.manager.lock().unwrap();
        let context = manager
            .plugins
            .values_mut()
            .next()
            .unwrap()
            .store
            .data_mut();
        assert_eq!(context.test_item_bytes(), 0);
        assert!(context.test_item_values().is_empty());
    }
}

#[tokio::test]
async fn required_death_trap_releases_new_item_values_and_fences_ordinary_saves() {
    let mut harness = Harness::new(
        &[("on-player-death", "unreachable".into())],
        4096,
        true,
        false,
    );
    harness.kill();
    assert!(harness.manager.lock().unwrap().check_required().is_err());
    let player = harness.player;
    assert!(crate::server::game::inventory_effects::is_recovery_fenced(
        harness.world(),
        player
    ));
    assert!(!harness.router.save_player(42).await);
    assert!(harness.values().is_empty());
}

#[tokio::test]
async fn lethal_real_arrow_retains_departed_launch_account_in_the_owned_death() {
    let (account, _) = fixture::field_layout(
        "on-player-death",
        "death",
        &[
            "attribution",
            "owner",
            "some",
            "authenticated",
            "some",
            "xuid",
        ],
    );
    let mut harness = Harness::new(
        &[(
            "on-player-death",
            format!(
                "local.get 0 i64.load offset={account} i64.const 43 i64.ne if unreachable end {}",
                record_slots()
            ),
        )],
        4096,
        true,
        false,
    );
    let (owner, packets) = Harness::spawn(&mut harness.router, 43, DVec3::new(2.5, 17.62, 4.5));
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
    harness.router.despawn_player(43);
    harness.world().entity_mut(projectile).insert((
        Position(DVec3::new(3.5, 16.9, 4.5)),
        Velocity(DVec3::new(3.0, 0.0, 0.0)),
    ));
    let player = harness.player;
    harness.world().get_mut::<Health>(player).unwrap().current = 1.0;
    harness.router.tick().await.unwrap();
    assert_eq!(harness.logs(), ["D"]);
    assert!(
        harness
            .world()
            .get::<crate::entity::player_life::Dead>(player)
            .is_some()
    );
}
