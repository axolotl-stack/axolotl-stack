//! Real canonical imports stage native player mechanics and preserve callback failure semantics.

use super::*;
use crate::ecs::events::{EventBuffer, EventKind};
use crate::entity::components::{EffectType, Effects, GameMode, Health, PlayerSession, Velocity};
use crate::plugin::manifest::PluginCapability;
use crate::server::game::{GameServer, PlayerSpawnData, ServerConfig};
use jolyne::valentine::{McpePacket, McpePacketData};
use std::sync::{Arc, Mutex};

struct Harness {
    server: GameServer,
    manager: Arc<Mutex<PluginManager>>,
    player: Entity,
    packets: tokio::sync::mpsc::Receiver<McpePacket>,
}
impl Harness {
    /// Complete native join first; each case then calls an actual current-ABI component.
    fn new(bodies: &[(&str, String)], permitted: bool) -> Self {
        Self::with_capabilities(bodies, permitted, &[])
    }
    /// Compose current-ABI imports with additional explicit manifest grants.
    fn with_capabilities(
        bodies: &[(&str, String)],
        permitted: bool,
        extra: &[PluginCapability],
    ) -> Self {
        Self::configured(bodies, permitted, extra, true)
    }
    /// Optional malformed callbacks still reject defaults and discard their own staged imports.
    fn configured(
        bodies: &[(&str, String)],
        permitted: bool,
        extra: &[PluginCapability],
        required: bool,
    ) -> Self {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        let engine = engine();
        let mut capabilities = std::collections::HashSet::from([
            PluginCapability::ReadPlayers,
            PluginCapability::SendMessages,
        ]);
        if permitted {
            capabilities.insert(PluginCapability::PlayerMechanics);
        }
        capabilities.extend(extra.iter().cloned());
        let mut subscriptions = [
            EventKind::PlayerCommand,
            EventKind::PlayerJump,
            EventKind::PlayerChat,
            EventKind::PlayerDamage,
            EventKind::PlayerMove,
        ]
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
        if bodies
            .iter()
            .any(|(name, _)| *name == "on-player-admission")
        {
            subscriptions.insert(EventKind::PlayerJoin);
        }
        let manifest = PluginManifest {
            id: PluginId("example.mechanics".into()),
            name: "Mechanics".into(),
            version: "1".into(),
            api_version: PLUGIN_API_VERSION.into(),
            required,
            config: None,
            capabilities,
            subscriptions,
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
            command_roots: Default::default(),
            ui_registry: Default::default(),
            view_registry: Default::default(),
        }));
        PluginManager::attach_to_world(manager.clone(), server.ecs.world_mut());
        {
            let mut manager = manager.lock().unwrap();
            assert!(
                manager.begin_tick(server.ecs.world_mut()),
                "{:?}",
                manager.readiness.check()
            );
        }
        let (outbound_tx, mut packets) = tokio::sync::mpsc::channel(4096);
        let player = server
            .spawn_player(PlayerSpawnData {
                session_id: 42,
                runtime_id: 42,
                display_name: "Player".into(),
                xuid: Some("42".into()),
                uuid: Some(uuid::Uuid::from_u128(42).to_string()),
                position: glam::DVec3::new(0.5, 17.62, 0.5),
                outbound_tx,
                saved_data: None,
                chunk_radius: 0,
            })
            .unwrap();
        while packets.try_recv().is_ok() {}
        server.ecs.world_mut().resource_mut::<EventBuffer>().drain();
        Self {
            server,
            manager,
            player,
            packets,
        }
    }
    /// Dispatch a native buffered notification but leave mechanics pending for its world phase.
    fn jump(&mut self) {
        self.server
            .ecs
            .world_mut()
            .resource_mut::<EventBuffer>()
            .push(ServerEvent::PlayerJump {
                entity: self.player,
            });
        self.manager
            .lock()
            .unwrap()
            .drain_world_events(self.server.ecs.world_mut());
    }
    /// Apply outside the manager guard, as GameServer::tick does after simulation.
    fn apply(&mut self) {
        players::apply_mechanics(self.server.ecs.world_mut());
    }
    /// Run the ordinary effects packet system without advancing effect duration in a test assertion.
    fn sync(&mut self) {
        let mut schedule = bevy_ecs::schedule::Schedule::default();
        schedule.add_systems(crate::server::game::effects::sync_player_effects);
        schedule.run(self.server.ecs.world_mut());
    }
    /// Observe only the current component's bounded log evidence.
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

/// Lower a finite effect and assert that the host returned only Staged.
fn effect(kind: u8, amplifier: u8, duration: u32, ambient: bool, particles: bool) -> String {
    format!(
        "local.get 1 i32.const {kind} i32.const {amplifier} i32.const {duration} i32.const {} i32.const {} i32.const 0 i32.const 0 i32.const 64 call @[method]player.apply-effect@ i32.const 64 i32.load8_u if unreachable end i32.const 65 i32.load8_u if unreachable end",
        u8::from(ambient),
        u8::from(particles)
    )
}
/// Stage server motion with a canonical owned-result return area.
fn velocity(x: f64) -> String {
    format!(
        "local.get 1 f64.const {x} f64.const 0 f64.const 0 i32.const 64 call @[method]player.set-velocity@ i32.const 64 i32.load8_u if unreachable end"
    )
}
/// One byte is enough to witness actual guest progress.
fn witness() -> &'static str {
    "i32.const 1024 i32.const 120 i32.store8 i32.const 0 i32.const 1024 i32.const 1 call @log@"
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_effects_and_motion_wait_for_commit_and_encode_native_updates() {
    let mut harness = Harness::new(
        &[(
            "on-player-jump",
            format!(
                "{} {} {}",
                effect(0, 1, 100, true, false),
                velocity(0.75),
                witness()
            ),
        )],
        true,
    );
    harness.jump();
    let world = harness.server.ecs.world();
    assert!(
        world
            .get::<Effects>(harness.player)
            .is_none_or(|effects| !effects.has(EffectType::SPEED))
    );
    assert!(world.get::<Velocity>(harness.player).is_none());
    assert!(harness.packets.try_recv().is_err());
    harness.apply();
    harness.sync();
    assert_eq!(harness.logs(), ["x"]);
    let effect = harness
        .server
        .ecs
        .world()
        .get::<Effects>(harness.player)
        .unwrap()
        .get(EffectType::SPEED)
        .unwrap();
    assert_eq!(
        (
            effect.level,
            effect.duration_ticks,
            effect.ambient,
            effect.show_particles
        ),
        (1, 100, true, false)
    );
    let mut effect_packet = false;
    let mut motion_packet = false;
    while let Ok(packet) = harness.packets.try_recv() {
        // Exercise real packet codecs as well as native values.
        let mut encoded = bytes::BytesMut::new();
        packet.data.encode_inner(&mut encoded, 0, 0).unwrap();
        jolyne::valentine::BorrowedMcpePacket::decode_inner(&mut encoded.freeze()).unwrap();
        match packet.data {
            McpePacketData::MobEffectPacket(packet) => {
                assert_eq!(packet.effect_id, 1);
                assert!(packet.ambient);
                assert!(!packet.show_particles);
                effect_packet = true;
            }
            McpePacketData::SetActorMotionPacket(packet) => {
                assert_eq!(packet.motion.x, 0.75);
                motion_packet = true;
            }
            _ => {}
        }
    }
    assert!(effect_packet && motion_packet);
    harness.jump();
    harness.apply();
    harness.sync();
    assert!(
        harness.packets.try_recv().is_err(),
        "identical target state needs no repeated packet"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn traps_discard_explicit_mechanics_but_valid_damage_veto_preserves_replacements() {
    let mut trap = Harness::new(
        &[("on-player-jump", format!("{} unreachable", velocity(1.0)))],
        true,
    );
    trap.jump();
    trap.apply();
    assert!(trap.manager.lock().unwrap().readiness.check().is_err());
    assert!(trap.packets.try_recv().is_err());
    // Test the real typed damage callback's option veto, not a handled-command boolean.
    let (player, _) = fixture::field_layout("on-player-damage", "player", &[]);
    let mut veto = Harness::new(
        &[(
            "on-player-damage",
            format!(
                "{} i32.const 32 i32.const 0 i32.store i32.const 32",
                velocity(1.0).replace(
                    "local.get 1",
                    &format!("local.get 0 i32.load offset={player}")
                )
            ),
        )],
        true,
    );
    let target = veto.player;
    let before_timer = veto
        .server
        .ecs
        .world()
        .get::<crate::entity::mob::combat::HurtTimer>(target)
        .map(|timer| (timer.until_tick, timer.last_damage));
    let lost = crate::entity::player_life::hurt_player(
        veto.server.ecs.world_mut(),
        target,
        &crate::entity::mob::combat::Hit {
            damage: 2.0,
            cause: unastar_data::actor_components::body::ActorDamageCause::EntityAttack,
            attacker: None,
            by_player: true,
            knockback_from: None,
            knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
        },
    );
    assert_eq!(lost, 0.0);
    veto.apply();
    assert_eq!(
        veto.server.ecs.world().get::<Velocity>(target).unwrap().0.x,
        1.0
    );
    assert_eq!(
        veto.server
            .ecs
            .world()
            .get::<Health>(target)
            .unwrap()
            .current,
        20.0
    );
    assert_eq!(
        veto.server
            .ecs
            .world()
            .get::<crate::entity::mob::combat::HurtTimer>(target)
            .map(|timer| (timer.until_tick, timer.last_damage)),
        before_timer
    );
    assert!(matches!(
        veto.packets.try_recv().unwrap().data,
        McpePacketData::SetActorMotionPacket(_)
    ));
    assert!(veto.manager.lock().unwrap().readiness.check().is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handled_chat_commits_explicit_effects_and_messages_instead_of_discarding_them() {
    let mut harness = Harness::new(
        &[(
            "on-player-chat",
            format!(
                "{} i32.const 1024 i32.const 120 i32.store8 local.get 1 i32.const 1024 i32.const 1 i32.const 80 call @[method]player.message@ i32.const 80 i32.load8_u if unreachable end i32.const 0",
                velocity(1.0)
            ),
        )],
        true,
    );
    harness
        .server
        .ecs
        .world_mut()
        .resource_mut::<EventBuffer>()
        .push(ServerEvent::PlayerChat {
            entity: harness.player,
            player_id: "Player".into(),
            message: "message".into(),
        });
    harness
        .manager
        .lock()
        .unwrap()
        .drain_world_events(harness.server.ecs.world_mut());
    harness.apply();
    assert_eq!(
        harness
            .server
            .ecs
            .world()
            .get::<Velocity>(harness.player)
            .unwrap()
            .0
            .x,
        1.0
    );
    let packets = std::iter::from_fn(|| harness.packets.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        packets
            .iter()
            .any(|packet| matches!(packet.data, McpePacketData::TextPacket(_)))
    );
    assert!(
        packets
            .iter()
            .any(|packet| matches!(packet.data, McpePacketData::SetActorMotionPacket(_)))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stale_replaced_session_dead_and_recovery_fenced_targets_drop_queued_intents() {
    for variant in 0..4 {
        let mut harness = Harness::new(&[("on-player-jump", velocity(1.0))], true);
        harness.jump();
        let world = harness.server.ecs.world_mut();
        match variant {
            0 => {
                world
                    .get_mut::<PlayerSession>(harness.player)
                    .unwrap()
                    .session_id += 1;
            }
            1 => {
                let (tx, _rx) = tokio::sync::mpsc::channel(32);
                world
                    .get_mut::<PlayerSession>(harness.player)
                    .unwrap()
                    .outbound_tx = tx;
            }
            2 => {
                world
                    .entity_mut(harness.player)
                    .insert(crate::entity::player_life::Dead);
            }
            _ => {
                let source = crate::server::game::inventory_effects::RecoverySource::capture(
                    world,
                    harness.player,
                )
                .unwrap();
                crate::server::game::inventory_effects::fence_for_recovery(world, &source).unwrap();
            }
        }
        harness.apply();
        assert!(
            harness
                .server
                .ecs
                .world()
                .get::<Velocity>(harness.player)
                .is_none()
        );
        assert!(harness.packets.try_recv().is_err());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typed_permissions_limits_and_native_mode_overlays_remain_authoritative() {
    let denied = "local.get 1 f64.const 1 f64.const 0 f64.const 0 i32.const 64 call @[method]player.set-velocity@ i32.const 64 i32.load8_u i32.const 1 i32.ne if unreachable end";
    let mut harness = Harness::new(
        &[("on-player-jump", format!("{denied} {}", witness()))],
        false,
    );
    harness.jump();
    harness.apply();
    assert_eq!(harness.logs(), ["x"]);
    assert!(
        harness
            .server
            .ecs
            .world()
            .get::<Velocity>(harness.player)
            .is_none()
    );
    let mut harness = Harness::new(
        &[(
            "on-player-jump",
            format!(
                "local.get 1 i32.const 1 i32.const 1 i32.const 64 call @[method]player.set-mode-override@ i32.const 64 i32.load8_u if unreachable end local.get 1 i32.const 2 i32.const 64 call @[method]player.set-game-mode@ i32.const 64 i32.load8_u if unreachable end local.get 1 i32.const 1 i32.const 1 i32.const 64 call @[method]player.set-flight-override@ i32.const 64 i32.load8_u if unreachable end {}",
                witness()
            ),
        )],
        true,
    );
    harness.jump();
    harness.apply();
    let world = harness.server.ecs.world();
    assert_eq!(
        crate::command::exec::base_game_mode(world, harness.player),
        Some(GameMode::Adventure)
    );
    assert_eq!(
        world.get::<GameMode>(harness.player),
        Some(&GameMode::Creative)
    );
    assert_eq!(
        crate::server::game::flight::override_value(world, harness.player),
        Some(true)
    );
    assert!(crate::server::game::flight::allowed(world, harness.player));
    let packets = std::iter::from_fn(|| harness.packets.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        packets
            .iter()
            .any(|packet| matches!(packet.data, McpePacketData::UpdatePlayerGameTypePacket(_)))
    );
    assert!(
        packets
            .iter()
            .any(|packet| matches!(packet.data, McpePacketData::UpdateAbilitiesPacket(_)))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn instant_damage_uses_native_hooks_after_unlocking_manager_and_can_be_vetoed() {
    let (owner_xuid, _) = fixture::field_layout(
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
    let self_sourced = effect(6, 0, 1, false, true).replace(
        "i32.const 0 i32.const 0 i32.const 64 call @[method]player.apply-effect@",
        "i32.const 1 local.get 1 i32.const 64 call @[method]player.apply-effect@",
    );
    let mut harness = Harness::new(
        &[
            ("on-player-jump", self_sourced),
            (
                "on-player-damage",
                format!(
                    "local.get 0 i64.load offset={owner_xuid} i64.const 42 i64.ne if unreachable end {} i32.const 32 i32.const 0 i32.store i32.const 32",
                    witness()
                ),
            ),
        ],
        true,
    );
    harness.jump();
    harness.apply();
    assert_eq!(harness.logs(), ["x"]);
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
    assert!(harness.manager.lock().unwrap().readiness.check().is_ok());
}

/// Resolve current import-result layouts from WIT so tests do not invent record offsets.
fn mechanics_layout(fields: &[&str]) -> usize {
    let mut resolve = wit_parser::Resolve::default();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../unastar-api/wit");
    resolve.push_dir(path).unwrap();
    let function = resolve
        .interfaces
        .iter()
        .find_map(|(_, interface)| interface.functions.get("[method]player.mechanics"))
        .unwrap();
    let mut ty = function.result.unwrap();
    let mut sizes = wit_parser::SizeAlign::default();
    sizes.fill(&resolve);
    let mut offset = 0usize;
    for name in fields {
        let mut id = loop {
            let wit_parser::Type::Id(id) = ty else {
                panic!("structured value")
            };
            if let wit_parser::TypeDefKind::Type(inner) = resolve.types[id].kind {
                ty = inner;
            } else {
                break id;
            }
        };
        // Results prefix the aligned payload with their discriminant.
        if *name == "ok" {
            let wit_parser::TypeDefKind::Result(result) = &resolve.types[id].kind else {
                panic!("result")
            };
            offset = (offset + 1).next_multiple_of(sizes.align(&ty).align_wasm32());
            ty = result.ok.unwrap();
            continue;
        }
        if *name == "some" {
            let wit_parser::TypeDefKind::Option(inner) = resolve.types[id].kind else {
                panic!("option")
            };
            offset = (offset + 1).next_multiple_of(sizes.align(&inner).align_wasm32());
            ty = inner;
            continue;
        }
        while let wit_parser::TypeDefKind::Type(wit_parser::Type::Id(inner)) =
            resolve.types[id].kind
        {
            id = inner;
        }
        let wit_parser::TypeDefKind::Record(record) = &resolve.types[id].kind else {
            panic!("record")
        };
        let mut found = None;
        for field in &record.fields {
            offset = offset.next_multiple_of(sizes.align(&field.ty).align_wasm32());
            if field.name == *name {
                found = Some(field.ty);
                break;
            }
            offset += sizes.size(&field.ty).size_wasm32();
        }
        ty = found.expect("named field");
    }
    offset
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_component_reads_owned_effects_motion_and_base_mode_without_predicting_staged_state()
{
    let velocity_tag = mechanics_layout(&["ok", "velocity"]);
    let velocity = mechanics_layout(&["ok", "velocity", "some", "x"]);
    let effects = mechanics_layout(&["ok", "effects"]);
    let base = mechanics_layout(&["ok", "base-game-mode"]);
    let mut harness = Harness::new(
        &[
            (
                "on-player-jump",
                format!(
                    "{} local.get 1 i32.const 128 call @[method]player.mechanics@ i32.const 128 i32.load8_u if unreachable end i32.const {} i32.load8_u if unreachable end",
                    velocity_body(0.5),
                    128 + velocity_tag
                ),
            ),
            (
                "on-player-chat",
                format!(
                    "local.get 1 i32.const 128 call @[method]player.mechanics@ i32.const 128 i32.load8_u if unreachable end i32.const {} f64.load f64.const 0.5 f64.ne if unreachable end i32.const {} i32.load i32.const 1 i32.ne if unreachable end i32.const {} i32.load8_u if unreachable end {} i32.const 1",
                    128 + velocity,
                    128 + effects + 4,
                    128 + base,
                    witness()
                ),
            ),
        ],
        true,
    );
    harness.jump();
    harness.apply();
    crate::server::game::effects::add_effect(
        harness.server.ecs.world_mut(),
        harness.player,
        EffectType::HASTE,
        2,
        123,
    );
    harness
        .server
        .ecs
        .world_mut()
        .resource_mut::<EventBuffer>()
        .push(ServerEvent::PlayerChat {
            entity: harness.player,
            player_id: "Player".into(),
            message: "read".into(),
        });
    harness
        .manager
        .lock()
        .unwrap()
        .drain_world_events(harness.server.ecs.world_mut());
    assert_eq!(harness.logs(), ["x"]);
}

/// Keep the motion-construction helper available when a local field offset is named velocity.
fn velocity_body(x: f64) -> String {
    velocity(x)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn effect_precedence_and_native_recipient_veto_do_not_bypass_authority() {
    let mut harness = Harness::new(&[("on-player-jump", effect(0, 0, 20, true, false))], true);
    crate::server::game::effects::add_effect(
        harness.server.ecs.world_mut(),
        harness.player,
        EffectType::SPEED,
        2,
        100,
    );
    harness.jump();
    harness.apply();
    let effect = harness
        .server
        .ecs
        .world()
        .get::<Effects>(harness.player)
        .unwrap()
        .get(EffectType::SPEED)
        .unwrap();
    assert_eq!(
        (
            effect.level,
            effect.duration_ticks,
            effect.ambient,
            effect.show_particles
        ),
        (2, 100, false, true)
    );
    struct Reject;
    impl crate::entity::projectile::hooks::ProjectileHandler for Reject {
        fn on_effect(
            &self,
            _world: &mut bevy_ecs::world::World,
            event: &mut crate::entity::projectile::hooks::EffectApplication,
        ) {
            event.cancel();
        }
    }
    harness
        .server
        .ecs
        .world_mut()
        .get_resource_or_init::<crate::entity::projectile::hooks::ProjectileHooks>()
        .register(Reject);
    crate::server::game::effects::clear_effects(harness.server.ecs.world_mut(), harness.player);
    harness.jump();
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
async fn bounded_queue_invalid_motion_and_invalid_duration_return_typed_errors_without_native_writes()
 {
    let mut body = String::new();
    body.push_str("local.get 1 f64.const nan f64.const 0 f64.const 0 i32.const 64 call @[method]player.set-velocity@ i32.const 64 i32.load8_u i32.const 1 i32.ne if unreachable end ");
    body.push_str("local.get 1 f64.const 65 f64.const 0 f64.const 0 i32.const 64 call @[method]player.set-velocity@ i32.const 64 i32.load8_u i32.const 1 i32.ne if unreachable end ");
    body.push_str("local.get 1 i32.const 0 i32.const 0 i32.const 0 i32.const 0 i32.const 1 i32.const 0 i32.const 0 i32.const 64 call @[method]player.apply-effect@ i32.const 64 i32.load8_u i32.const 1 i32.ne if unreachable end ");
    for _ in 0..64 {
        body.push_str(&velocity(0.0));
        body.push(' ');
    }
    body.push_str("local.get 1 f64.const 1 f64.const 0 f64.const 0 i32.const 64 call @[method]player.set-velocity@ i32.const 64 i32.load8_u i32.const 1 i32.ne if unreachable end ");
    body.push_str(witness());
    let mut harness = Harness::new(&[("on-player-jump", body)], true);
    harness.jump();
    harness.apply();
    assert_eq!(harness.logs(), ["x"]);
    assert_eq!(
        harness
            .server
            .ecs
            .world()
            .get::<Velocity>(harness.player)
            .unwrap()
            .0,
        glam::DVec3::ZERO
    );
    assert!(matches!(
        harness.packets.try_recv().unwrap().data,
        McpePacketData::SetActorMotionPacket(_)
    ));
    assert!(harness.packets.try_recv().is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn actual_inventory_quarantine_blocks_current_reads_and_discards_previously_staged_motion() {
    use crate::server::game::inventory_effects::{self, InventoryEffect, InventoryState};
    let unavailable = "local.get 1 i32.const 128 call @[method]player.mechanics@ i32.const 128 i32.load8_u i32.const 1 i32.ne if unreachable end local.get 1 f64.const 1 f64.const 0 f64.const 0 i32.const 64 call @[method]player.set-velocity@ i32.const 64 i32.load8_u i32.const 1 i32.ne if unreachable end";
    let mut harness = Harness::new(
        &[
            ("on-player-jump", velocity(1.0)),
            (
                "on-player-chat",
                format!("{unavailable} {} i32.const 1", witness()),
            ),
        ],
        true,
    );
    harness.jump();
    let directory = tempfile::tempdir().unwrap();
    let provider = Arc::new(crate::storage::LevelDBPlayerProvider::open(directory.path()).unwrap());
    harness
        .server
        .ecs
        .world_mut()
        .insert_resource(crate::server::game::types::PlayerProviderResource(provider));
    inventory_effects::install(&mut harness.server).unwrap();
    let world = harness.server.ecs.world_mut();
    let expected = InventoryState::capture(world, harness.player).unwrap();
    let replacement = expected.clone();
    inventory_effects::submit(
        world,
        harness.player,
        InventoryEffect {
            id: "guarded-mechanics".into(),
            expected,
            replacement,
        },
    )
    .unwrap();
    assert!(inventory_effects::is_quarantined(world, harness.player));
    harness.apply();
    assert!(
        harness
            .server
            .ecs
            .world()
            .get::<Velocity>(harness.player)
            .is_none()
    );
    harness
        .server
        .ecs
        .world_mut()
        .resource_mut::<EventBuffer>()
        .push(ServerEvent::PlayerChat {
            entity: harness.player,
            player_id: "Player".into(),
            message: "blocked".into(),
        });
    harness
        .manager
        .lock()
        .unwrap()
        .drain_world_events(harness.server.ecs.world_mut());
    assert_eq!(harness.logs(), ["x"]);
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            inventory_effects::poll(harness.server.ecs.world_mut());
            if inventory_effects::receipt(
                harness.server.ecs.world(),
                harness.player,
                "guarded-mechanics",
            )
            .is_some()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(
        harness
            .server
            .ecs
            .world()
            .get::<Velocity>(harness.player)
            .is_none()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_guest_can_clear_mode_and_flight_overlays_without_replacing_durable_base() {
    let mut harness=Harness::new(&[("on-player-jump","local.get 1 i32.const 0 i32.const 0 i32.const 64 call @[method]player.set-mode-override@ i32.const 64 i32.load8_u if unreachable end local.get 1 i32.const 0 i32.const 0 i32.const 64 call @[method]player.set-flight-override@ i32.const 64 i32.load8_u if unreachable end".into())],true);
    let world = harness.server.ecs.world_mut();
    assert!(crate::command::exec::set_mode_override(
        world,
        harness.player,
        Some(GameMode::Creative)
    ));
    assert!(crate::server::game::flight::set_override(
        world,
        harness.player,
        Some(true)
    ));
    while harness.packets.try_recv().is_ok() {}
    harness.jump();
    harness.apply();
    let world = harness.server.ecs.world();
    assert_eq!(
        world.get::<GameMode>(harness.player),
        Some(&GameMode::Survival)
    );
    assert_eq!(
        crate::command::exec::base_game_mode(world, harness.player),
        Some(GameMode::Survival)
    );
    assert_eq!(
        crate::command::exec::mode_override(world, harness.player),
        None
    );
    assert_eq!(
        crate::server::game::flight::override_value(world, harness.player),
        None
    );
    assert!(!crate::server::game::flight::allowed(world, harness.player));
    let packets = std::iter::from_fn(|| harness.packets.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        packets
            .iter()
            .any(|packet| matches!(packet.data, McpePacketData::UpdateAbilitiesPacket(_)))
    );
    harness.jump();
    harness.apply();
    assert!(harness.packets.try_recv().is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handled_real_command_keeps_explicit_mechanics_and_reply_without_running_native_default() {
    use jolyne::valentine::{BorrowedMcpePacket, CommandRequestPacket, McpePacketArgs};
    let mut harness = Harness::new(
        &[(
            "on-player-command",
            format!(
                "{} i32.const 1024 i32.const 120 i32.store8 local.get 1 i32.const 1024 i32.const 1 i32.const 80 call @[method]player.message@ i32.const 80 i32.load8_u if unreachable end i32.const 0",
                velocity(0.5)
            ),
        )],
        true,
    );
    harness
        .server
        .ecs
        .world_mut()
        .entity_mut(harness.player)
        .insert(crate::command::Operator);
    let before = *harness
        .server
        .ecs
        .world()
        .resource::<crate::world::rules::Difficulty>();
    let packet = McpePacket::from(CommandRequestPacket {
        command: "/difficulty hard".into(),
        origin: jolyne::valentine::types::CommandOriginDatajson {
            type_: "player".into(),
            request_id: "mechanics-42".into(),
            ..Default::default()
        },
        is_internal: false,
        version: "latest".into(),
    });
    let mut encoded = bytes::BytesMut::new();
    packet.data.encode_inner(&mut encoded, 0, 0).unwrap();
    harness.server.route_packet(
        42,
        McpePacketArgs,
        BorrowedMcpePacket::decode_inner(&mut encoded.freeze()).unwrap(),
    );
    harness.server.tick();
    assert_eq!(
        *harness
            .server
            .ecs
            .world()
            .resource::<crate::world::rules::Difficulty>(),
        before
    );
    assert_eq!(
        harness
            .server
            .ecs
            .world()
            .get::<Velocity>(harness.player)
            .unwrap()
            .0
            .x,
        0.5
    );
    let packets = std::iter::from_fn(|| harness.packets.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        packets
            .iter()
            .any(|packet| matches!(packet.data, McpePacketData::TextPacket(_)))
    );
    assert!(
        packets
            .iter()
            .any(|packet| matches!(packet.data, McpePacketData::SetActorMotionPacket(_)))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn genuine_deferred_admission_allows_owned_reads_but_rejects_ordinary_mechanics() {
    let defer = "local.get 1 i32.const 64 call @[method]player.defer-admission@ i32.const 64 i32.load8_u if unreachable end i32.const 512 i32.const 68 i32.load i32.store i32.const 32 i32.const 2 i32.store i32.const 32";
    let unavailable = "local.get 1 f64.const 1 f64.const 0 f64.const 0 i32.const 64 call @[method]player.set-velocity@ i32.const 64 i32.load8_u i32.const 1 i32.ne if unreachable end local.get 1 i32.const 128 call @[method]player.mechanics@ i32.const 128 i32.load8_u if unreachable end";
    let mut harness = Harness::new(
        &[
            ("on-player-admission", defer.into()),
            ("on-player-jump", format!("{unavailable} {}", witness())),
        ],
        true,
    );
    assert!(crate::server::game::player_events::initial_state_pending(
        harness.server.ecs.world(),
        harness.player
    ));
    harness.jump();
    harness.apply();
    assert_eq!(harness.logs(), ["x"]);
    assert!(
        harness
            .server
            .ecs
            .world()
            .get::<Velocity>(harness.player)
            .is_none()
    );
    assert!(harness.packets.try_recv().is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_late_effect_observer_cannot_replace_the_authorized_runtime_binding() {
    struct Replace(bool);
    impl crate::entity::projectile::hooks::ProjectileHandler for Replace {
        fn on_effect(
            &self,
            world: &mut bevy_ecs::world::World,
            event: &mut crate::entity::projectile::hooks::EffectApplication,
        ) {
            if self.0 {
                world
                    .get_mut::<crate::entity::components::RuntimeEntityId>(event.target())
                    .unwrap()
                    .0 += 1;
            } else {
                event.duration_ticks = u32::MAX;
            }
        }
    }
    for replace_identity in [true, false] {
        let mut harness = Harness::new(&[("on-player-jump", effect(0, 0, 100, false, true))], true);
        harness
            .server
            .ecs
            .world_mut()
            .get_resource_or_init::<crate::entity::projectile::hooks::ProjectileHooks>()
            .register(Replace(replace_identity));
        harness.jump();
        harness.apply();
        harness.sync();
        assert!(
            harness
                .server
                .ecs
                .world()
                .get::<Effects>(harness.player)
                .is_none_or(|effects| !effects.has(EffectType::SPEED))
        );
        assert!(harness.packets.try_recv().is_err());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn composed_handled_chat_commits_ui_and_mechanics_without_leaking_owned_inventory_images() {
    let body = format!(
        r#"
        local.get 1 i32.const 64 call @[method]player.inventory@
        i32.const 64 i32.load8_u if unreachable end
        i32.const 68 i32.load i32.const 0 i32.const 80 call @[method]inventory-image.slot-count@
        i32.const 80 i32.load8_u if unreachable end
        i32.const 84 i32.load i32.const 36 i32.ne if unreachable end
        i32.const 68 i32.load call @inventory-image_drop@
        {}
        i32.const 1100 i32.const 120 i32.store8
        local.get 0 local.get 1 i32.const 1 i32.const 1100 i32.const 1 i32.const 96 call @set-action-bar@
        i32.const 96 i32.load8_u if unreachable end
        {} i32.const 0
    "#,
        velocity(0.5),
        witness()
    );
    let mut h = Harness::with_capabilities(
        &[("on-player-chat", body)],
        true,
        &[
            PluginCapability::ReadInventory,
            PluginCapability::Presentation,
        ],
    );
    let before = crate::server::game::inventory_effects::InventoryState::capture(
        h.server.ecs.world(),
        h.player,
    )
    .unwrap();
    h.server
        .ecs
        .world_mut()
        .resource_mut::<EventBuffer>()
        .push(ServerEvent::PlayerChat {
            entity: h.player,
            player_id: "Player".into(),
            message: "handled".into(),
        });
    h.manager
        .lock()
        .unwrap()
        .drain_world_events(h.server.ecs.world_mut());
    h.apply();
    h.manager
        .lock()
        .unwrap()
        .publish_ui(h.server.ecs.world_mut());
    assert_eq!(h.logs(), ["x"]);
    assert_eq!(
        crate::server::game::inventory_effects::InventoryState::capture(
            h.server.ecs.world(),
            h.player
        )
        .unwrap(),
        before
    );
    assert_eq!(
        h.server.ecs.world().get::<Velocity>(h.player).unwrap().0.x,
        0.5
    );
    assert_eq!(
        h.manager
            .lock()
            .unwrap()
            .plugins
            .values()
            .next()
            .unwrap()
            .store
            .data()
            .test_item_bytes(),
        0
    );
    let packets = std::iter::from_fn(|| h.packets.try_recv().ok()).collect::<Vec<_>>();
    assert!(packets.iter().any(|packet|matches!(&packet.data,McpePacketData::SetTitlePacket(value) if &*value.title_text=="x")));
    assert!(
        packets
            .iter()
            .any(|packet| matches!(&packet.data, McpePacketData::SetActorMotionPacket(_)))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_damage_reply_discards_explicit_motion_and_reply_through_callback_failure() {
    let (player, _) = fixture::field_layout("on-player-damage", "player", &[]);
    let (tuning, size) = fixture::field_layout("on-player-damage", "proposal", &["tuning"]);
    let body = format!(
        "{} i32.const 1100 i32.const 120 i32.store8 local.get 0 i32.load offset={player} i32.const 1100 i32.const 1 i32.const 80 call @[method]player.message@ i32.const 80 i32.load8_u if unreachable end i32.const 512 i32.const 1 i32.store8 i32.const 520 local.get 0 i32.const {tuning} i32.add i32.const {size} memory.copy i32.const 520 f32.const nan f32.store i32.const 512",
        velocity(1.0).replace(
            "local.get 1",
            &format!("local.get 0 i32.load offset={player}")
        )
    );
    let mut h = Harness::configured(&[("on-player-damage", body)], true, &[], false);
    let lost = crate::entity::player_life::hurt_player(
        h.server.ecs.world_mut(),
        h.player,
        &crate::entity::mob::combat::Hit {
            damage: 2.0,
            cause: unastar_data::actor_components::body::ActorDamageCause::EntityAttack,
            attacker: None,
            by_player: true,
            knockback_from: None,
            knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
        },
    );
    assert_eq!(lost, 0.0);
    h.apply();
    assert_eq!(
        h.server
            .ecs
            .world()
            .get::<Health>(h.player)
            .unwrap()
            .current,
        20.0
    );
    assert!(h.server.ecs.world().get::<Velocity>(h.player).is_none());
    assert!(h.packets.try_recv().is_err());
    assert!(h.manager.lock().unwrap().readiness.check().is_ok());
}
