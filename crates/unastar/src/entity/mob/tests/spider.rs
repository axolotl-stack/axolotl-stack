//! The spider and the cave spider replayed against BDS 1.26.51
//! (`mob/ai/testdata/bds-1.26.51-spider.json`, distilled by
//! `tools/bds-sweep/mob_fixture.py` from `sweep.py spider`): their summons,
//! climbing walls, day and night, anger, leaps and the cave spider's poison.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::McpePacketData;
use jolyne::valentine::actor_flags;
use jolyne::valentine::types::EnumsMobEffectPacketPayloadEvent;

use super::bds_mob::{EffectOp, Event, Fixture};
use super::hostile::{assert_add_actor_matches, native};
use crate::entity::components::{GameMode, Health, Position};
use crate::entity::definition::flags::ActorFlags;
use crate::world::rules::Difficulty;

const SPIDER: &str = "minecraft:spider";
const CAVE_SPIDER: &str = "minecraft:cave_spider";
/// The poison effect's wire id.
const POISON: i32 = 19;

fn summon(identifier: &str, bytes: &[u8]) {
    let native = native(bytes);
    let mut a = super::facets::floor(1, Difficulty::Normal, GameMode::Creative);
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let mob: Entity = a.adult(identifier, feet);
    assert_add_actor_matches(a.world(), identifier, mob, &native);
}

#[test]
fn spawned_spider_add_actor_matches_native_summon() {
    summon(
        SPIDER,
        include_bytes!("../../../../../bds-extractor/captures/1.26.51/operator/summons/spider.bin"),
    );
}

#[test]
fn spawned_cave_spider_add_actor_matches_native_summon() {
    summon(
        CAVE_SPIDER,
        include_bytes!(
            "../../../../../bds-extractor/captures/1.26.51/operator/summons/cave_spider.bin"
        ),
    );
}

fn fixture() -> Fixture {
    Fixture::parse(include_str!("../ai/testdata/bds-1.26.51-spider.json"))
}

/// The climb of `normal/wall-1`: BDS's spider walks west into a 3 × 3 stone
/// block 3 tall (x -1..=1, y -60..=-58) and climbs onto it. Replayed with
/// the production physics step from the tick before it touches the block,
/// walking west at its BDS speed every tick: it must rise, clear the top and
/// land exactly as BDS's did, and climb on the ticks BDS showed WALLCLIMBING.
#[test]
fn a_spider_climbs_a_wall_as_on_bds() {
    use crate::entity::components::{PhysicsBody, WallClimbing};
    use crate::entity::effects::EffectPhysics;
    use crate::entity::systems::physics::step_body_with_effects;
    use crate::entity::systems::physics::tests::TestBlocks;

    let fixture = fixture();
    let item = fixture.item("normal/wall-1");
    let mut blocks = TestBlocks::floor("minecraft:grass_block", -61);
    for x in -1..=1 {
        for y in -60..=-58 {
            for z in -1..=1 {
                blocks.set(IVec3::new(x, y, z), "minecraft:stone");
            }
        }
    }
    let mut body = PhysicsBody::living(1.4, 0.9);
    body.wall_climber = true;
    let (first, last) = (82, 104);
    let climbing_ticks: Vec<(u32, bool)> = item
        .flag_changes(actor_flags::WALLCLIMBING)
        .into_iter()
        .filter(|&(t, _)| (first..last).contains(&t))
        .collect();
    assert_eq!(climbing_ticks, [(83, true), (99, false)], "the BDS flag");
    let mut feet = DVec3::from_array(item.feet(first));
    let mut velocity = DVec3::ZERO;
    let mut on_ground = true;
    let mut climbing = WallClimbing(false);
    let mut ours = Vec::new();
    for t in first..last {
        // The spider walks west at its BDS speed (z as BDS moved it).
        let [x0, _, z0] = item.feet(t);
        let [x1, _, z1] = item.feet(t + 1);
        velocity.x = if t < 99 { -0.19 } else { x1 - x0 };
        velocity.z = z1 - z0;
        let was = climbing.0;
        feet = step_body_with_effects(
            &blocks,
            &body,
            feet,
            &mut velocity,
            &mut on_ground,
            u64::from(t),
            EffectPhysics::default(),
            Some(&mut climbing),
        )
        .0;
        let bds = item.feet(t + 1);
        assert!(
            (feet.y - bds[1]).abs() < 1e-4,
            "tick {t}: y {} vs BDS {}",
            feet.y,
            bds[1]
        );
        if climbing.0 != was {
            ours.push((t, climbing.0));
        }
    }
    assert_eq!(ours, climbing_ticks, "WALLCLIMBING changes");
    assert!(on_ground, "landed on the block");
}

/// A spider walking into a wall shows `WALLCLIMBING` while it climbs and
/// climbs over it, end to end through its AI.
#[test]
fn a_spider_chasing_the_player_climbs_onto_its_block() {
    let mut a = super::facets::floor(3, Difficulty::Normal, GameMode::Survival);
    for x in 11..=13 {
        for y in 64..=66 {
            for z in 11..=13 {
                a.set_block(IVec3::new(x, y, z), "minecraft:stone");
            }
        }
    }
    a.move_player(DVec3::new(12.5, 67.0, 12.5));
    let mob = a.adult(SPIDER, DVec3::new(19.5, 64.0, 12.5));
    let mut flagged = false;
    let mut on_top = false;
    for _ in 0..600 {
        a.ticks(1);
        let world = a.world();
        flagged |= world
            .get::<ActorFlags>(mob)
            .unwrap()
            .get(actor_flags::WALLCLIMBING)
            .unwrap();
        let feet = world.get::<Position>(mob).unwrap().0;
        if feet.y >= 67.0 {
            on_top = true;
            break;
        }
    }
    assert!(flagged, "the spider never showed WALLCLIMBING");
    assert!(on_top, "the spider never climbed onto the block");
}

/// The cave spider's hit poisons for 0 (easy: the effect is added and
/// removed at once), 7 (normal) and 15 seconds (hard), as BDS's did.
#[test]
fn cave_spider_hits_poison_by_difficulty_as_on_bds() {
    let fixture = fixture();
    for (label, difficulty) in [
        ("easy", Difficulty::Easy),
        ("normal", Difficulty::Normal),
        ("hard", Difficulty::Hard),
    ] {
        let bds: Vec<i32> = fixture
            .items(&format!("{label}/cave-"))
            .flat_map(|item| item.events.iter())
            .filter_map(|e| match *e {
                Event::Effect {
                    effect: POISON,
                    op: EffectOp::Add,
                    duration,
                } => Some(duration),
                _ => None,
            })
            .collect();
        let bds_duration = *bds.first().expect("a BDS poison");
        assert!(bds.iter().all(|d| *d == bds_duration));

        let mut a = super::facets::floor(5, difficulty, GameMode::Survival);
        a.adult(CAVE_SPIDER, DVec3::new(18.5, 64.0, 12.5));
        let mut ours = None;
        for _ in 0..600 {
            a.ticks(1);
            let packets = a.packets();
            let mut removed = false;
            for packet in &packets {
                if let McpePacketData::MobEffectPacket(p) = packet
                    && p.effect_id == POISON
                {
                    match p.event_id {
                        EnumsMobEffectPacketPayloadEvent::Add => {
                            ours = Some(p.effect_duration_ticks)
                        }
                        EnumsMobEffectPacketPayloadEvent::Remove => removed = true,
                        _ => {}
                    }
                }
            }
            if let Some(duration) = ours {
                assert_eq!(duration, bds_duration, "{label}");
                if duration == 0 {
                    // BDS adds and removes the 0-tick poison in one batch.
                    assert!(removed, "{label}: the 0-tick poison stays");
                }
                break;
            }
        }
        assert!(ours.is_some(), "{label}: no poison");
    }
}

/// A spider spawned at `time` near the survival player: whether it turned
/// hostile (it has the hostile group's target goal) after a few ticks.
fn hostile_at(time: u64) -> bool {
    use crate::entity::mob::components::NearestAttackableTargetGoal;
    let mut a = super::facets::floor(7, Difficulty::Normal, GameMode::Survival);
    crate::world::clock::set_time(a.world(), time);
    let mob = a.adult(SPIDER, DVec3::new(16.5, 64.0, 12.5));
    a.ticks(3);
    a.world().get::<NearestAttackableTargetGoal>(mob).is_some()
}

/// The spider turns hostile as the evening sky darkens its light below
/// 0.49: BDS's frozen spiders, the clock moved 100 ticks every 40 from
/// 11000, first targeted the player while the time was 12600 and 12700,
/// never before.
#[test]
fn a_spider_turns_hostile_at_dusk_as_on_bds() {
    let fixture = fixture();
    for item in fixture.items("normal/dusk-") {
        let (first, _) = item.target_changes()[0];
        // The time after the clock steps before the tick it targeted.
        let time = 11_000 + 100 * u64::from(first / 40);
        assert!((12_600..=12_700).contains(&time), "{}: {time}", item.key);
    }
    assert!(!hostile_at(6_000), "noon");
    assert!(!hostile_at(12_500));
    assert!(hostile_at(12_600));
    assert!(hostile_at(18_000), "midnight");
}

/// At noon a spider stays neutral until the player hurts it, then is angry
/// (and targets the player) for 10 ± 3 seconds counted in whole seconds, as
/// BDS's angry spiders calmed 140 to 260 ticks after they took a target.
#[test]
fn a_hurt_spider_by_day_is_angry_for_whole_seconds_as_on_bds() {
    use crate::entity::mob::combat;
    use crate::entity::mob::components::KnockbackPower;
    use crate::entity::mob::state::AttackTarget;
    use unastar_data::actor_components::body::ActorDamageCause;

    let fixture = fixture();
    let mut bds = Vec::new();
    // The leap items hurt their spider every 50 ticks, restarting its anger.
    for item in fixture
        .items
        .iter()
        .filter(|item| !item.key.contains("/leap-"))
    {
        // Spans that end as the anger does (ANGRY cleared with the target).
        let calmed = item.flag_changes(actor_flags::ANGRY);
        let changes = item.target_changes();
        for pair in changes.windows(2) {
            if let [(start, true), (end, false)] = *pair
                && calmed.contains(&(end, false))
            {
                bds.push(end - start);
            }
        }
    }
    assert!(bds.len() >= 8, "{bds:?}");
    assert!(
        bds.iter().all(|d| (140..=260).contains(d) && d % 20 == 0),
        "{bds:?}"
    );

    let mut ours = Vec::new();
    for seed in 0..12 {
        let mut a = super::facets::floor(20 + seed, Difficulty::Normal, GameMode::Survival);
        crate::world::clock::set_time(a.world(), 6_000);
        // BDS's player stood in a barrier pen open to the east, too narrow
        // for the spider: every swing restarts the anger, and none landed.
        for y in 64..=66 {
            for (x, z) in [
                (11, 11),
                (11, 12),
                (11, 13),
                (12, 11),
                (13, 11),
                (12, 13),
                (13, 13),
            ] {
                a.set_block(IVec3::new(x, y, z), "minecraft:barrier");
            }
        }
        let mob = a.adult(SPIDER, DVec3::new(15.5, 64.0, 12.5));
        a.ticks(60);
        assert!(
            a.world().get::<AttackTarget>(mob).is_none(),
            "neutral by day"
        );
        let player = a.player;
        combat::hurt_mob(
            a.world(),
            mob,
            combat::Hit {
                damage: 1.0,
                cause: ActorDamageCause::EntityAttack,
                attacker: Some(player),
                by_player: true,
                knockback_from: Some(DVec3::new(12.5, 64.0, 12.5)),
                knockback_power: KnockbackPower::DEFAULT,
            },
        );
        let mut start = None;
        for t in 0..400u32 {
            a.ticks(1);
            let world = a.world();
            // The player lives through the hits, as BDS's did on Health Boost.
            let mut health = world.get_mut::<Health>(player).unwrap();
            health.current = health.max;
            let angry = world
                .get::<ActorFlags>(mob)
                .unwrap()
                .get(actor_flags::ANGRY)
                .unwrap();
            let targets = world.get::<AttackTarget>(mob).is_some();
            if start.is_some_and(|s| t == s + 1) {
                // BDS shows ANGRY one tick after the target, as here.
                assert!(angry, "angry with its target");
            }
            match (start, targets) {
                (None, true) => start = Some(t),
                (Some(s), false) => {
                    ours.push(t - s);
                    break;
                }
                _ => {}
            }
        }
    }
    assert_eq!(ours.len(), 12, "{ours:?}");
    assert!(
        ours.iter().all(|d| (140..=260).contains(d) && d % 20 == 0),
        "{ours:?}"
    );
}

/// Whether a frozen spider `distance` blocks east of the survival player,
/// hurt by it, leaps by exactly (-0.4, 0.4, 0) within 45 ticks.
fn leaps(seed: u64, distance: f64) -> bool {
    use crate::entity::components::{ActiveEffect, EffectType, Velocity};
    use crate::entity::mob::combat;
    use crate::entity::mob::components::KnockbackPower;
    use unastar_data::actor_components::body::ActorDamageCause;

    let mut a = super::facets::floor(40 + seed, Difficulty::Normal, GameMode::Survival);
    let spot = DVec3::new(12.5 + distance, 64.0, 12.5);
    let mob = a.adult(SPIDER, spot);
    crate::entity::effects::give(
        a.world(),
        mob,
        EffectType::SLOWNESS,
        ActiveEffect::new(254, 100_000),
    );
    let player = a.player;
    combat::hurt_mob(
        a.world(),
        mob,
        combat::Hit {
            damage: 1.0,
            cause: ActorDamageCause::EntityAttack,
            attacker: Some(player),
            by_player: true,
            knockback_from: None,
            knockback_power: KnockbackPower::DEFAULT,
        },
    );
    // Put back where it stood, as the sweep did three ticks after the hit.
    // BDS's spiders leapt only before their melee goal took over (right
    // after they turned angry, in the air from the knockback); the melee
    // goal is taken away so only the leap's own conditions decide.
    a.ticks(3);
    a.world()
        .entity_mut(mob)
        .remove::<crate::entity::mob::components::MeleeBoxAttackGoal>();
    a.world().get_mut::<Position>(mob).unwrap().0 = spot;
    a.world().get_mut::<Velocity>(mob).unwrap().0 = DVec3::ZERO;
    let mut last = spot;
    for _ in 0..42 {
        a.ticks(1);
        let feet = a.world().get::<Position>(mob).unwrap().0;
        let step = feet - last;
        last = feet;
        if (step - DVec3::new(-0.4, 0.4, 0.0)).length() < 1e-6 {
            return true;
        }
    }
    false
}

/// A frozen spider just turned angry leaps at the player from 1.5 to 3.5
/// blocks away and never from 4.5 or 5.5, as BDS's did (`normal/leap-*`:
/// each leap moved it by exactly (-0.4, 0.4, 0)).
#[test]
fn an_angry_spider_leaps_from_the_distances_bds_spiders_leapt_from() {
    let fixture = fixture();
    for item in fixture.items("normal/leap-") {
        // The spider stood this far east of the player.
        let distance = item.at[0] - 0.5;
        let bds_leapt = item
            .moves
            .windows(2)
            .filter(|w| w[1].0 == w[0].0 + 1)
            .any(|w| {
                (w[1].1 - w[0].1 + 0.4).abs() < 1e-3
                    && (w[1].2 - w[0].2 - 0.4).abs() < 1e-3
                    && (w[1].3 - w[0].3).abs() < 1e-3
            });
        assert_eq!(bds_leapt, distance < 4.0, "BDS at {distance}");
        let ours = (0..6).filter(|&seed| leaps(seed, distance)).count();
        if distance < 4.0 {
            assert!(ours >= 3, "{distance}: {ours} of 6 leapt");
        } else {
            assert_eq!(ours, 0, "{distance}");
        }
    }
}
