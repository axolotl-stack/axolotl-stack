use super::*;
use crate::ecs::resources::TickCounter;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::combat::{fire, melee};
use crate::entity::mob::gear::{GearSlot, MobEquipment};
use crate::entity::player_life::hooks::{
    PlayerDamage, PlayerLifecycleHandler, PlayerLifecycleHooks,
};
use crate::item::enchant::{Enchantment, with_enchantments};
use crate::server::game::PlayerSpawnData;

/// An authoritative stack; Fire Aspect does not change its ordinary attack damage.
fn weapon(item: &str, level: u8) -> ItemStack {
    with_enchantments(
        &crate::item::test_support::stack(item, 1),
        &[(Enchantment::FireAspect, level)],
    )
}

/// Replace the held stack during damage authorization to test captured facts.
struct SwapWeapon(ItemStack);
impl PlayerLifecycleHandler for SwapWeapon {
    fn on_damage(&self, world: &mut World, event: &mut PlayerDamage) {
        let attacker = event.hit.attacker.unwrap();
        if let Some(mut main) = world.get_mut::<MainInventory>(attacker) {
            main.0.set_item(0, self.0.clone()).unwrap();
        } else {
            world.get_mut::<MobEquipment>(attacker).unwrap().set(
                GearSlot::Mainhand,
                self.0.clone(),
                false,
            );
        }
    }
}

/// A complete survival target; its receiver remains owned by the calling test.
fn target_player(h: &mut Harness) -> (Entity, Receiver<McpePacket>) {
    let (tx, rx) = tokio::sync::mpsc::channel(65536);
    let target = h
        .server
        .spawn_player(PlayerSpawnData {
            session_id: 2,
            runtime_id: 2,
            display_name: "Target".into(),
            xuid: None,
            uuid: None,
            position: DVec3::new(6.5, 64.0 + crate::entity::PLAYER_EYE_HEIGHT, 8.5),
            outbound_tx: tx,
            saved_data: None,
            chunk_radius: 0,
        })
        .unwrap();
    h.world()
        .entity_mut(target)
        .insert((GameMode::Survival, Health::new(100.0)));
    (target, rx)
}

/// Stand within native reach using actual player eye or mob feet coordinates.
fn player_hit(h: &mut Harness, target: Entity) {
    let player = h.player;
    let world = h.world();
    let mut feet = world.get::<Position>(target).unwrap().0;
    if world.get::<Player>(target).is_some() {
        feet.y -= crate::entity::PLAYER_EYE_HEIGHT;
    }
    world.get_mut::<Position>(player).unwrap().0 =
        feet + DVec3::new(-2.0, crate::entity::PLAYER_EYE_HEIGHT, 0.0);
    player_attack(world, player, target, &Default::default());
}

#[test]
fn player_fire_aspect_uses_the_original_sword_during_damage_callbacks() {
    for original in [0, 2] {
        let mut h = harness(80);
        let (target, _rx) = target_player(&mut h);
        let player = h.player;
        h.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, weapon("minecraft:diamond_sword", original))
            .unwrap();
        h.world()
            .resource_mut::<PlayerLifecycleHooks>()
            .register(SwapWeapon(weapon("minecraft:diamond_sword", 2 - original)));
        let now = h.world().resource::<TickCounter>().current;
        player_hit(&mut h, target);
        assert!(h.world().get::<Health>(target).unwrap().current < 100.0);
        assert_eq!(
            h.world()
                .get::<crate::entity::player_life::environment::Burning>(target)
                .map(|b| b.until_tick),
            (original > 0).then_some(now + u64::from(original) * 80)
        );
        assert_eq!(
            crate::item::enchant::durability_damage(held_item(h.world(), player).unwrap()),
            0,
            "a replacement stack is not charged"
        );
    }
}

#[test]
fn player_swords_ignite_living_mobs_by_level_without_changing_damage() {
    for (item, level) in [
        ("minecraft:diamond_sword", 0),
        ("minecraft:diamond_sword", 1),
        ("minecraft:diamond_sword", 2),
        ("minecraft:diamond_axe", 2),
    ] {
        let mut h = harness(80);
        let target = h.cow(DVec3::new(6.5, 64.0, 8.5));
        h.world().entity_mut(target).insert(Health::new(100.0));
        let player = h.player;
        h.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, weapon(item, level))
            .unwrap();
        let now = h.world().resource::<TickCounter>().current;
        player_hit(&mut h, target);
        assert_eq!(
            100.0 - h.world().get::<Health>(target).unwrap().current,
            unastar_data::melee::attack_damage(Some(item))
        );
        let wanted = if item.ends_with("_sword") && level > 0 {
            Some(now + u64::from(level) * 80)
        } else {
            None
        };
        assert_eq!(
            h.world().get::<fire::Burning>(target).map(|b| b.until_tick),
            wanted
        );
    }
}

#[test]
fn rejected_player_melee_and_fire_immunity_never_ignite() {
    for mode in [
        "veto",
        "creative",
        "hurt-window",
        "fire-immune",
        "zero-loss",
    ] {
        let mut h = harness(80);
        let (target, _rx) = target_player(&mut h);
        let player = h.player;
        h.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, weapon("minecraft:diamond_sword", 2))
            .unwrap();
        match mode {
            "veto" => {
                h.world()
                    .add_observer(|mut event: On<PlayerDamage>| event.cancel());
            }
            "creative" => {
                h.world().entity_mut(target).insert(GameMode::Creative);
            }
            "hurt-window" => {
                h.world().entity_mut(target).insert(HurtTimer {
                    until_tick: 1000,
                    last_damage: 100.0,
                });
            }
            "fire-immune" => {
                h.world().entity_mut(target).insert(ActorFlags::default());
                h.world()
                    .get_mut::<ActorFlags>(target)
                    .unwrap()
                    .update(jolyne::valentine::actor_flags::FIRE_IMMUNE, true)
                    .unwrap();
            }
            "zero-loss" => {
                h.world()
                    .add_observer(|mut event: On<PlayerDamage>| event.post_armour_multiplier = 0.0);
            }
            _ => unreachable!(),
        }
        player_hit(&mut h, target);
        assert!(
            h.world()
                .get::<crate::entity::player_life::environment::Burning>(target)
                .is_none(),
            "{mode}"
        );
        let health = h.world().get::<Health>(target).unwrap().current;
        if mode == "fire-immune" {
            assert!(health < 100.0);
        } else {
            assert_eq!(health, 100.0, "{mode}");
        }
    }
}

#[test]
fn equipped_mob_swords_capture_fire_aspect_before_the_hit_callbacks() {
    for original in [0, 2] {
        let mut h = harness(80);
        let (target, _rx) = target_player(&mut h);
        let world = h.world();
        let attacker = spawn_mob(
            world,
            "minecraft:zombie",
            DVec3::new(4.5, 64.0, 8.5),
            0.0,
            SpawnCause::Spawned,
        )
        .unwrap();
        crate::entity::definition::apply_pending_definitions(world);
        assert!(
            world
                .get::<crate::entity::mob::components::MeleeBoxAttackGoal>(attacker)
                .is_some()
        );
        world
            .entity_mut(attacker)
            .entry::<MobEquipment>()
            .or_default()
            .get_mut()
            .set(
                GearSlot::Mainhand,
                weapon("minecraft:diamond_sword", original),
                false,
            );
        world
            .resource_mut::<PlayerLifecycleHooks>()
            .register(SwapWeapon(weapon("minecraft:diamond_sword", 2 - original)));
        let now = world.resource::<TickCounter>().current;
        melee::melee_attack(world, attacker, target);
        assert!(world.get::<Health>(target).unwrap().current < 100.0);
        assert_eq!(
            world
                .get::<crate::entity::player_life::environment::Burning>(target)
                .map(|b| b.until_tick),
            (original > 0).then_some(now + u64::from(original) * 80)
        );
    }
}

#[test]
fn accepted_fire_aspect_cannot_ignite_dead_or_fire_immune_mobs() {
    for immune in [false, true] {
        let mut h = harness(80);
        let target = h.cow(DVec3::new(6.5, 64.0, 8.5));
        let player = h.player;
        h.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, weapon("minecraft:diamond_sword", 2))
            .unwrap();
        if immune {
            h.world()
                .get_mut::<ActorFlags>(target)
                .unwrap()
                .update(jolyne::valentine::actor_flags::FIRE_IMMUNE, true)
                .unwrap();
        } else {
            h.world().entity_mut(target).insert(Health::new(0.5));
        }
        player_hit(&mut h, target);
        assert!(h.world().get::<fire::Burning>(target).is_none());
        if !immune {
            assert!(h.world().get::<Dead>(target).is_some());
        }
    }
}
