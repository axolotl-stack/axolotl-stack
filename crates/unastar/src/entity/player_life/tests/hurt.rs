use super::*;
use crate::entity::components::ArmourInventory;
use crate::item::test_support::stack;

/// A 3-damage melee hit from a mob, the player wearing an iron chestplate
/// (6 defence, no toughness): scaled by the difficulty, then reduced to
/// `damage × (1 − max(6 / 5, 6 − damage / 2) / 25)`.
#[test]
fn a_mob_hit_is_scaled_by_difficulty_reduced_by_armour_and_knocks_the_player_back() {
    for (difficulty, scaled, dealt) in [
        (Difficulty::Easy, 2.5, 2.5 * (1.0 - 4.75 / 25.0)),
        (Difficulty::Normal, 3.0, 3.0 * (1.0 - 4.5 / 25.0)),
        (Difficulty::Hard, 4.5, 4.5 * (1.0 - 3.75 / 25.0)),
    ] {
        let mut h = harness(difficulty);
        let player = h.player;
        h.world()
            .get_mut::<ArmourInventory>(player)
            .unwrap()
            .set_chestplate(stack("minecraft:iron_chestplate", 1))
            .unwrap();
        let mut viewer = h.viewer();
        let cow = h.cow();

        let removed = h.hit_from(cow, 3.0);
        assert!(
            (removed - dealt).abs() < 1.0e-4,
            "{difficulty:?}: {removed} vs {dealt} ({scaled} before armour)"
        );
        assert!((h.health() - (20.0 - dealt)).abs() < 1.0e-4);
        let hurt = h
            .world()
            .get::<crate::entity::mob::state::LastHurt>(player)
            .copied()
            .unwrap();
        assert_eq!(
            (hurt.cause, hurt.attacker),
            (ActorDamageCause::EntityAttack, Some(cow))
        );

        h.tick();
        let packets = h.drain();
        assert_eq!(actor_events(&packets, SESSION), [EnumsActorEvent::Hurt]);
        assert_eq!(
            actor_events(&drain(&mut viewer), SESSION),
            [EnumsActorEvent::Hurt],
            "the viewer sees the hurt"
        );
        let health = health_updates(&packets);
        assert_eq!(health.len(), 1, "{packets:?}");
        assert!((health[0] - (20.0 - dealt)).abs() < 1.0e-4);
        // Pushed away from the cow standing to the west, and up.
        let motion = motions(&packets);
        assert_eq!(motion.len(), 1, "one knockback to the player's own client");
        assert!(motion[0].x > 0.0 && motion[0].y > 0.0, "{:?}", motion[0]);
        assert!(motion[0].z.abs() < motion[0].x, "{:?}", motion[0]);
    }
}

#[test]
fn peaceful_mobs_deal_nothing() {
    let mut h = harness(Difficulty::Peaceful);
    let cow = h.cow();
    assert_eq!(h.hit_from(cow, 3.0), 0.0);
    h.tick();
    let packets = h.drain();
    assert_eq!(h.health(), 20.0);
    assert!(actor_events(&packets, SESSION).is_empty());
    assert!(health_updates(&packets).is_empty());
    assert!(motions(&packets).is_empty());
}

#[test]
fn creative_and_spectator_players_are_immune_to_all_but_the_void() {
    for mode in [GameMode::Creative, GameMode::Spectator] {
        let mut h = harness(Difficulty::Normal);
        h.set_mode(mode);
        let cow = h.cow();
        assert_eq!(h.hit_from(cow, 5.0), 0.0, "{mode:?}: mob hit");

        h.fall(HOME, 10);
        assert_eq!(h.health(), 20.0, "{mode:?}: fall");

        h.set_block(HOME.floor().as_ivec3(), *blocks::LAVA);
        h.ticks(20);
        assert_eq!(h.health(), 20.0, "{mode:?}: lava");
        let player = h.player;
        assert!(
            h.world()
                .get::<super::super::environment::Burning>(player)
                .is_none()
        );
        assert!(actor_events(&h.drain(), SESSION).is_empty());

        // More than 16 blocks under the bottom of the world.
        let void = f64::from(crate::world::chunk::MIN_Y) - 17.0;
        h.world().get_mut::<Position>(player).unwrap().0 = DVec3::new(HOME.x, void, HOME.z);
        h.tick();
        assert_eq!(h.health(), 16.0, "{mode:?}: the void hurts");
        let hurt = h
            .world()
            .get::<crate::entity::mob::state::LastHurt>(player)
            .copied()
            .unwrap();
        assert_eq!(hurt.cause, ActorDamageCause::Void);
    }
}
