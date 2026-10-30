use super::*;
use crate::entity::mob::state::LastHurt;
use crate::entity::player_life::environment::Burning;

fn last_cause(h: &mut Harness) -> ActorDamageCause {
    let player = h.player;
    h.world().get::<LastHurt>(player).unwrap().cause
}

/// Ten blocks of fall reported by `PlayerAuthInput`: `ceil(10 − 3)` = 7.
#[test]
fn a_ten_block_fall_deals_seven_damage() {
    let mut h = harness(Difficulty::Normal);
    h.fall(HOME, 10);
    assert_eq!(h.health(), 13.0);
    assert_eq!(last_cause(&mut h), ActorDamageCause::Fall);
    assert!(!h.dead());
    h.tick();
    let packets = h.drain();
    assert_eq!(health_updates(&packets), [13.0]);
    assert!(actor_events(&packets, SESSION).contains(&EnumsActorEvent::Hurt));
    // The landing block's `fall` sound names the block by its wire id.
    assert!(packets.iter().any(|packet| matches!(packet,
        McpePacketData::LevelSoundEventPacket(sound) if &*sound.sound_event == "fall")));
    assert!(
        crate::server::wire_audit::assert_block_ids_on_wire(
            crate::registry::block::BlockNetworkIds::Hashed,
            &packets
        ) > 0
    );

    // Three blocks are safe.
    h.fall(HOME, 3);
    assert_eq!(h.health(), 13.0);
}

#[test]
fn falls_into_water_in_creative_or_without_the_rule_deal_nothing() {
    let mut h = harness(Difficulty::Normal);
    let pool = HOME.floor().as_ivec3();
    h.set_block(pool, *blocks::WATER);
    h.set_block(pool + IVec3::Y, *blocks::WATER);
    h.fall(HOME, 10);
    assert_eq!(h.health(), 20.0, "water");

    let dry = HOME + DVec3::X * 3.0;
    h.set_mode(GameMode::Creative);
    h.step(dry, true);
    h.fall(dry, 10);
    assert_eq!(h.health(), 20.0, "creative");

    h.set_mode(GameMode::Survival);
    h.rules().fall_damage = false;
    h.fall(dry, 10);
    assert_eq!(h.health(), 20.0, "fall_damage off");
}

/// Lava hurts 4 per hit under the 10-tick hurt timer and sets the player
/// alight; out of the lava the fire deals 1 every 20 ticks.
#[test]
fn lava_burns() {
    let mut h = harness(Difficulty::Normal);
    h.set_block(HOME.floor().as_ivec3(), *blocks::LAVA);
    h.tick();
    assert_eq!(h.health(), 16.0);
    assert_eq!(last_cause(&mut h), ActorDamageCause::Lava);
    let player = h.player;
    assert!(h.world().get::<Burning>(player).is_some());
    h.ticks(10);
    assert_eq!(h.health(), 12.0, "the next lava hit after the hurt timer");
    let packets = h.drain();
    assert_eq!(health_updates(&packets), [16.0, 12.0]);

    // Out of the lava the player keeps burning.
    h.step(HOME + DVec3::X * 3.0, true);
    let health = h.health();
    h.ticks(20);
    assert_eq!(h.health(), health - 1.0);
    assert_eq!(last_cause(&mut h), ActorDamageCause::FireTick);

    // Water puts the fire out.
    let pool = (HOME + DVec3::X * 6.0).floor().as_ivec3();
    h.set_block(pool, *blocks::WATER);
    h.step(HOME + DVec3::X * 6.0, true);
    assert!(h.world().get::<Burning>(player).is_none());
}

#[test]
fn lava_deals_nothing_without_the_fire_damage_rule() {
    let mut h = harness(Difficulty::Normal);
    h.rules().fire_damage = false;
    h.set_block(HOME.floor().as_ivec3(), *blocks::LAVA);
    h.ticks(30);
    assert_eq!(h.health(), 20.0);
}
