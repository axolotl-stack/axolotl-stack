//! Real component intents are checked against initial and subsequent native player packets.

use super::*;
use crate::ecs::events::{EventBuffer, EventKind};
use crate::entity::components::*;
use crate::plugin::manifest::PluginCapability;
use crate::server::game::{GameServer, PlayerSpawnData};
use jolyne::valentine::*;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

struct Harness {
    server: GameServer,
    manager: Arc<Mutex<PluginManager>>,
    players: Vec<Entity>,
    receivers: Vec<mpsc::Receiver<McpePacket>>,
}
impl Harness {
    /// First jump retains a source handle; later jumps resolve it in a fresh transaction.
    fn new(body: String) -> Self {
        Self::with_hold(body, false)
    }
    /// Optionally retain the actual native initial-publication barrier for the source actor.
    fn with_hold(body: String, hold: bool) -> Self {
        let mut server = GameServer::with_config(crate::ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        let engine = engine();
        let manifest = manifest("example.views");
        let pending = PendingPlugin {
            component: fixture::component(&engine, &[("on-player-jump", body)]),
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
            archive_registry: Default::default(),
            command_roots: Default::default(),
            ui_registry: Default::default(),
            view_registry: Default::default(),
        }));
        PluginManager::attach_to_world(manager.clone(), server.ecs.world_mut());
        manager
            .lock()
            .unwrap()
            .initialize_startup(server.ecs.world_mut())
            .unwrap();
        if hold {
            server
                .ecs
                .world_mut()
                .resource_mut::<crate::server::game::player_events::PlayerEventHooks>()
                .register(Hold);
        }
        let mut h = Self {
            server,
            manager,
            players: Vec::new(),
            receivers: Vec::new(),
        };
        for id in [7, 8, 9] {
            let (actor, mut rx) = spawn(&mut h.server, id);
            drain(&mut rx);
            h.players.push(actor);
            h.receivers.push(rx);
        }
        h.jump(0);
        h
    }
    /// Run a controlled committed notification through the real canonical exports.
    fn jump(&mut self, index: usize) {
        self.server
            .ecs
            .world_mut()
            .resource_mut::<EventBuffer>()
            .push(ServerEvent::PlayerJump {
                entity: self.players[index],
            });
        self.manager
            .lock()
            .unwrap()
            .tick(self.server.ecs.world_mut());
    }
    /// Native reconciliation is the only publisher of the accepted view.
    fn sync(&mut self) {
        crate::server::broadcast::player_views::sync_player_views(self.server.ecs.world_mut());
    }
    /// Retain only packets from the real outbound channel and validate their complete wire codec.
    fn packets(&mut self, index: usize) -> Vec<McpePacketData> {
        drain(&mut self.receivers[index])
    }
    /// Add a second component that resolves the source through the same ordinary owned-handle API.
    fn add_owner(&mut self, body: String) {
        let mut manager = self.manager.lock().unwrap();
        let manifest = manifest("example.other-views");
        let pending = PendingPlugin {
            component: fixture::component(&manager.engine, &[("on-player-jump", body)]),
            store: store(&manager.engine, &manifest.limits),
            manifest,
        };
        manager.pending.push(pending);
    }
}
/// A narrowly scoped ordinary component with no authority to alter identity or physical inventory.
fn manifest(id: &str) -> PluginManifest {
    PluginManifest {
        id: PluginId(id.into()),
        name: "Views".into(),
        version: "1".into(),
        api_version: PLUGIN_API_VERSION.into(),
        required: false,
        config: None,
        capabilities: [PluginCapability::ReadPlayers, PluginCapability::PlayerViews].into(),
        subscriptions: [EventKind::PlayerJump].into(),
        limits: PluginLimits::default(),
    }
}
/// Spawn a complete native actor with authenticated account fields and a retained transport.
fn spawn(server: &mut GameServer, id: u64) -> (Entity, mpsc::Receiver<McpePacket>) {
    let (tx, rx) = mpsc::channel(65536);
    let actor = server
        .spawn_player(PlayerSpawnData {
            session_id: id,
            runtime_id: id as i64,
            display_name: format!("Viewer{id}"),
            xuid: Some(id.to_string()),
            uuid: Some(uuid::Uuid::from_u128(id as u128).to_string()),
            position: glam::DVec3::new(0.5, 17.62, 0.5),
            outbound_tx: tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    server
        .ecs
        .world_mut()
        .resource_mut::<crate::server::broadcast::EntityGrid>()
        .insert((0, 0), actor);
    (actor, rx)
}
/// Wire-round-trip actual outbound representations, not hand-constructed expected packets.
fn drain(rx: &mut mpsc::Receiver<McpePacket>) -> Vec<McpePacketData> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .filter(|packet| {
            matches!(
                packet.data,
                McpePacketData::AddPlayerPacket(_)
                    | McpePacketData::SetActorDataPacket(_)
                    | McpePacketData::MobEquipmentPacket(_)
                    | McpePacketData::MobArmorEquipmentPacket(_)
                    | McpePacketData::RemoveActorPacket(_)
            )
        })
        .map(|packet| {
            let mut bytes = bytes::BytesMut::new();
            packet.data.encode_inner(&mut bytes, 0, 0).unwrap();
            BorrowedMcpePacket::decode_inner(&mut bytes.freeze())
                .unwrap()
                .into_owned(McpePacketArgs)
                .unwrap()
                .data
        })
        .collect()
}
/// Store bounded guest strings away from the canonical realloc arena.
fn literal(address: usize, value: &str) -> String {
    value
        .bytes()
        .enumerate()
        .map(|(offset, byte)| {
            format!(
                "i32.const {} i32.const {byte} i32.store8 ",
                address + offset
            )
        })
        .collect()
}
/// Source handles remain owned while every later live source is freshly resolved and dropped.
fn scoped(body: &str) -> String {
    format!(
        r#"
        i32.const 600 i32.load i32.eqz if
            local.get 1 i32.const 64 call @[method]player.to-handle@
            i32.const 64 i32.load8_u if unreachable end
            i32.const 512 i32.const 68 i32.load i32.store
        else
            local.get 0 i32.const 512 i32.load i32.const 80 call @[method]world-tx.player@
            i32.const 80 i32.load8_u if unreachable end
            {body}
            i32.const 84 i32.load call @player_drop@
        end
        i32.const 600 i32.const 600 i32.load i32.const 1 i32.add i32.store
    "#
    )
}
/// A complete optional projection, with explicit false values that must not reveal native state.
fn project(tag: &str, invisible: bool, actor: bool, equipment: bool) -> String {
    format!(
        r#"
        {}
        local.get 0 i32.const 84 i32.load local.get 1
        i32.const 1
        i32.const 1 i32.const 1000 i32.const {}
        i32.const 1 i32.const 2
        i32.const 1 i32.const {}
        i32.const 1 i32.const {}
        i32.const 1 i32.const {}
        i32.const 128 call @project-player@
        i32.const 128 i32.load8_u if unreachable end
    "#,
        literal(1000, tag),
        tag.len(),
        u8::from(invisible),
        u8::from(actor),
        u8::from(equipment)
    )
}
/// Clear this exact owned pair through the public typed import.
fn clear() -> &'static str {
    r#"local.get 0 i32.const 84 i32.load local.get 1
        i32.const 0
        i32.const 0 i32.const 0 i32.const 0
        i32.const 0 i32.const 0
        i32.const 0 i32.const 0
        i32.const 0 i32.const 0
        i32.const 0 i32.const 0
        i32.const 128 call @project-player@
        i32.const 128 i32.load8_u if unreachable end"#
}
/// Extract only the subject's real initial actor metadata.
fn initial(packets: &[McpePacketData]) -> &[DataItemEntry] {
    packets
        .iter()
        .find_map(|packet| match packet {
            McpePacketData::AddPlayerPacket(add) if add.target_runtime_id.actor_runtime_id == 7 => {
                Some(add.entity_data.data.as_slice())
            }
            _ => None,
        })
        .expect("subject initial representation")
}
/// Read the displayed name without consulting or mutating authoritative identity.
fn name(data: &[DataItemEntry]) -> &str {
    data.iter()
        .find_map(|entry| match &entry.payload {
            DataItemEntryPayload::DataItemStringPayload(value)
                if entry.id == actor_data_i_ds::NAME =>
            {
                Some(&*value.value)
            }
            _ => None,
        })
        .expect("name metadata")
}
/// Read only the subject's most recent metadata update.
fn changed(packets: &[McpePacketData]) -> &[DataItemEntry] {
    packets
        .iter()
        .rev()
        .find_map(|packet| match packet {
            McpePacketData::SetActorDataPacket(data)
                if data.target_runtime_id.actor_runtime_id == 7 =>
            {
                Some(data.actor_data.data.as_slice())
            }
            _ => None,
        })
        .expect("subject metadata update")
}
/// Preserve the complete current flag word, including unrelated native bits.
fn flags(data: &[DataItemEntry]) -> u64 {
    data.iter()
        .find_map(|entry| match &entry.payload {
            DataItemEntryPayload::DataItemInt64Payload(value)
                if entry.id == actor_data_i_ds::RESERVED_0 =>
            {
                Some(value.value as u64)
            }
            _ => None,
        })
        .expect("flag metadata")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initial_tags_are_viewer_specific_and_canonical_identity_is_unchanged() {
    let body = format!(
        "i32.const 600 i32.load i32.const 1 i32.eq if {} else {} end",
        project("Blue", false, true, true),
        project("Gold", false, true, true)
    );
    let mut h = Harness::new(scoped(&body));
    h.jump(1);
    h.jump(2);
    h.sync();
    assert_eq!(name(initial(&h.packets(1))), "Blue");
    assert_eq!(name(initial(&h.packets(2))), "Gold");
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<PlayerName>(h.players[0])
            .unwrap()
            .0,
        "Viewer7"
    );
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<PlayerUuid>(h.players[0])
            .unwrap()
            .0,
        uuid::Uuid::from_u128(7)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn equipment_is_hidden_and_restored_without_overwriting_physical_slots() {
    let body = format!(
        "i32.const 600 i32.load i32.const 1 i32.eq if {} else {} end",
        project("Hidden gear", false, true, false),
        clear()
    );
    let mut h = Harness::new(scoped(&body));
    let subject = h.players[0];
    h.server
        .ecs
        .world_mut()
        .get_mut::<MainInventory>(subject)
        .unwrap()
        .0
        .set_item(
            0,
            crate::item::test_support::stack("minecraft:diamond_sword", 1),
        )
        .unwrap();
    h.server
        .ecs
        .world_mut()
        .get_mut::<ArmourInventory>(subject)
        .unwrap()
        .set_helmet(crate::item::test_support::stack(
            "minecraft:diamond_helmet",
            1,
        ))
        .unwrap();
    h.jump(1);
    h.sync();
    let hidden = h.packets(1);
    assert!(hidden.iter().any(|packet|matches!(packet,McpePacketData::AddPlayerPacket(add)if add.target_runtime_id.actor_runtime_id==7 && add.carried_item.id==0)));
    assert!(hidden.iter().any(|packet|matches!(packet,McpePacketData::MobArmorEquipmentPacket(armour)if armour.target_runtime_id.actor_runtime_id==7 && armour.head.id==0)));
    assert!(
        !h.server
            .ecs
            .world()
            .get::<ArmourInventory>(subject)
            .unwrap()
            .helmet()
            .is_empty()
    );
    h.jump(1);
    h.sync();
    let restored = h.packets(1);
    assert!(restored.iter().any(|packet|matches!(packet,McpePacketData::MobEquipmentPacket(gear)if gear.target_runtime_id.actor_runtime_id==7 && gear.container_id==0 && gear.item.id!=0)));
    assert!(restored.iter().any(|packet|matches!(packet,McpePacketData::MobArmorEquipmentPacket(armour)if armour.target_runtime_id.actor_runtime_id==7 && armour.head.id!=0)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restrictive_actor_visibility_and_native_audiences_both_remain_authoritative() {
    let body = format!(
        "i32.const 600 i32.load i32.const 1 i32.eq if {} else {} end",
        project("Hidden", false, false, true),
        project("Visible", false, true, true)
    );
    let mut h = Harness::new(scoped(&body));
    h.sync();
    h.packets(1);
    h.jump(1);
    h.sync();
    assert!(h.packets(1).iter().any(|packet|matches!(packet,McpePacketData::RemoveActorPacket(remove)if remove.target_actor_id.actor_unique_id==7)));
    assert!(!crate::server::broadcast::player_views::can_receive_actor(
        h.server.ecs.world(),
        h.players[0],
        h.players[1]
    ));
    h.server
        .ecs
        .world_mut()
        .entity_mut(h.players[0])
        .insert(ActorAudience::only([]));
    h.jump(1);
    h.sync();
    assert!(!h.packets(1).iter().any(|packet|matches!(packet,McpePacketData::AddPlayerPacket(add)if add.target_runtime_id.actor_runtime_id==7)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trapped_replacement_keeps_previously_committed_projection() {
    let body = format!(
        "i32.const 600 i32.load i32.const 1 i32.eq if {} else {} unreachable end",
        project("Committed", false, true, true),
        project("Trap", false, true, true)
    );
    let mut h = Harness::new(scoped(&body));
    h.jump(1);
    h.sync();
    assert_eq!(name(initial(&h.packets(1))), "Committed");
    h.jump(1);
    h.sync();
    assert!(h.packets(1).iter().all(|packet|!matches!(packet,McpePacketData::SetActorDataPacket(data)if data.target_runtime_id.actor_runtime_id==7)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owner_drop_restores_the_current_native_name_and_invisibility() {
    let mut h = Harness::new(scoped(&project("Projected", true, true, true)));
    h.jump(1);
    h.sync();
    h.packets(1);
    h.manager
        .lock()
        .unwrap()
        .plugins
        .remove(&PluginId("example.views".into()));
    h.sync();
    let baseline = h.packets(1);
    assert_eq!(name(changed(&baseline)), "Viewer7");
    assert_eq!(
        flags(changed(&baseline)) & (1u64 << actor_flags::INVISIBLE),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_number_replacement_channel_cannot_inherit_a_viewer_projection() {
    let mut h = Harness::new(scoped(&project("Old connection", false, true, true)));
    h.jump(1);
    h.sync();
    h.packets(1);
    let (tx, rx) = mpsc::channel(65536);
    h.server
        .ecs
        .world_mut()
        .get_mut::<PlayerSession>(h.players[1])
        .unwrap()
        .outbound_tx = tx;
    h.receivers[1] = rx;
    h.sync();
    assert_eq!(name(initial(&h.packets(1))), "Viewer7");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn false_invisibility_preserves_native_invisibility_and_immediate_flags_preserve_the_tag() {
    let mut h = Harness::new(scoped(&project("Tagged", false, true, true)));
    h.server
        .ecs
        .world_mut()
        .get_mut::<Effects>(h.players[0])
        .unwrap()
        .add(EffectType::INVISIBILITY, 0, 200);
    h.jump(1);
    h.sync();
    let spawned = h.packets(1);
    assert_ne!(
        flags(initial(&spawned)) & (1u64 << actor_flags::INVISIBLE),
        0
    );
    crate::server::game::item_use::start_using(h.server.ecs.world_mut(), h.players[0], 0, 0);
    let updated = h.packets(1);
    assert_eq!(name(changed(&updated)), "Tagged");
    assert_ne!(
        flags(changed(&updated)) & (1u64 << actor_flags::USINGITEM),
        0
    );
    assert_ne!(
        flags(changed(&updated)) & (1u64 << actor_flags::INVISIBLE),
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn self_projection_changes_metadata_without_a_duplicate_local_actor() {
    let mut h = Harness::new(scoped(&project("Local tag", false, false, false)));
    h.jump(0);
    h.sync();
    let packets = h.packets(0);
    assert_eq!(name(changed(&packets)), "Local tag");
    assert!(!packets.iter().any(|packet|matches!(packet,McpePacketData::AddPlayerPacket(add)if add.target_runtime_id.actor_runtime_id==7)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_live_owner_receives_a_typed_conflict() {
    let mut h = Harness::new(scoped(&project("First", false, true, true)));
    h.jump(1);
    let denied=project("Second",false,true,true).replace("i32.const 128 i32.load8_u if unreachable end","i32.const 128 i32.load8_u i32.const 1 i32.ne if unreachable end i32.const 129 i32.load8_u i32.const 1 i32.ne if unreachable end i32.const 0 i32.const 0 i32.const 0 call @log@");
    h.add_owner(scoped(&denied));
    h.jump(0);
    h.jump(1);
    h.sync();
    assert_eq!(name(initial(&h.packets(1))), "First");
    assert_eq!(
        h.manager.lock().unwrap().plugins[&PluginId("example.other-views".into())]
            .store
            .data()
            .test_logs
            .len(),
        1
    );
}

/// Hold only one source before its initial packet batch, using the ordinary native boundary.
struct Hold;
impl crate::server::game::player_events::PlayerEventHandler for Hold {
    fn on_joined(
        &self,
        world: &mut bevy_ecs::world::World,
        event: &crate::server::game::player_events::PlayerJoined,
    ) {
        if event.identity().session_id() == 7 {
            assert!(crate::server::game::player_events::defer_initial_state(
                world,
                event.player()
            ));
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn projection_cannot_publish_a_source_before_the_actual_initial_state_barrier_opens() {
    let mut h = Harness::with_hold(scoped(&project("After release", false, true, true)), true);
    h.jump(1);
    h.sync();
    assert!(!h.packets(1).iter().any(|packet| matches!(packet,
        McpePacketData::AddPlayerPacket(add) if add.target_runtime_id.actor_runtime_id == 7)));
    assert!(crate::server::game::player_events::release_initial_state(
        h.server.ecs.world_mut(),
        h.players[0]
    ));
    h.server.tick();
    h.sync();
    assert_eq!(name(initial(&h.packets(1))), "After release");
}

/// A real retained actor keeps its original account binding while losing its transport.
struct Retain;
impl crate::server::game::player_events::PlayerEventHandler for Retain {
    fn on_quit(
        &self,
        _: &mut bevy_ecs::world::World,
        event: &mut crate::server::game::player_events::PlayerQuit,
    ) {
        event.retain_entity();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retained_sessionless_subject_keeps_the_original_binding_and_remains_attackable() {
    let mut h = Harness::new(scoped(&project("Retained", false, true, true)));
    h.jump(1);
    h.sync();
    h.packets(1);
    h.server
        .ecs
        .world_mut()
        .resource_mut::<crate::server::game::player_events::PlayerEventHooks>()
        .register(Retain);
    h.server.despawn_player(7);
    assert!(
        h.server
            .ecs
            .world()
            .get::<PlayerSession>(h.players[0])
            .is_none()
    );
    assert!(h.server.ecs.world().get::<Health>(h.players[0]).is_some());
    h.jump(1);
    h.sync();
    assert!(
        crate::plugin::manager::views::projection(h.server.ecs.world(), h.players[0], h.players[1])
            .is_some()
    );
    let removed = crate::entity::player_life::hurt_player(
        h.server.ecs.world_mut(),
        h.players[0],
        &crate::entity::mob::combat::Hit {
            damage: 1.0,
            cause: unastar_data::actor_components::body::ActorDamageCause::Override,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
        },
    );
    assert!(removed > 0.0);
    assert!(!h.packets(1).iter().any(|packet| matches!(packet,
        McpePacketData::RemoveActorPacket(remove) if remove.target_actor_id.actor_unique_id == 7)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pair_quota_denial_does_not_publish_a_partial_second_projection() {
    let denied=project("Rejected",false,true,true).replace("i32.const 128 i32.load8_u if unreachable end","i32.const 128 i32.load8_u i32.const 1 i32.ne if unreachable end i32.const 129 i32.load8_u i32.const 3 i32.ne if unreachable end i32.const 0 i32.const 0 i32.const 0 call @log@");
    let body = format!(
        "i32.const 600 i32.load i32.const 1 i32.eq if {} else {denied} end",
        project("Accepted", false, true, true)
    );
    let mut h = Harness::new(scoped(&body));
    h.manager
        .lock()
        .unwrap()
        .plugins
        .get_mut(&PluginId("example.views".into()))
        .unwrap()
        .store
        .data_mut()
        .api_limits
        .max_view_pairs = 1;
    h.jump(1);
    h.jump(2);
    h.sync();
    assert_eq!(name(initial(&h.packets(1))), "Accepted");
    assert_eq!(name(initial(&h.packets(2))), "Viewer7");
    assert_eq!(
        h.manager.lock().unwrap().plugins[&PluginId("example.views".into())]
            .store
            .data()
            .test_logs
            .len(),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn invalid_tag_does_not_reserve_a_pair_before_a_valid_write() {
    let denied=project(&"x".repeat(1025),false,true,true).replace("i32.const 128 i32.load8_u if unreachable end","i32.const 128 i32.load8_u i32.const 1 i32.ne if unreachable end i32.const 129 i32.load8_u i32.const 2 i32.ne if unreachable end");
    let mut h = Harness::new(scoped(&format!(
        "{denied} {}",
        project("Valid", false, true, true)
    )));
    h.manager
        .lock()
        .unwrap()
        .plugins
        .get_mut(&PluginId("example.views".into()))
        .unwrap()
        .store
        .data_mut()
        .api_limits
        .max_view_pairs = 1;
    h.jump(1);
    h.sync();
    assert_eq!(name(initial(&h.packets(1))), "Valid");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_outbound_channel_retries_projected_metadata_without_advancing_the_cache() {
    let body = format!(
        "i32.const 600 i32.load i32.const 1 i32.eq if {} else {} end",
        project("Old", false, true, true),
        project("New", false, true, true)
    );
    let mut h = Harness::new(scoped(&body));
    let tx = h
        .server
        .ecs
        .world()
        .get::<PlayerSession>(h.players[1])
        .unwrap()
        .outbound_tx
        .clone();
    // Backpressure must preserve the exact original authenticated connection.
    let _occupied = tx.try_reserve_many(tx.capacity() - 1).unwrap();
    h.jump(1);
    h.packets(1);
    for _ in 0..16 {
        h.sync();
        h.packets(1);
    }
    h.jump(1);
    tx.try_send(
        PlayStatusPacket {
            status: PlayStatusPacketStatus::PlayerSpawn,
        }
        .into(),
    )
    .unwrap();
    h.sync();
    assert!(h.packets(1).is_empty());
    h.sync();
    assert_eq!(name(changed(&h.packets(1))), "New");
}

/// Complete a real native world move and its exact client loading acknowledgement.
async fn transfer(
    router: &mut crate::server::worlds::DimensionRouter,
    session: u64,
    feet: glam::DVec3,
    rx: &mut mpsc::Receiver<McpePacket>,
) {
    let (dimension, actor) = router.locate_session(session).unwrap();
    crate::server::worlds::request_transfer_checked(
        router.world_mut(dimension).unwrap().ecs.world_mut(),
        actor,
        1,
        feet,
        0,
        format!("view-transfer-{session}"),
        |_, _, _| true,
    )
    .unwrap();
    router.tick().await.unwrap();
    assert_eq!(router.locate_session(session).unwrap().0, 1);
    let mut loading = None;
    while let Ok(packet) = rx.try_recv() {
        if let McpePacketData::ChangeDimensionPacket(change) = packet.data {
            loading = change.loading_screen_id;
        }
    }
    let ack = McpePacket::from(ServerboundLoadingScreenPacket {
        loading_screen_packet_type: EnumsServerboundLoadingScreenPacketType::EndLoadingScreen,
        loading_screen_id: Some(loading.expect("actual native loading barrier")),
    });
    let mut bytes = bytes::BytesMut::new();
    ack.data.encode_inner(&mut bytes, 0, 0).unwrap();
    router.route_packet(
        session,
        McpePacketArgs,
        BorrowedMcpePacket::decode_inner(&mut bytes.freeze()).unwrap(),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn world_migration_discards_old_pair_and_fresh_component_scope_can_rebind() {
    let mut h = Harness::new(scoped(&project("Scoped tag", false, true, true)));
    h.jump(1);
    h.sync();
    h.packets(1);
    let Harness {
        server,
        manager,
        mut receivers,
        ..
    } = h;
    let mut router = crate::server::worlds::DimensionRouter::new(server);
    let destination = router
        .create_world(crate::ServerConfig {
            world: crate::world::WorldConfig {
                dimension: 1,
                ..crate::world::WorldConfig::void_spawn_platform(1)
            },
            ..Default::default()
        })
        .unwrap();
    router.add(destination).unwrap();
    PluginManager::attach_to_world(
        manager.clone(),
        router.world_mut(1).unwrap().ecs.world_mut(),
    );
    router.set_plugin_manager(manager);
    router.refresh_directory();
    transfer(
        &mut router,
        7,
        glam::DVec3::new(4.5, 16.0, 4.5),
        &mut receivers[0],
    )
    .await;
    transfer(
        &mut router,
        8,
        glam::DVec3::new(6.5, 16.0, 4.5),
        &mut receivers[1],
    )
    .await;
    router.tick().await.unwrap();
    assert_eq!(name(initial(&drain(&mut receivers[1]))), "Viewer7");
    let (_, viewer) = router.locate_session(8).unwrap();
    router
        .world_mut(1)
        .unwrap()
        .ecs
        .world_mut()
        .resource_mut::<EventBuffer>()
        .push(ServerEvent::PlayerJump { entity: viewer });
    router.tick().await.unwrap();
    router.tick().await.unwrap();
    assert_eq!(name(changed(&drain(&mut receivers[1]))), "Scoped tag");
}
