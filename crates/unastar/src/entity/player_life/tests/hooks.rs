//! Lifecycle extension regressions through the live player state and packets.

use super::*;
use crate::entity::components::{Hunger, Player};
use crate::entity::mob::combat::HurtTimer;
use crate::entity::mob::state::LastHurt;
use crate::entity::player_life::hooks::{
    PlayerDamage, PlayerDeath, PlayerLifecycleHandler, PlayerLifecycleHooks, PlayerRespawn,
    PlayerRespawned,
};
use crate::entity::player_life::respawn::{self, RespawnQueue, RespawnRequest};

/// Committed lifecycle notifications captured by an embedding application.
#[derive(Resource, Default)]
struct Notifications {
    deaths: Vec<(Entity, Option<Entity>, bool)>,
    respawns: Vec<(Entity, DVec3, bool)>,
}

/// Capture the state visible at committed lifecycle boundaries.
struct RecordLifecycle;

impl PlayerLifecycleHandler for RecordLifecycle {
    fn on_death(&self, world: &mut World, event: &PlayerDeath) {
        let dead = world
            .get::<crate::entity::player_life::Dead>(event.player)
            .is_some();
        world.resource_mut::<Notifications>().deaths.push((
            event.player,
            event.cause.attacker,
            dead,
        ));
    }

    fn on_respawned(&self, world: &mut World, event: &PlayerRespawned) {
        let alive = world
            .get::<crate::entity::player_life::Dead>(event.player)
            .is_none()
            && world
                .get::<Health>(event.player)
                .is_some_and(|health| health.current == health.max)
            && world
                .get::<Position>(event.player)
                .is_some_and(|position| position.0 == event.position);
        world
            .resource_mut::<Notifications>()
            .respawns
            .push((event.player, event.position, alive));
    }
}

/// Kill the subject and discard the ordinary death screen packets.
fn kill(h: &mut Harness) -> Entity {
    let cow = h.cow();
    h.hit_from(cow, 100.0);
    assert!(h.dead());
    h.drain();
    cow
}

#[test]
fn cancelled_damage_changes_no_health_timer_food_or_packets() {
    let mut h = harness(Difficulty::Normal);
    h.world()
        .add_observer(|mut event: On<PlayerDamage>| event.cancel());
    let cow = h.cow();
    let player = h.player;
    let hunger = h.world().get::<Hunger>(player).unwrap().clone();
    assert_eq!(h.hit_from(cow, 100.0), 0.0);
    assert_eq!(h.health(), 20.0);
    assert!(!h.dead());
    assert!(h.world().get::<HurtTimer>(player).is_none());
    assert!(h.world().get::<LastHurt>(player).is_none());
    let after = h.world().get::<Hunger>(player).unwrap();
    assert_eq!(
        (after.food_level, after.saturation, after.exhaustion),
        (hunger.food_level, hunger.saturation, hunger.exhaustion)
    );
    assert!(h.drain().is_empty());
}

#[test]
fn an_observer_can_reduce_damage_and_remove_knockback() {
    let mut h = harness(Difficulty::Normal);
    h.world().add_observer(|mut event: On<PlayerDamage>| {
        event.hit.damage = 4.0;
        event.hit.knockback_from = None;
    });
    let cow = h.cow();
    assert_eq!(h.hit_from(cow, 100.0), 4.0);
    assert_eq!(h.health(), 16.0);
    assert!(!h.dead());
    let packets = h.drain();
    assert!(actor_events(&packets, SESSION).contains(&EnumsActorEvent::Hurt));
    assert!(motions(&packets).is_empty());
}

#[test]
fn non_finite_adjusted_damage_and_knockback_are_rejected() {
    for value in [f32::NAN, f32::INFINITY, -1.0] {
        let mut h = harness(Difficulty::Normal);
        h.world()
            .add_observer(move |mut event: On<PlayerDamage>| event.hit.damage = value);
        let cow = h.cow();
        assert_eq!(h.hit_from(cow, 4.0), 0.0);
        assert_eq!(h.health(), 20.0);
        assert!(h.drain().is_empty());
    }
    let mut h = harness(Difficulty::Normal);
    h.world().add_observer(|mut event: On<PlayerDamage>| {
        event.hit.knockback_power.horizontal = f64::INFINITY;
    });
    let cow = h.cow();
    assert_eq!(h.hit_from(cow, 4.0), 0.0);
    assert_eq!(h.health(), 20.0);
    assert!(h.drain().is_empty());
}

/// Remove the subject during a callback to exercise post-callback validation.
struct RemoveSubject;

impl PlayerLifecycleHandler for RemoveSubject {
    fn on_damage(&self, world: &mut World, event: &mut PlayerDamage) {
        world.despawn(event.player());
    }
}

#[test]
fn a_handler_may_remove_the_subject_without_committing_a_stale_hit() {
    let mut h = harness(Difficulty::Normal);
    h.world()
        .resource_mut::<PlayerLifecycleHooks>()
        .register(RemoveSubject);
    let cow = h.cow();
    assert_eq!(h.hit_from(cow, 4.0), 0.0);
    let player = h.player;
    assert!(h.world().get::<Player>(player).is_none());
    assert!(h.drain().is_empty());
}

/// Prevent one hit while keeping later listeners available to inspect it.
struct CancelDamage;

impl PlayerLifecycleHandler for CancelDamage {
    fn on_damage(&self, _world: &mut World, event: &mut PlayerDamage) {
        event.cancel();
    }
}

#[test]
fn handler_cancellation_remains_set_for_later_observers() {
    let mut h = harness(Difficulty::Normal);
    h.world()
        .resource_mut::<PlayerLifecycleHooks>()
        .register(CancelDamage);
    h.world().add_observer(|mut event: On<PlayerDamage>| {
        assert!(event.is_cancelled());
        event.hit.damage = 1.0;
    });
    let cow = h.cow();
    assert_eq!(h.hit_from(cow, 4.0), 0.0);
    assert_eq!(h.health(), 20.0);
}

#[test]
fn death_notifies_once_with_committed_state_and_credited_attacker() {
    let mut h = harness(Difficulty::Normal);
    h.world().init_resource::<Notifications>();
    h.world()
        .resource_mut::<PlayerLifecycleHooks>()
        .register(RecordLifecycle);
    let cow = kill(&mut h);
    assert_eq!(h.hit_from(cow, 100.0), 0.0);
    let player = h.player;
    assert_eq!(
        h.world().resource::<Notifications>().deaths,
        [(player, Some(cow), true)]
    );
}

#[test]
fn cancelled_client_ready_respawn_stays_dead_without_acknowledgement() {
    let mut h = harness(Difficulty::Normal);
    kill(&mut h);
    h.world()
        .add_observer(|mut event: On<PlayerRespawn>| event.cancel());
    let player = h.player;
    let position = h.world().get::<Position>(player).unwrap().0;
    h.world()
        .resource_mut::<RespawnQueue>()
        .0
        .push((player, RespawnRequest::ClientReady));
    h.tick();
    assert!(h.dead());
    assert_eq!(h.health(), 0.0);
    assert_eq!(h.world().get::<Position>(player).unwrap().0, position);
    let packets = h.drain();
    assert!(respawns(&packets).is_empty());
    assert!(!actor_events(&packets, SESSION).contains(&EnumsActorEvent::SpawnAlive));
    assert!(
        !packets
            .iter()
            .any(|packet| matches!(packet, McpePacketData::MovePlayerPacket(_)))
    );
}

#[test]
fn a_respawn_destination_override_is_committed_and_reported_once() {
    let mut h = harness(Difficulty::Normal);
    h.world().init_resource::<Notifications>();
    h.world()
        .resource_mut::<PlayerLifecycleHooks>()
        .register(RecordLifecycle);
    kill(&mut h);
    let destination = eyes(HOME + DVec3::X * 3.0);
    h.world()
        .add_observer(move |mut event: On<PlayerRespawn>| event.position = destination);
    let player = h.player;
    h.world()
        .resource_mut::<RespawnQueue>()
        .0
        .push((player, RespawnRequest::ClientReady));
    h.tick();
    assert!(!h.dead());
    assert_eq!(h.world().get::<Position>(player).unwrap().0, destination);
    assert_eq!(
        respawns(&h.drain()),
        [(
            EnumsPlayerRespawnState::Readytospawn,
            destination.as_vec3().as_dvec3()
        )]
    );
    respawn::respawn(h.world(), player);
    assert_eq!(
        h.world().resource::<Notifications>().respawns,
        [(player, destination, true)]
    );
}

#[test]
fn invalid_respawn_destinations_keep_the_player_dead_without_packets() {
    for position in [DVec3::splat(f64::NAN), DVec3::splat(f64::MAX)] {
        let mut h = harness(Difficulty::Normal);
        kill(&mut h);
        h.world()
            .add_observer(move |mut event: On<PlayerRespawn>| event.position = position);
        let player = h.player;
        respawn::respawn(h.world(), player);
        assert!(h.dead());
        assert_eq!(h.health(), 0.0);
        assert!(h.drain().is_empty());
    }
}

#[test]
fn immediate_respawn_also_respects_the_veto() {
    let mut h = harness(Difficulty::Normal);
    h.rules().do_immediate_respawn = true;
    h.world()
        .add_observer(|mut event: On<PlayerRespawn>| event.cancel());
    let cow = h.cow();
    h.hit_from(cow, 100.0);
    assert!(h.dead());
    let packets = h.drain();
    assert!(!actor_events(&packets, SESSION).contains(&EnumsActorEvent::SpawnAlive));
    assert!(
        !packets
            .iter()
            .any(|packet| matches!(packet, McpePacketData::MovePlayerPacket(_)))
    );
}

/// Dispatch a second player's hit from inside the first player's callback.
struct NestedDamage {
    first: Entity,
    second: Entity,
}

impl PlayerLifecycleHandler for NestedDamage {
    fn on_damage(&self, world: &mut World, event: &mut PlayerDamage) {
        if event.player() == self.first {
            crate::entity::player_life::hurt_player(world, self.second, &event.hit);
        }
    }
}

#[test]
fn native_handlers_can_dispatch_nested_events_without_borrowing_the_registry() {
    let mut h = harness(Difficulty::Normal);
    let second = h.viewer().entity;
    let first = h.player;
    h.world()
        .resource_mut::<PlayerLifecycleHooks>()
        .register(NestedDamage { first, second });
    let cow = h.cow();
    assert_eq!(h.hit_from(cow, 4.0), 4.0);
    assert_eq!(h.health(), 16.0);
    assert_eq!(h.world().get::<Health>(second).unwrap().current, 16.0);
}

/// Remove the player while evaluating its proposed respawn.
struct RemoveRespawningSubject;

impl PlayerLifecycleHandler for RemoveRespawningSubject {
    fn on_respawn(&self, world: &mut World, event: &mut PlayerRespawn) {
        world.despawn(event.player());
    }
}

#[test]
fn removing_the_player_in_a_respawn_handler_sends_no_stale_packets() {
    let mut h = harness(Difficulty::Normal);
    kill(&mut h);
    h.world()
        .resource_mut::<PlayerLifecycleHooks>()
        .register(RemoveRespawningSubject);
    let player = h.player;
    h.world()
        .resource_mut::<RespawnQueue>()
        .0
        .push((player, RespawnRequest::ClientReady));
    h.tick();
    assert!(h.world().get::<Player>(player).is_none());
    assert!(respawns(&h.drain()).is_empty());
}

#[test]
fn kill_reports_committed_death_even_when_immediately_respawned() {
    let mut h = harness(Difficulty::Normal);
    h.world().resource_mut::<GameRules>().do_immediate_respawn = true;
    let player = h.player;
    assert!(crate::entity::player_life::kill_player(h.world(), player));
    assert!(!h.dead());
    assert_eq!(h.health(), 20.0);
}

#[test]
fn kill_reports_false_when_damage_is_cancelled_or_reduced_to_nonlethal() {
    let mut h = harness(Difficulty::Normal);
    h.world()
        .add_observer(|mut event: On<PlayerDamage>| event.cancel());
    let player = h.player;
    assert!(!crate::entity::player_life::kill_player(h.world(), player));
    assert_eq!(h.health(), 20.0);

    let mut h = harness(Difficulty::Normal);
    h.world()
        .add_observer(|mut event: On<PlayerDamage>| event.hit.damage = 1.0);
    let player = h.player;
    assert!(!crate::entity::player_life::kill_player(h.world(), player));
    assert_eq!(h.health(), 19.0);
}

/// Preserve notification order and the state visible after health loss.
#[derive(Resource, Default)]
struct DamageNotifications(Vec<(&'static str, f32, f32)>);

/// Record actual damage separately from attempts and eventual death.
struct RecordDamage;

impl PlayerLifecycleHandler for RecordDamage {
    fn on_damaged(
        &self,
        world: &mut World,
        event: &crate::entity::player_life::hooks::PlayerDamaged,
    ) {
        let health = world.get::<Health>(event.player).unwrap().current;
        assert!(
            world
                .get::<crate::entity::player_life::Dead>(event.player)
                .is_none()
        );
        world
            .resource_mut::<DamageNotifications>()
            .0
            .push(("damage", event.damage, health));
    }

    fn on_death(&self, world: &mut World, event: &PlayerDeath) {
        assert!(
            world
                .get::<crate::entity::player_life::Dead>(event.player)
                .is_some()
        );
        world
            .resource_mut::<DamageNotifications>()
            .0
            .push(("death", 0.0, 0.0));
    }
}

#[test]
fn committed_damage_notifications_exclude_cancelled_and_suppressed_hits_and_precede_death() {
    let mut h = harness(Difficulty::Normal);
    h.world().init_resource::<DamageNotifications>();
    h.world()
        .resource_mut::<PlayerLifecycleHooks>()
        .register(RecordDamage);
    let cow = h.cow();
    assert_eq!(h.hit_from(cow, 4.0), 4.0);
    assert_eq!(h.hit_from(cow, 4.0), 0.0);
    assert_eq!(
        h.world().resource::<DamageNotifications>().0,
        vec![("damage", 4.0, 16.0)]
    );
    assert_eq!(h.hit_from(cow, 100.0), 16.0);
    assert_eq!(
        h.world().resource::<DamageNotifications>().0,
        vec![
            ("damage", 4.0, 16.0),
            ("damage", 16.0, 0.0),
            ("death", 0.0, 0.0)
        ]
    );

    let mut h = harness(Difficulty::Normal);
    h.world().init_resource::<DamageNotifications>();
    h.world()
        .resource_mut::<PlayerLifecycleHooks>()
        .register(RecordDamage);
    h.world()
        .add_observer(|mut event: On<PlayerDamage>| event.cancel());
    let cow = h.cow();
    assert_eq!(h.hit_from(cow, 100.0), 0.0);
    assert!(h.world().resource::<DamageNotifications>().0.is_empty());
}

#[test]
fn native_armour_override_preserves_hurt_timer_cause_attribution_and_wear() {
    let mut ordinary = harness(Difficulty::Normal);
    let player = ordinary.player;
    ordinary
        .world()
        .get_mut::<crate::entity::components::ArmourInventory>(player)
        .unwrap()
        .set_chestplate(crate::item::test_support::stack(
            "minecraft:iron_chestplate",
            1,
        ))
        .unwrap();
    let cow = ordinary.cow();
    assert!(ordinary.hit_from(cow, 4.0) < 4.0);
    let mut h = harness(Difficulty::Normal);
    let player = h.player;
    h.world()
        .get_mut::<crate::entity::components::ArmourInventory>(player)
        .unwrap()
        .set_chestplate(crate::item::test_support::stack(
            "minecraft:iron_chestplate",
            1,
        ))
        .unwrap();
    h.world()
        .add_observer(|mut event: On<PlayerDamage>| event.bypass_armour = true);
    let cow = h.cow();
    assert_eq!(h.hit_from(cow, 4.0), 4.0);
    assert_eq!(h.hit_from(cow, 4.0), 0.0);
    let hurt = h.world().get::<LastHurt>(player).unwrap();
    assert_eq!(
        (hurt.cause, hurt.attacker),
        (ActorDamageCause::EntityAttack, Some(cow))
    );
    let chest = h
        .world()
        .get::<crate::entity::components::ArmourInventory>(player)
        .unwrap()
        .chestplate()
        .clone();
    assert_eq!(crate::item::enchant::durability_damage(&chest), 1);
}

#[test]
fn deferred_observer_despawn_cannot_commit_a_stale_damage_attempt() {
    let mut h = harness(Difficulty::Normal);
    h.world()
        .add_observer(|event: On<PlayerDamage>, mut commands: Commands| {
            commands.entity(event.player()).despawn();
        });
    let cow = h.cow();
    assert_eq!(h.hit_from(cow, 100.0), 0.0);
    assert!(h.drain().is_empty());
}

#[test]
fn final_damage_multiplier_runs_after_armour_and_rejects_invalid_values_before_effects() {
    let mut h = harness(Difficulty::Normal);
    let player = h.player;
    h.world()
        .get_mut::<crate::entity::components::ArmourInventory>(player)
        .unwrap()
        .set_chestplate(crate::item::test_support::stack(
            "minecraft:iron_chestplate",
            1,
        ))
        .unwrap();
    h.world()
        .add_observer(|mut event: On<PlayerDamage>| event.post_armour_multiplier = 0.5);
    let cow = h.cow();
    let expected = unastar_data::armour::damage_after_armour(4.0, 6.0, 0.0) * 0.5;
    assert!((h.hit_from(cow, 4.0) - expected).abs() < 1.0e-4);
    for multiplier in [f32::NAN, f32::INFINITY, -1.0] {
        let mut h = harness(Difficulty::Normal);
        h.world().add_observer(move |mut event: On<PlayerDamage>| {
            event.post_armour_multiplier = multiplier
        });
        let cow = h.cow();
        assert_eq!(h.hit_from(cow, 4.0), 0.0);
        assert!(h.drain().is_empty());
        assert_eq!(h.health(), 20.0);
    }
}

/// Retain owned death facts after their source actor has changed or respawned.
#[derive(Resource, Default)]
struct DeathImages(Vec<PlayerDeath>);

/// Install the same immutable notification path available to native applications.
fn record_death_images(h: &mut Harness) {
    h.world().init_resource::<DeathImages>();
    h.world()
        .add_observer(|event: On<PlayerDeath>, mut images: ResMut<DeathImages>| {
            images.0.push(event.event().clone());
        });
}

#[test]
fn committed_death_inventory_precedes_drops_and_remains_owned_after_respawn() {
    use crate::entity::components::{
        ArmourInventory, HeldSlot, MainInventory, OffhandSlot, PlayerUi,
    };
    use crate::item::{enchant, test_support::stack};
    let mut h = harness(Difficulty::Normal);
    record_death_images(&mut h);
    let player = h.player;
    let sword = enchant::with_enchantment_ids(
        &stack("minecraft:diamond_sword", 1).with_damage(12),
        [(9, 2)],
    );
    h.world()
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(3, sword.clone())
        .unwrap();
    h.world()
        .get_mut::<ArmourInventory>(player)
        .unwrap()
        .set_helmet(stack("minecraft:iron_helmet", 1))
        .unwrap();
    h.world().get_mut::<OffhandSlot>(player).unwrap().0 = stack("minecraft:shield", 1);
    h.world()
        .get_mut::<PlayerUi>(player)
        .unwrap()
        .set_cursor(stack("minecraft:emerald", 5));
    h.world().get_mut::<HeldSlot>(player).unwrap().0 = 3;
    assert!(super::super::kill_player(h.world(), player));
    assert_eq!(h.world().resource::<DeathImages>().0.len(), 1);
    let event = h.world().resource::<DeathImages>().0[0].clone();
    assert!(!event.keep_inventory);
    let image = event.inventory.as_ref().unwrap();
    assert_eq!(image.main.len(), 36);
    assert_eq!(image.main[3].item_id(), sword.item_id());
    assert_eq!(image.main[3].damage, 12);
    assert_eq!(image.main[3].nbt, sword.nbt);
    assert_eq!(image.armour[0].item_id(), "minecraft:iron_helmet");
    assert_eq!(image.offhand.item_id(), "minecraft:shield");
    assert_eq!(image.ui.len(), 54);
    assert_eq!(image.ui[0].count, 5);
    assert_eq!(image.held_slot, 3);
    assert!(
        h.world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(3)
            .unwrap()
            .is_empty()
    );
    assert!(!super::super::kill_player(h.world(), player));
    assert_eq!(h.world().resource::<DeathImages>().0.len(), 1);
    respawn::respawn(h.world(), player);
    assert!(!h.dead());
    h.world()
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(3, stack("minecraft:gold_ingot", 2))
        .unwrap();
    assert_eq!(
        event.inventory.unwrap().main[3].item_id(),
        "minecraft:diamond_sword"
    );
}

#[test]
fn kept_inventory_and_immediate_respawn_preserve_the_same_committed_death_image() {
    use crate::entity::components::MainInventory;
    use crate::item::test_support::stack;
    let mut h = harness(Difficulty::Normal);
    record_death_images(&mut h);
    h.rules().keep_inventory = true;
    h.rules().do_immediate_respawn = true;
    let player = h.player;
    h.world()
        .get_mut::<MainInventory>(player)
        .unwrap()
        .0
        .set_item(0, stack("minecraft:diamond", 3))
        .unwrap();
    assert!(super::super::kill_player(h.world(), player));
    assert!(!h.dead());
    assert!(h.drops().is_empty());
    let event = &h.world().resource::<DeathImages>().0[0];
    assert!(event.keep_inventory);
    assert_eq!(event.inventory.as_ref().unwrap().main[0].count, 3);
    assert_eq!(
        h.world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .unwrap()
            .count,
        3
    );
}

#[test]
fn incomplete_source_inventory_is_reported_without_fabricating_a_partial_image() {
    use crate::entity::components::{PlayerSession, PlayerUi};
    let mut h = harness(Difficulty::Normal);
    record_death_images(&mut h);
    let player = h.player;
    h.world()
        .entity_mut(player)
        .remove::<(PlayerUi, PlayerSession)>();
    assert!(super::super::kill_player(h.world(), player));
    assert!(h.world().resource::<DeathImages>().0[0].inventory.is_none());
}
