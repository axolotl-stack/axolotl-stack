//! Real-component decisions exercised through the ordinary player lifecycle.

use super::super::tests::{component_with_tick, engine};
use super::super::{PLUGIN_API_VERSION, PendingPlugin, create_store};
use super::*;
use crate::ecs::events::{ActionQueue, EventBuffer};
use crate::ecs::resources::TickCounter;
use crate::entity::components::{
    GameMode, Health, Hunger, PlayerName, PlayerSession, Position, Rotation, RuntimeEntityId,
};
use crate::entity::mob::combat::{Hit, HurtTimer};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::state::LastHurt;
use crate::entity::player_life::{self, Dead};
use crate::plugin::manifest::{PluginId, PluginLimits, PluginManifest};
use crate::world::rules::{Difficulty, GameRules};
use tokio::sync::mpsc;
use wasmtime_wasi::WasiCtx;

/// Decision body that preserves the incoming hit's damage.
const KEEP_DAMAGE: &str =
    "i32.const 32 i32.const 1 i32.store i32.const 36 local.get 4 f32.store i32.const 32";
/// Decision body that keeps the proposed respawn destination.
const KEEP_POSITION: &str = "i32.const 64 i32.const 1 i32.store i32.const 72 local.get 1 f64.store i32.const 80 local.get 2 f64.store i32.const 88 local.get 3 f64.store i32.const 64";
/// Return no decision value, vetoing an operation.
const CANCEL: &str = "i32.const 32 i32.const 0 i32.store i32.const 32";

/// Subject, live world and the component manager used by its native bridge.
struct Harness {
    world: World,
    player: Entity,
    manager: Arc<Mutex<PluginManager>>,
    packets: mpsc::Receiver<jolyne::valentine::McpePacket>,
}

impl Harness {
    /// Load one real component into a minimal live player world.
    fn new(subscriptions: &[EventKind], damage_body: &str, respawn_body: &str) -> Self {
        Self::with_tick(subscriptions, damage_body, respawn_body, "")
    }

    /// Load a component with a separately controlled buffered tick callback.
    fn with_tick(
        subscriptions: &[EventKind],
        damage_body: &str,
        respawn_body: &str,
        tick_body: &str,
    ) -> Self {
        let engine = engine();
        let manifest = PluginManifest {
            id: PluginId("lifecycle".into()),
            name: "Lifecycle test".into(),
            version: "1".into(),
            api_version: PLUGIN_API_VERSION.into(),
            capabilities: Default::default(),
            subscriptions: subscriptions.iter().copied().collect(),
            limits: PluginLimits::default(),
        };
        let pending = PendingPlugin {
            component: component_with_tick(&engine, true, damage_body, respawn_body, tick_body),
            store: create_store(
                &engine,
                WasiCtx::builder().build(),
                manifest.id.clone(),
                &manifest.limits,
            )
            .unwrap(),
            manifest,
        };
        let manager = Arc::new(Mutex::new(PluginManager {
            engine,
            plugins: Default::default(),
            pending: vec![pending],
            tick_prepared: false,
            tick_budget_valid: true,
        }));
        let mut world = World::new();
        world.insert_resource(TickCounter { current: 42 });
        world.insert_resource(ActionQueue::default());
        world.insert_resource(EventBuffer::default());
        world.insert_resource(Difficulty::Normal);
        world.insert_resource(GameRules::default());
        world.resource_mut::<GameRules>().keep_inventory = true;
        world.resource_mut::<GameRules>().show_death_messages = false;
        player_life::install(&mut world, DVec3::new(1.0, 70.0, 1.0)).unwrap();
        LifecycleBridge::install(Arc::clone(&manager), &mut world);
        let (tx, packets) = mpsc::channel(128);
        let player = world
            .spawn((
                Player,
                PlayerName("Alice".into()),
                PlayerSession::new(7, "Alice".into(), None, None, tx),
                RuntimeEntityId(7),
                Position(DVec3::new(2.0, 70.0, 2.0)),
                Rotation::default(),
                GameMode::Survival,
                Health::default(),
                Hunger::default(),
            ))
            .id();
        Self {
            world,
            player,
            manager,
            packets,
        }
    }

    /// Apply an ordinary incoming hit with an optional player attacker.
    fn hit(&mut self, damage: f32, attacker: Option<Entity>) -> f32 {
        player_life::hurt_player(
            &mut self.world,
            self.player,
            &Hit {
                damage,
                cause: ActorDamageCause::EntityAttack,
                attacker,
                by_player: attacker.is_some(),
                knockback_from: None,
                knockback_power: KnockbackPower::DEFAULT,
            },
        )
    }

    /// Remaining instructions in the loaded component's current tick budget.
    fn fuel(&self) -> u64 {
        self.manager
            .lock()
            .unwrap()
            .plugins
            .values()
            .next()
            .unwrap()
            .store
            .get_fuel()
            .unwrap()
    }

    /// Discard packets from the preceding committed operation.
    fn drain(&mut self) {
        while self.packets.try_recv().is_ok() {}
    }
}

#[test]
fn damage_veto_traps_and_invalid_results_leave_player_state_unchanged() {
    for body in [
        CANCEL,
        "unreachable",
        "i32.const 32 i32.const 1 i32.store i32.const 36 f32.const nan f32.store i32.const 32",
    ] {
        let mut h = Harness::new(&[EventKind::PlayerDamage], body, KEEP_POSITION);
        assert_eq!(h.hit(100.0, None), 0.0);
        assert_eq!(h.world.get::<Health>(h.player).unwrap().current, 20.0);
        assert!(h.world.get::<Dead>(h.player).is_none());
        assert!(h.world.get::<HurtTimer>(h.player).is_none());
        assert!(h.world.get::<LastHurt>(h.player).is_none());
        assert_eq!(h.world.get::<Hunger>(h.player).unwrap().exhaustion, 0.0);
        assert!(h.packets.try_recv().is_err());
        assert!(h.manager.lock().unwrap().pending.is_empty());
    }
}

#[test]
fn damage_callback_changes_the_hit_and_receives_a_player_attacker() {
    let body = "local.get 1 i32.const 1 i32.ne if unreachable end i32.const 32 i32.const 1 i32.store i32.const 36 f32.const 3 f32.store i32.const 32";
    let mut h = Harness::new(&[EventKind::PlayerDamage], body, KEEP_POSITION);
    let attacker = h.world.spawn(Player).id();
    assert_eq!(h.hit(100.0, Some(attacker)), 3.0);
    assert_eq!(h.world.get::<Health>(h.player).unwrap().current, 17.0);
    assert!(h.world.get::<Dead>(h.player).is_none());
}

#[test]
fn death_and_respawn_notifications_run_once_after_each_commit() {
    let mut h = Harness::new(
        &[EventKind::PlayerDeath, EventKind::PlayerRespawned],
        KEEP_DAMAGE,
        KEEP_POSITION,
    );
    h.manager.lock().unwrap().begin_tick(&mut h.world);
    let initial = h.fuel();
    h.hit(100.0, None);
    assert!(h.world.get::<Dead>(h.player).is_some());
    let after_death = h.fuel();
    assert!(after_death < initial);
    assert!(!player_life::kill_player(&mut h.world, h.player));
    assert_eq!(h.fuel(), after_death);
    player_life::respawn::respawn(&mut h.world, h.player);
    assert!(h.world.get::<Dead>(h.player).is_none());
    let after_respawn = h.fuel();
    assert!(after_respawn < after_death);
    player_life::respawn::respawn(&mut h.world, h.player);
    assert_eq!(h.fuel(), after_respawn);
}

#[test]
fn respawn_callback_can_veto_or_choose_the_destination() {
    let redirect = "i32.const 64 i32.const 1 i32.store i32.const 72 f64.const 7 f64.store i32.const 80 f64.const 80 f64.store i32.const 88 f64.const -4 f64.store i32.const 64";
    for (body, allowed) in [(CANCEL, false), ("unreachable", false), (redirect, true)] {
        let mut h = Harness::new(&[EventKind::PlayerRespawn], KEEP_DAMAGE, body);
        h.hit(100.0, None);
        h.drain();
        let old = h.world.get::<Position>(h.player).unwrap().0;
        player_life::respawn::respawn(&mut h.world, h.player);
        if allowed {
            assert!(h.world.get::<Dead>(h.player).is_none());
            assert_eq!(h.world.get::<Health>(h.player).unwrap().current, 20.0);
            assert_eq!(
                h.world.get::<Position>(h.player).unwrap().0,
                DVec3::new(7.0, 80.0, -4.0)
            );
        } else {
            assert!(h.world.get::<Dead>(h.player).is_some());
            assert_eq!(h.world.get::<Health>(h.player).unwrap().current, 0.0);
            assert_eq!(h.world.get::<Position>(h.player).unwrap().0, old);
            assert!(h.packets.try_recv().is_err());
        }
    }
}

#[test]
fn synchronous_and_buffered_callbacks_share_fuel_until_the_next_tick() {
    let body =
        format!("global.get $damage-calls i32.const 1 i32.eq if (loop br 0) end {KEEP_DAMAGE}");
    let mut h = Harness::new(
        &[EventKind::PlayerDamage, EventKind::Tick],
        &body,
        KEEP_POSITION,
    );
    assert_eq!(h.hit(2.0, None), 0.0);
    assert_eq!(h.fuel(), 0);
    h.manager.lock().unwrap().begin_tick(&mut h.world);
    assert_eq!(h.fuel(), 0);
    h.manager.lock().unwrap().tick(&mut h.world);
    assert_eq!(h.fuel(), 0);
    h.manager.lock().unwrap().begin_tick(&mut h.world);
    assert!(h.fuel() > 0);
    // Renewing fuel does not recreate a component instance invalidated by a trap.
    assert_eq!(h.hit(2.0, None), 0.0);
    assert_eq!(h.world.get::<Health>(h.player).unwrap().current, 20.0);
}

#[test]
fn manager_contention_fails_closed_without_waiting_or_mutating_the_player() {
    let mut h = Harness::new(&[], KEEP_DAMAGE, KEEP_POSITION);
    let manager = Arc::clone(&h.manager);
    let _held = manager.lock().unwrap();
    assert_eq!(h.hit(100.0, None), 0.0);
    assert_eq!(h.world.get::<Health>(h.player).unwrap().current, 20.0);
    assert!(h.packets.try_recv().is_err());
}

#[test]
fn old_api_manifest_is_rejected_before_reading_or_compiling_wasm() {
    let mut manager = PluginManager {
        engine: engine(),
        plugins: Default::default(),
        pending: Vec::new(),
        tick_prepared: false,
        tick_budget_valid: true,
    };
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("plugin.toml"),
        r#"
        id = "old"
        name = "Old component"
        version = "1"
        api_version = "0.2.0"
    "#,
    )
    .unwrap();
    let error = manager
        .load_plugin(directory.path())
        .unwrap_err()
        .to_string();
    assert!(error.contains("Plugin API 0.2.0 is unsupported"));
    assert!(manager.pending.is_empty());
}

#[test]
fn trapped_buffered_callbacks_discard_partial_host_actions_after_lifecycle_calls() {
    let mut h = Harness::with_tick(
        &[EventKind::PlayerDamage, EventKind::Tick],
        KEEP_DAMAGE,
        KEEP_POSITION,
        "i32.const 1 i32.const 2 i32.const 3 i32.const 4 call $set-block unreachable",
    );
    assert_eq!(h.hit(2.0, None), 2.0);
    h.manager.lock().unwrap().tick(&mut h.world);
    assert!(h.world.resource_mut::<ActionQueue>().drain().is_empty());
}
