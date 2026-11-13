//! The enderman replayed against BDS 1.26.51
//! (`mob/ai/testdata/bds-1.26.51-enderman.json`, distilled by
//! `tools/bds-sweep/mob_fixture.py` from `sweep.py enderman`): its summon,
//! the stare, its teleports and the blocks it carries.
//!
//! BDS's superflat floor is at y -61 (feet at -60), the arena's at 63
//! (feet at 64); the replays rebuild each BDS setup in the arena. Random
//! parts are compared by distribution: many seeded trials here against
//! BDS's counts.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::McpePacketData;
use jolyne::valentine::actor_flags;

use super::bds_mob::{Event, Fixture, Item};
use super::hostile::{assert_add_actor_matches, native};
use crate::entity::components::{GameMode, PhysicsBody, Position, Rotation, Velocity};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::components::LookedAtConfig;
use crate::entity::mob::constants::LOOKED_AT_POINT_RATIO;
use crate::entity::mob::life::looked_at::{in_view, view_vector};
use crate::entity::mob::state::AttackTarget;
use crate::entity::systems::BlockSource;
use crate::world::rules::Difficulty;

const ENDERMAN: &str = "minecraft:enderman";
/// The player's eyes above its feet.
const EYE: f64 = crate::entity::PLAYER_EYE_HEIGHT;
/// The level event at both ends of a teleport.
const TELEPORT_EVENT: i32 = 1018;

fn fixture() -> Fixture {
    Fixture::parse(include_str!("../ai/testdata/bds-1.26.51-enderman.json"))
}

#[test]
fn spawned_enderman_add_actor_matches_native_summon() {
    let native = native(include_bytes!(
        "../../../../../bds-extractor/captures/1.26.51/operator/summons/enderman.bin"
    ));
    let mut a = super::facets::floor(1, Difficulty::Normal, GameMode::Creative);
    let feet = DVec3::new(
        f64::from(native.position.x),
        f64::from(native.position.y),
        f64::from(native.position.z),
    );
    let mob: Entity = a.adult(ENDERMAN, feet);
    assert_add_actor_matches(a.world(), ENDERMAN, mob, &native);
}

/// Whether BDS's enderman moved (teleported) in samples `from..=to`.
fn moved_between(item: &Item, from: u32, to: u32) -> bool {
    let home = item.feet(from);
    (from..=to).any(|t| {
        let feet = item.feet(t);
        (feet[0] - home[0]).abs() > 0.01 || (feet[2] - home[2]).abs() > 0.01
    })
}

/// Whether BDS's enderman took the player as its target.
fn provoked(item: &Item) -> bool {
    item.target_changes().iter().any(|&(_, set)| set)
}

/// Every BDS look from 4 to 32 blocks, off in yaw or aimed up and down the
/// enderman, replayed through the production cone with the enderman's
/// compiled `minecraft:looked_at`: it provokes exactly when BDS's did.
#[test]
fn the_stare_cone_replays_every_bds_look() {
    let fixture = fixture();
    let mut a = super::facets::floor(2, Difficulty::Normal, GameMode::Survival);
    let mob = a.adult(ENDERMAN, DVec3::new(8.5, 64.0, 8.5));
    let world = a.world();
    let config = world.get::<LookedAtConfig>(mob).unwrap().clone();
    let height = world.get::<PhysicsBody>(mob).unwrap().height;
    let mut checked = 0;
    for item in fixture.items("normal/look-") {
        let key = item.key.trim_start_matches("normal/look-");
        // Past 48 blocks BDS never sent the client the enderman.
        if !["yaw", "h1", "h2", "h3"].iter().any(|p| key.starts_with(p)) {
            continue;
        }
        let turn = item.player_turns[0];
        // Kept out: BDS's enderman teleported away (after it was put back
        // on tick 5) before the target was due.
        if moved_between(item, 6, turn.t + 8) {
            continue;
        }
        let eye = DVec3::from_array(turn.feet) + DVec3::Y * EYE;
        let point = DVec3::from_array(item.feet(6)) + DVec3::Y * height * LOOKED_AT_POINT_RATIO;
        let view = view_vector(&Rotation::new(turn.yaw, turn.pitch));
        let ours = eye.distance(point) <= config.search_radius
            && in_view(eye, view, point, config.cone_slack);
        assert_eq!(ours, provoked(item), "{}", item.key);
        checked += 1;
    }
    assert!(checked >= 25, "only {checked} looks replayed");
}

/// A stare in the arena: the survival player 8 blocks west of a frozen
/// enderman turns to look at its look-at point on tick `look` and away
/// after `hold` ticks (never when `None`). Returns, relative to `look`, the
/// tick of the `TARGET`, of the `ANGRY` flag and of the `stare` and
/// `mob.warning` sounds.
struct Stare {
    target: Option<u64>,
    angry: Option<u64>,
    stare: Option<u64>,
    warning: Option<u64>,
}

fn stare(hold: Option<u64>, setup: impl FnOnce(&mut super::facets::Arena)) -> Stare {
    let mut a = super::facets::floor(3, Difficulty::Normal, GameMode::Survival);
    let feet = DVec3::new(20.5, 64.0, 8.5);
    let player_feet = DVec3::new(12.5, 64.0, 8.5);
    a.move_player(player_feet);
    setup(&mut a);
    let mob = a.adult(ENDERMAN, feet);
    let player = a.player;
    let height = a.world().get::<PhysicsBody>(mob).unwrap().height;
    let to = feet + DVec3::Y * height * LOOKED_AT_POINT_RATIO - (player_feet + DVec3::Y * EYE);
    let yaw = (-to.x).atan2(to.z).to_degrees() as f32;
    let pitch = (-to.y).atan2(to.x.hypot(to.z)).to_degrees() as f32;
    let look = 10;
    let mut out = Stare {
        target: None,
        angry: None,
        stare: None,
        warning: None,
    };
    for t in 0..40u64 {
        // The enderman stays put whatever its random teleports did.
        let world = a.world();
        world.get_mut::<Position>(mob).unwrap().0 = feet;
        world.get_mut::<Velocity>(mob).unwrap().0 = DVec3::ZERO;
        let rotation = if t >= look && hold.is_none_or(|hold| t < look + hold) {
            Rotation::new(yaw, pitch)
        } else {
            Rotation::new(90.0, 0.0)
        };
        *world.get_mut::<Rotation>(player).unwrap() = rotation;
        a.ticks(1);
        let world = a.world();
        if out.target.is_none() && world.get::<AttackTarget>(mob).is_some() {
            out.target = Some(t - look);
        }
        if out.angry.is_none()
            && world
                .get::<ActorFlags>(mob)
                .unwrap()
                .get(actor_flags::ANGRY)
                .unwrap()
        {
            out.angry = Some(t - look);
        }
        for packet in a.packets() {
            if let McpePacketData::LevelSoundEventPacket(p) = packet {
                let at = Some(t - look);
                match &*p.sound_event {
                    "stare" => out.stare = at,
                    "mob.warning" => out.warning = at,
                    _ => {}
                }
            }
        }
    }
    out
}

/// BDS's target came 7 ticks after the `/tp` that turned the player: the
/// sweep client echoes a server-set rotation about 2 ticks later, then the
/// look must last `min_looked_at_duration` (0.25 s, 5 ticks). Here the
/// rotation is the server's at once, so the target comes 5 ticks after the
/// turn; `ANGRY`, `stare` and `mob.warning` one tick later, as on BDS
/// (target on tick 17, the rest on 18, every provoked look).
#[test]
fn a_stare_provokes_the_enderman_as_on_bds() {
    let fixture = fixture();
    let bds = fixture.item("normal/look-yaw4.0-8");
    let turn = bds.player_turns[0].t;
    let target = bds.target_changes()[0].0;
    let angry = bds.flag_changes(actor_flags::ANGRY)[0].0;
    let sounds: Vec<(u32, &str)> = bds
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Sound { t, sound } if sound != "spawn" => Some((*t, sound.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!((target - turn, angry - target), (7, 1));
    assert_eq!(sounds, [(angry, "mob.warning"), (angry, "stare")]);

    let ours = stare(None, |_| {});
    let echo = 2;
    assert_eq!(ours.target, Some(u64::from(target - turn - echo)));
    assert_eq!(ours.angry, Some(u64::from(angry - turn - echo)));
    assert_eq!(ours.stare, ours.angry);
    assert_eq!(ours.warning, ours.angry);
}

/// BDS: a look held 7 ticks after the `/tp` never provoked, 8 did (the
/// player turned away on the tick the target would have come); here, with
/// no echo, 5 and 6.
#[test]
fn a_short_look_does_not_provoke_as_on_bds() {
    let fixture = fixture();
    let held = |k: u32| provoked(fixture.item(&format!("normal/look-hold{k}-8")));
    assert!(!held(7) && held(8));
    assert_eq!(stare(Some(5), |_| {}).target, None);
    assert!(stare(Some(6), |_| {}).target.is_some());
}

/// BDS: no target with a carved pumpkin on the player's head, in creative,
/// or with a stone or a glass wall between (glass has a collision box).
#[test]
fn a_pumpkin_creative_and_walls_stop_the_stare_as_on_bds() {
    use crate::entity::components::ArmourInventory;
    let fixture = fixture();
    for key in ["pumpkin-8", "creative-8", "wall-stone-8", "wall-glass-8"] {
        assert!(
            !provoked(fixture.item(&format!("normal/look-{key}"))),
            "{key}"
        );
    }
    let pumpkin = stare(None, |a| {
        let player = a.player;
        let world = a.world();
        let items = &world
            .resource::<crate::server::game::types::ItemRegistryResource>()
            .0;
        let helmet = items.stack("minecraft:carved_pumpkin", 1).unwrap();
        let mut armour = ArmourInventory::new();
        armour.set_helmet(helmet).unwrap();
        world.entity_mut(player).insert(armour);
    });
    assert_eq!(pumpkin.target, None, "pumpkin");
    let creative = stare(None, |a| {
        let player = a.player;
        a.world().entity_mut(player).insert(GameMode::Creative);
    });
    assert_eq!(creative.target, None, "creative");
    for wall in ["minecraft:stone", "minecraft:glass"] {
        let walled = stare(None, |a| {
            for y in 64..=67 {
                for z in 6..=10 {
                    a.set_block(IVec3::new(15, y, z), wall);
                }
            }
        });
        assert_eq!(walled.target, None, "{wall}");
    }
}

/// The ticks (samples) at which BDS's enderman teleported in `item`: the
/// first level event of each 1018 pair.
fn bds_teleports(item: &Item) -> Vec<u32> {
    let ticks: Vec<u32> = item
        .events
        .iter()
        .filter_map(|e| match *e {
            Event::Level { t, id, .. } if id == TELEPORT_EVENT => Some(t),
            _ => None,
        })
        .collect();
    ticks.chunks(2).map(|pair| pair[0]).collect()
}

/// Whether BDS's enderman teleported in the tick it spawned, over `items`.
fn bds_spawn_teleports(fixture: &Fixture, prefix: &str) -> (usize, usize) {
    let items: Vec<&Item> = fixture.items(prefix).collect();
    let moved = items
        .iter()
        .filter(|item| bds_teleports(item).first() == Some(&0))
        .count();
    (moved, items.len())
}

/// `n` endermen spawned in turn at `feet` in the arena `build` makes:
/// how many teleported in their spawn tick, and every such move.
fn spawn_tick_teleports(
    n: usize,
    feet: DVec3,
    build: impl Fn(IVec3) -> Option<u32> + Copy,
) -> (usize, Vec<DVec3>) {
    let mut a = super::facets::arena(4, Difficulty::Normal, GameMode::Creative, build);
    a.move_player(feet + DVec3::new(0.0, 0.0, -9.0));
    let mut moves = Vec::new();
    for _ in 0..n {
        let mob = a.adult(ENDERMAN, feet);
        a.ticks(1);
        let now = a.feet(mob);
        if now.distance(feet) > 1e-6 {
            moves.push(now - feet);
        }
        a.world().despawn(mob);
    }
    (moves.len(), moves)
}

/// Two proportions agree: their difference within `z` standard errors of
/// the pooled proportion.
fn same_rate((a, n): (usize, usize), (b, m): (usize, usize), z: f64) -> bool {
    let (a, n, b, m) = (a as f64, n as f64, b as f64, m as f64);
    let pooled = (a + b) / (n + m);
    let error = (pooled * (1.0 - pooled) * (1.0 / n + 1.0 / m))
        .sqrt()
        .max(1e-9);
    ((a / n - b / m) / error).abs() <= z
}

/// The spawn-tick teleport is one random attempt: on BDS's superflat about
/// half the targets lie in the ground and fail, and on a stone platform 16
/// blocks up nearly all land, on the platform or on the floor below it.
#[test]
fn an_enderman_tries_one_teleport_in_its_spawn_tick_as_on_bds() {
    let fixture = fixture();
    let stone = super::facets::block("minecraft:stone");
    let flat = spawn_tick_teleports(300, DVec3::new(8.5, 64.0, 8.5), |_| None);
    let bds_flat = bds_spawn_teleports(&fixture, "normal/spawn-flat-");
    assert!(
        same_rate((flat.0, 300), bds_flat, 3.0),
        "flat: ours {} of 300, BDS {bds_flat:?}",
        flat.0
    );
    let platform = move |pos: IVec3| {
        (pos.y == 79 && (-8..=24).contains(&pos.x) && (-8..=24).contains(&pos.z)).then_some(stone)
    };
    let high = spawn_tick_teleports(300, DVec3::new(8.5, 80.0, 8.5), platform);
    let bds_high = bds_spawn_teleports(&fixture, "normal/spawn-high-");
    assert!(
        same_rate((high.0, 300), bds_high, 3.0),
        "high: ours {} of 300, BDS {bds_high:?}",
        high.0
    );
    // Landings on the platform (dy 0) against the floor below (dy -16).
    let on_top = |moves: &[DVec3]| moves.iter().filter(|m| m.y.abs() < 0.5).count();
    let bds_moves: Vec<DVec3> = fixture
        .items("normal/spawn-high-")
        .filter(|item| bds_teleports(item).first() == Some(&0))
        .map(|item| DVec3::from_array(item.feet(1)) - DVec3::from_array(item.feet(0)))
        .collect();
    assert!(same_rate(
        (on_top(&high.1), high.1.len()),
        (on_top(&bds_moves), bds_moves.len()),
        3.0
    ));
    for m in high.1.iter().chain(flat.1.iter()) {
        assert!(m.x.abs() <= 16.0 && m.z.abs() <= 16.0, "{m}");
        assert!(m.y.abs() < 1e-9 || (m.y + 16.0).abs() < 1e-9, "{m}");
    }
}

/// The horizontal offsets of the spawn-tick teleports, ours and BDS's,
/// share a distribution: uniform over 32 blocks (two-sample
/// Kolmogorov–Smirnov at the 0.1% level).
#[test]
fn teleport_offsets_spread_like_bds() {
    let fixture = fixture();
    let mut bds: Vec<f64> = fixture
        .items("normal/spawn-")
        .filter(|item| bds_teleports(item).first() == Some(&0))
        .flat_map(|item| {
            let (from, to) = (item.feet(0), item.feet(1));
            [to[0] - from[0], to[2] - from[2]]
        })
        .collect();
    let (_, moves) = spawn_tick_teleports(400, DVec3::new(8.5, 64.0, 8.5), |_| None);
    let mut ours: Vec<f64> = moves.iter().flat_map(|m| [m.x, m.z]).collect();
    bds.sort_by(f64::total_cmp);
    ours.sort_by(f64::total_cmp);
    let cdf =
        |sample: &[f64], x: f64| sample.partition_point(|v| *v <= x) as f64 / sample.len() as f64;
    let d = bds
        .iter()
        .chain(ours.iter())
        .map(|&x| (cdf(&bds, x) - cdf(&ours, x)).abs())
        .fold(0.0, f64::max);
    let (n, m) = (bds.len() as f64, ours.len() as f64);
    let critical = 1.95 * ((n + m) / (n * m)).sqrt();
    assert!(
        d < critical,
        "KS distance {d} (critical {critical}, BDS n {n})"
    );
}

/// Two rates agree: `a` events in `ta` ticks and `b` in `tb`, their
/// difference within `z` standard errors of the pooled rate (Poisson).
fn same_poisson_rate((a, ta): (usize, u32), (b, tb): (usize, u32), z: f64) -> bool {
    let (a, ta, b, tb) = (a as f64, f64::from(ta), b as f64, f64::from(tb));
    let pooled = (a + b) / (ta + tb);
    let error = (pooled * (1.0 / ta + 1.0 / tb)).sqrt().max(1e-9);
    ((a / ta - b / tb) / error).abs() <= z
}

/// The random teleport rate of a frozen enderman on the stone platform,
/// put back every 40 ticks, at midnight, at noon and at noon under a stone
/// roof 6 blocks above the platform, against BDS's rate over the ticks its
/// enderman was there (one noon enderman despawned on the floor). Midnight
/// and the roof roll the dark chance; noon on the platform the light one.
#[test]
fn random_teleports_come_as_often_as_on_bds() {
    let fixture = fixture();
    let stone = super::facets::block("minecraft:stone");
    let square = |x: i32, z: i32| (-8..=24).contains(&x) && (-8..=24).contains(&z);
    for (prefix, time, roof) in [
        ("normal/idle-night-", 18_000, false),
        ("normal/idle-noon-", 6_000, false),
        ("normal/idle-roof", 6_000, true),
    ] {
        let (bds, observed) = fixture.items(prefix).fold((0, 0), |(n, ticks), item| {
            let observed = item.observed_ticks();
            let moves = bds_teleports(item)
                .iter()
                .filter(|&&t| t < observed)
                .count();
            (n + moves, ticks + observed)
        });
        let build = move |pos: IVec3| (pos.y == 79 && square(pos.x, pos.z)).then_some(stone);
        let mut a = super::facets::arena(5, Difficulty::Normal, GameMode::Creative, build);
        if roof {
            for x in -8..=24 {
                for z in -8..=24 {
                    a.set_block(IVec3::new(x, 86, z), "minecraft:stone");
                }
            }
        }
        crate::world::clock::set_time(a.world(), time);
        let home = DVec3::new(8.5, 80.0, 8.5);
        a.move_player(DVec3::new(8.5, 64.0, -0.5));
        let mob = a.adult(ENDERMAN, home);
        freeze(&mut a, mob);
        let ticks = 4000;
        let mut ours = 0;
        for t in 0..ticks {
            if t % 40 == 5 {
                a.world().get_mut::<Position>(mob).unwrap().0 = home;
            }
            let before = a.feet(mob);
            a.ticks(1);
            if a.feet(mob).distance(before) > 1.0 {
                ours += 1;
            }
        }
        assert!(
            same_poisson_rate((ours, ticks), (bds, observed), 3.0),
            "{prefix}: ours {ours} in {ticks} ticks, BDS {bds} in {observed}"
        );
    }
}

/// A hurt makes two attempts in its tick. BDS hurt its enderman on the
/// superflat every 20 ticks and put it back (`hurtnear`): counted per hurt,
/// it teleported 0, 1 or 2 times in that tick as two coin flips would.
/// Ours, hurt the same way, splits the same (each share within 3 standard
/// errors).
#[test]
fn a_hurt_enderman_tries_two_teleports_in_its_tick_as_on_bds() {
    use crate::entity::mob::combat::{Hit, hurt_mob};
    use crate::entity::mob::components::KnockbackPower;
    use unastar_data::actor_components::body::ActorDamageCause;
    let fixture = fixture();
    let mut bds = [0usize; 3];
    for item in fixture.items("normal/hurtnear-") {
        let hurts = item.damage.iter().map(|d| d.t).collect::<Vec<u32>>();
        let teleports = bds_teleports(item);
        for hurt in hurts {
            let n = teleports.iter().filter(|&&t| t == hurt).count();
            bds[n.min(2)] += 1;
        }
    }
    let mut a = super::facets::floor(6, Difficulty::Normal, GameMode::Creative);
    let home = DVec3::new(8.5, 64.0, 8.5);
    let mob = a.adult(ENDERMAN, home);
    freeze(&mut a, mob);
    let mut ours = [0usize; 3];
    for _ in 0..400 {
        // Random teleports between the hurts do not count: it is put back.
        a.ticks(11);
        let world = a.world();
        world.get_mut::<Position>(mob).unwrap().0 = home;
        let mut health = world
            .get_mut::<crate::entity::components::Health>(mob)
            .unwrap();
        health.current = health.max;
        let _ = a.packets();
        hurt_mob(
            a.world(),
            mob,
            Hit {
                damage: 1.0,
                cause: ActorDamageCause::Magic,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: KnockbackPower::DEFAULT,
            },
        );
        let ends = a
            .packets()
            .iter()
            .filter(|p| matches!(p, McpePacketData::LevelEventPacket(e) if e.event_id == TELEPORT_EVENT))
            .count();
        ours[(ends / 2).min(2)] += 1;
    }
    let (n, m) = (ours.iter().sum(), bds.iter().sum());
    for k in 0..3 {
        assert!(
            same_rate((ours[k], n), (bds[k], m), 3.0),
            "{k} teleports: ours {ours:?}, BDS {bds:?}"
        );
    }
}

/// `variable_max_auto_step` (1.0625 blocks): BDS's angry enderman walking
/// west into a one-block stone wall (x 4) stepped onto it in one tick,
/// staying on the ground (`step-block-0`, tick 33 to 34). Replayed with the
/// production physics step and the enderman's derived body.
#[test]
fn an_enderman_steps_up_a_block_as_on_bds() {
    use crate::entity::effects::EffectPhysics;
    use crate::entity::systems::physics::step_body_with_effects;
    use crate::entity::systems::physics::tests::TestBlocks;

    let fixture = fixture();
    let item = fixture.item("normal/step-block-0");
    let mut a = super::facets::floor(8, Difficulty::Normal, GameMode::Creative);
    let mob = a.adult(ENDERMAN, DVec3::new(8.5, 64.0, 8.5));
    let body = *a.world().get::<PhysicsBody>(mob).unwrap();
    assert_eq!(body.step_height, 1.0625);
    let mut blocks = TestBlocks::floor("minecraft:grass_block", -61);
    for z in -6..=6 {
        blocks.set(IVec3::new(4, -60, z), "minecraft:stone");
    }
    let (before, after) = (item.feet(33), item.feet(34));
    // A mob resting on the ground carries one tick of gravity and drag.
    let mut velocity = DVec3::new(after[0] - before[0], -0.08 * 0.98, after[2] - before[2]);
    let mut on_ground = true;
    let (feet, _) = step_body_with_effects(
        &blocks,
        &body,
        DVec3::from_array(before),
        &mut velocity,
        &mut on_ground,
        33,
        EffectPhysics::default(),
        None,
    );
    assert!(
        (feet.y - after[1]).abs() < 1e-9,
        "y {} vs BDS {}",
        feet.y,
        after[1]
    );
    assert!(
        (feet.x - after[0]).abs() < 1e-4,
        "x {} vs BDS {}",
        feet.x,
        after[0]
    );
    assert!(on_ground);
}

/// The sealed room of `sweep.py enderman --items carry`, in the arena:
/// stone fills the enderman's whole teleport cube but a 5 × 5 room 4 tall
/// (x 10..=14, z 6..=10, feet at y 64) on a stone floor, so it never
/// leaves. `ring` is put at its feet around it (or under it with
/// `floor`), `covered` lays stone on the ring.
fn sealed_room(
    seed: u64,
    ring: &str,
    floor: bool,
    covered: bool,
) -> (super::facets::Arena, Entity) {
    let stone = super::facets::block("minecraft:stone");
    let ring_state = super::facets::block(ring);
    let room = |p: IVec3| (10..=14).contains(&p.x) && (6..=10).contains(&p.z);
    let build = move |p: IVec3| {
        let feet_cell = p.x == 12 && p.z == 8;
        match p.y {
            63 if room(p) => Some(if floor { ring_state } else { stone }),
            64 if room(p) && !feet_cell && !floor => Some(ring_state),
            65 if room(p) && !feet_cell && covered => Some(stone),
            64..=67 if room(p) => None,
            64..=79 if (-5..=29).contains(&p.x) && (-9..=25).contains(&p.z) => Some(stone),
            _ => None,
        }
    };
    let mut a = super::facets::arena(seed, Difficulty::Normal, GameMode::Creative, build);
    a.move_player(DVec3::new(12.5, 64.0, 28.5));
    let mob = a.adult(ENDERMAN, DVec3::new(12.5, 64.0, 8.5));
    freeze(&mut a, mob);
    (a, mob)
}

/// Slowness 255, as the sweep froze BDS's endermen (they still teleport).
fn freeze(a: &mut super::facets::Arena, mob: Entity) {
    use crate::entity::components::{ActiveEffect, EffectType};
    use crate::entity::effects::give;
    let mut slow = ActiveEffect::new(255, 100_000);
    slow.show_particles = false;
    give(a.world(), mob, EffectType::SLOWNESS, slow);
}

/// The tick our enderman first held a block (its `MobEquipment` main hand)
/// within `ticks`, and the block it held.
fn first_take(a: &mut super::facets::Arena, mob: Entity, ticks: u32) -> Option<(u32, u32)> {
    use crate::entity::mob::ai::carry::carried;
    for t in 1..=ticks {
        a.ticks(1);
        if let Some(state) = carried(a.world(), mob) {
            return Some((t, state));
        }
    }
    None
}

/// BDS's sealed endermen took a listed block from the ring at their feet
/// (grass, sand, gravel, TNT, melon, clay, moss, mud, crimson nylium; dirt
/// and a pumpkin not within 400 ticks), never stone, planks or netherrack
/// (not listed), a poppy or a red mushroom (no collision box: never the
/// first box on the line of sight), a block under their feet, a covered
/// ring, or anything with mobGriefing off. Ours takes from the same rooms,
/// and as often: BDS's rate over its listed solid rings (takes per tick
/// before the item ended) against ours, Poisson, within 3 standard errors.
#[test]
fn an_enderman_takes_the_blocks_bds_takes() {
    let fixture = fixture();
    let take_tick = |item: &Item| {
        item.events.iter().find_map(|e| match *e {
            Event::Equipment { t, item, .. } if item != 0 => Some(t),
            _ => None,
        })
    };
    for key in [
        "stone",
        "oak_planks",
        "netherrack",
        "poppy",
        "red_mushroom",
        "floor",
        "covered",
        "nogrief",
    ] {
        let item = fixture.item(&format!("normal/carry-{key}"));
        assert_eq!(take_tick(item), None, "BDS took from {key}");
    }
    let solid = [
        "grass_block",
        "dirt",
        "sand",
        "gravel",
        "tnt",
        "pumpkin",
        "melon_block",
        "clay",
        "moss_block",
        "mud",
        "crimson_nylium",
    ];
    let (mut bds_takes, mut bds_ticks) = (0, 0);
    for item in fixture.items("normal/carry-") {
        let key = item.key.trim_start_matches("normal/carry-");
        if !(solid.contains(&key) || key.starts_with("grass-")) {
            continue;
        }
        match take_tick(item) {
            Some(t) => {
                bds_takes += 1;
                bds_ticks += t;
            }
            None => bds_ticks += item.observed_ticks(),
        }
    }
    let (mut ours, mut our_ticks) = (0, 0);
    for (n, name) in solid.iter().enumerate() {
        let block = format!("minecraft:{name}");
        for trial in 0..4u64 {
            let (mut a, mob) = sealed_room(100 + 10 * n as u64 + trial, &block, false, false);
            match first_take(&mut a, mob, 400) {
                Some((t, state)) => {
                    assert_eq!(
                        crate::registry::BlockRegistry::vanilla()
                            .get_by_runtime_id(state)
                            .unwrap()
                            .string_id,
                        block
                    );
                    ours += 1;
                    our_ticks += t;
                }
                None => our_ticks += 400,
            }
        }
    }
    assert!(
        same_poisson_rate((ours, our_ticks), (bds_takes, bds_ticks), 3.0),
        "ours {ours} in {our_ticks} ticks, BDS {bds_takes} in {bds_ticks}"
    );
    for (ring, floor, covered) in [
        ("minecraft:stone", false, false),
        ("minecraft:poppy", false, false),
        ("minecraft:red_mushroom", false, false),
        ("minecraft:grass_block", true, false),
        ("minecraft:grass_block", false, true),
    ] {
        let (mut a, mob) = sealed_room(7, ring, floor, covered);
        assert_eq!(
            first_take(&mut a, mob, 2000),
            None,
            "{ring} floor {floor} covered {covered}"
        );
    }
    let (mut a, mob) = sealed_room(8, "minecraft:grass_block", false, false);
    a.world()
        .resource_mut::<crate::world::rules::GameRules>()
        .mob_griefing = false;
    assert_eq!(first_take(&mut a, mob, 2000), None, "mobGriefing off");
}

/// The carried block is the main-hand item viewers see (BDS: a
/// `MobEquipment` with the grass item and its block state, the block
/// turned to air); a carrying enderman puts it down on an air block over a
/// full block within a block sideways and two up (BDS: on a ring block,
/// one up), and its hand empties.
#[test]
fn a_carried_block_shows_in_the_hand_and_is_put_down() {
    use crate::entity::mob::ai::carry::carried;
    let fixture = fixture();
    let placed = fixture.item("normal/carry-grass-0");
    let equipment: Vec<(u32, i32)> = placed
        .events
        .iter()
        .filter_map(|e| match *e {
            Event::Equipment { t, item, .. } => Some((t, item)),
            _ => None,
        })
        .collect();
    assert_eq!(equipment.len(), 2, "taken, then put down");
    assert!(equipment[0].1 != 0 && equipment[1].1 == 0);

    let (mut a, mob) = sealed_room(9, "minecraft:grass_block", false, false);
    let (_, state) = first_take(&mut a, mob, 2000).expect("a take");
    let shown = a.packets().into_iter().any(|p| {
        matches!(p, McpePacketData::MobEquipmentPacket(ref e)
            if e.item.id != 0 && e.item.stacksize == 1)
    });
    assert!(shown, "no MobEquipment with the block");
    assert_eq!(state, super::facets::block("minecraft:grass_block"));
    let grass = super::facets::block("minecraft:grass_block");
    let count = |world: &World| {
        let mut n = Vec::new();
        for x in 0..=24 {
            for z in -4..=20 {
                for y in 60..=70 {
                    if world.block_at(IVec3::new(x, y, z)) == Some(grass) {
                        n.push(IVec3::new(x, y, z));
                    }
                }
            }
        }
        n
    };
    let before = count(a.world());
    let mut put = false;
    for _ in 0..40_000 {
        a.ticks(1);
        if carried(a.world(), mob).is_none() {
            put = true;
            break;
        }
    }
    assert!(put, "never put the block down");
    let after = count(a.world());
    let new: Vec<_> = after.iter().filter(|p| !before.contains(p)).collect();
    let gone: Vec<_> = before.iter().filter(|p| !after.contains(p)).collect();
    assert_eq!(
        (new.len(), gone.len()),
        (1, 0),
        "new {new:?}, gone {gone:?}, feet {}",
        a.feet(mob)
    );
}

/// Water hurts the enderman (`hurt_on_condition`, `in_contact_with_water`:
/// drowning, 1 a tick), and the hurt teleports it: BDS's three endermen got
/// water at their feet on tick 20, were hurt by drowning in that tick and
/// teleported in it or the next.
#[test]
fn water_hurts_the_enderman_and_it_teleports_as_on_bds() {
    use crate::entity::mob::state::LastHurt;
    use unastar_data::actor_components::body::ActorDamageCause;
    let fixture = fixture();
    for item in fixture.items("normal/water-") {
        let hurt = &item.damage[0];
        assert_eq!(
            (hurt.t, hurt.cause.as_str()),
            (20, "drowning"),
            "{}",
            item.key
        );
        assert!(bds_teleports(item).iter().any(|&t| (20..=21).contains(&t)));
    }
    let mut moved = 0;
    for seed in 0..10 {
        let mut a = super::facets::floor(20 + seed, Difficulty::Normal, GameMode::Creative);
        let home = DVec3::new(8.5, 64.0, 8.5);
        let mob = a.adult(ENDERMAN, home);
        freeze(&mut a, mob);
        a.ticks(3);
        a.world().get_mut::<Position>(mob).unwrap().0 = home;
        a.set_block(IVec3::new(8, 64, 8), "minecraft:water");
        a.ticks(1);
        let hurt = a.world().get::<LastHurt>(mob).copied().expect("hurt");
        assert_eq!(hurt.cause, ActorDamageCause::Drowning);
        moved += usize::from(a.feet(mob).distance(home) > 1.0);
    }
    assert!(moved >= 5, "{moved} of 10 teleported");
}

/// A teleport on the wire, as BDS sent its spawn-tick teleports on the
/// platform: in the same tick a `MoveActorDelta` with `force_move` (ticks
/// 0), the trail (`LevelEventGeneric` 2023: 128 particles, scale 0.2, from
/// the old feet to the middle of the new feet block, spread over the box)
/// and level event 1018 at the old and at the new feet.
#[test]
fn a_teleport_looks_like_bds_on_the_wire() {
    use zuri_nbt::NBTTag;
    let fixture = fixture();
    let item = fixture.item("normal/spawn-high-0");
    let (from, to) = (item.feet(0), item.feet(1));
    let mut seen = (false, false, 0);
    for event in &item.events {
        match event {
            Event::ForcedMove { t: 0, ticks } => {
                assert_eq!(*ticks, 0);
                seen.0 = true;
            }
            Event::LevelGeneric {
                t: 0,
                id: 2023,
                data,
            } => {
                let field = |k: &str| data[k].as_f64().unwrap();
                assert_eq!((field("Count"), field("DirScale")), (128.0, 0.2));
                assert_eq!((field("Variationx"), field("Variationy")), (0.6, 2.9));
                assert_eq!([field("Startx"), field("Starty"), field("Startz")], from);
                let end = [to[0].floor() + 0.5, to[1] + 0.5, to[2].floor() + 0.5];
                assert_eq!([field("Endx"), field("Endy"), field("Endz")], end);
                seen.1 = true;
            }
            Event::Level {
                t: 0,
                id: TELEPORT_EVENT,
                ..
            } => seen.2 += 1,
            _ => {}
        }
    }
    assert_eq!(seen, (true, true, 2));

    let stone = super::facets::block("minecraft:stone");
    let platform = move |pos: IVec3| {
        (pos.y == 79 && (-8..=24).contains(&pos.x) && (-8..=24).contains(&pos.z)).then_some(stone)
    };
    let mut a = super::facets::arena(11, Difficulty::Normal, GameMode::Creative, platform);
    let home = DVec3::new(8.5, 80.0, 8.5);
    let mut packets = Vec::new();
    for _ in 0..20 {
        let mob = a.adult(ENDERMAN, home);
        let _ = a.packets();
        a.ticks(1);
        packets = a.packets();
        if a.feet(mob).distance(home) > 1.0 {
            break;
        }
        a.world().despawn(mob);
    }
    let to = packets
        .iter()
        .filter_map(|p| match p {
            McpePacketData::LevelEventPacket(e) if e.event_id == TELEPORT_EVENT => {
                Some(e.position.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(to.len(), 2, "the two ends");
    assert_eq!((to[0].x, to[0].y, to[0].z), (8.5, 80.0, 8.5));
    let forced = packets.iter().any(|p| {
        matches!(p, McpePacketData::MoveActorDeltaPacket(m)
            if m.move_data.force_move && m.move_data.ticks == 0)
    });
    assert!(forced, "no forced move");
    let trail = packets
        .iter()
        .find_map(|p| match p {
            McpePacketData::LevelEventGenericPacket(g) if g.event_id == 2023 => Some(g.clone()),
            _ => None,
        })
        .expect("no trail");
    let NBTTag::Compound(fields) = NBTTag::read(
        &mut trail.__ctd__.to_nbt().0.as_ref(),
        zuri_nbt::encoding::NetworkLittleEndian,
    )
    .unwrap() else {
        panic!("not a compound");
    };
    let float = |k: &str| match fields.0.get(k) {
        Some(NBTTag::Float(v)) => f64::from(v.0),
        other => panic!("{k}: {other:?}"),
    };
    assert!(matches!(fields.0.get("Count"), Some(NBTTag::Int(n)) if n.0 == 128));
    assert_eq!(float("Startx"), 8.5);
    assert_eq!(float("Endx"), f64::from(to[1].x).floor() + 0.5);
    assert_eq!(float("Variationy"), f64::from(2.9f32));
}

/// A landing touching liquid fails: BDS's endermen spawned east of a
/// one-deep pool never landed in it (6 spawn-tick teleports of 30, all on
/// dry ground east of x 12); ours, with water over the west of its cube,
/// never lands in water either.
#[test]
fn an_enderman_never_lands_in_water_as_on_bds() {
    let fixture = fixture();
    for item in fixture.items("normal/pool-") {
        for event in &item.events {
            if let Event::Level {
                id: TELEPORT_EVENT,
                at,
                ..
            } = event
            {
                assert!(at[0] >= 5.0, "{}: landed at {at:?}", item.key);
            }
        }
    }
    let water = super::facets::block("minecraft:water");
    let pool = move |pos: IVec3| (pos.y == 64 && pos.x <= 8).then_some(water);
    let (n, moves) = spawn_tick_teleports(300, DVec3::new(16.5, 64.0, 8.5), pool);
    assert!(n > 0);
    for m in moves {
        assert!(16.5 + m.x > 8.6, "landed in the pool at dx {}", m.x);
    }
}

/// An arrow shot at the enderman: the player 4 blocks west looks east at
/// its chest and shoots. Returns whether the enderman was hurt and whether
/// it teleported in the 5 ticks after.
fn shoot_at(a: &mut super::facets::Arena, mob: Entity, from: DVec3) -> (bool, bool) {
    use crate::entity::components::Health;
    let player = a.player;
    a.move_player(from);
    *a.world().get_mut::<Rotation>(player).unwrap() = Rotation::new(-90.0, -6.0);
    let home = a.feet(mob);
    let health = a.world().get::<Health>(mob).unwrap().current;
    let arrow = unastar_data::actor_identifiers::ActorType::Arrow;
    crate::entity::projectile::launch(a.world(), player, arrow, None).expect("an arrow");
    let mut moved = false;
    for _ in 0..5 {
        a.ticks(1);
        moved |= a.feet(mob).distance(home) > 1.0;
    }
    let hurt = a.world().get::<Health>(mob).unwrap().current < health;
    (hurt, moved)
}

/// BDS: every arrow a dispenser shot at a frozen enderman on the superflat
/// missed, the enderman teleporting in the tick the arrow reached it (the
/// dispensing tick or the next; 13 of 13), and the arrow flew on; in the sealed room every arrow hit it (4 of
/// 4: projectile damage). Ours dodges on open ground and is hit when no
/// teleport can land.
#[test]
fn arrows_make_the_enderman_teleport_as_on_bds() {
    let fixture = fixture();
    let dodged = |item: &Item| {
        let fired = item.events.iter().find_map(|e| match *e {
            Event::Level { t, id: 1002, .. } => Some(t),
            _ => None,
        });
        // The arrow reaches the box on its first or second flight tick.
        fired.is_some_and(|t| bds_teleports(item).iter().any(|&m| m == t || m == t + 1))
            && !item.damage.iter().any(|d| d.cause == "projectile")
    };
    let mut open = 0;
    for item in fixture.items("normal/dodgemore-") {
        // Kept out: its enderman had already teleported away by itself
        // before the shot, or stood in water the pool items left.
        let fired = item.events.iter().find_map(|e| match *e {
            Event::Level { t, id: 1002, .. } => Some(t),
            _ => None,
        });
        let early = bds_teleports(item)
            .iter()
            .any(|&t| t > 0 && fired.is_some_and(|f| t < f));
        if early || item.damage.iter().any(|d| d.t < 20) {
            continue;
        }
        assert!(dodged(item), "{}", item.key);
        open += 1;
    }
    assert!(open >= 8);
    for item in fixture.items("normal/dodgesealed-") {
        assert_eq!(item.damage[0].cause, "projectile", "{}", item.key);
    }

    let mut a = super::facets::floor(30, Difficulty::Normal, GameMode::Creative);
    let mut dodges = 0;
    for _ in 0..10 {
        let mob = a.adult(ENDERMAN, DVec3::new(20.5, 64.0, 8.5));
        freeze(&mut a, mob);
        a.ticks(2);
        let feet = a.feet(mob);
        let (hurt, moved) = shoot_at(&mut a, mob, feet - DVec3::new(4.0, 0.0, 0.0));
        dodges += usize::from(moved && !hurt);
        a.world().despawn(mob);
    }
    assert!(dodges >= 9, "{dodges} of 10 dodged");
    let mut hits = 0;
    for seed in 0..4 {
        let (mut a, mob) = sealed_room(40 + seed, "minecraft:stone", false, false);
        let (hurt, _) = shoot_at(&mut a, mob, DVec3::new(10.5, 64.0, 8.5));
        hits += usize::from(hurt);
    }
    assert!(hits >= 3, "{hits} of 4 hit in the sealed room");
}

/// BDS: an angry enderman whose target stood 17 to 40 blocks away
/// teleported toward it, six of its thirteen teleports landing within 4
/// blocks of the point `target_distance` (16) along the line to the target.
/// Ours, angry at a player 24 blocks away, aims the same way: a like share
/// of its teleports land within 4.5 blocks of that aim.
#[test]
fn an_angry_enderman_teleports_toward_a_far_target_as_on_bds() {
    let fixture = fixture();
    let aimed = |from: DVec3, to: DVec3, target: DVec3| {
        let away = (target - from) * DVec3::new(1.0, 0.0, 1.0);
        let aim = from + away.normalize() * 16.0;
        (to.x - aim.x).abs() <= 4.5 && (to.z - aim.z).abs() <= 4.5
    };
    let (mut bds_aimed, mut bds_all) = (0, 0);
    for item in fixture.items("normal/chase-") {
        let target = DVec3::new(-15.5, -60.0, 0.5);
        let ends: Vec<[f64; 3]> = item
            .events
            .iter()
            .filter_map(|e| match *e {
                Event::Level {
                    t,
                    id: TELEPORT_EVENT,
                    at,
                } if t > 30 => Some(at),
                _ => None,
            })
            .collect();
        for pair in ends.chunks(2) {
            let (from, to) = (DVec3::from_array(pair[0]), DVec3::from_array(pair[1]));
            if from.distance(target) > 16.0 {
                bds_all += 1;
                bds_aimed += usize::from(aimed(from, to, target));
            }
        }
    }
    assert!(
        bds_aimed >= 5 && bds_all >= 9,
        "BDS {bds_aimed} of {bds_all}"
    );

    let (mut ours_aimed, mut ours_all) = (0, 0);
    for seed in 0..6 {
        let mut a = super::facets::floor(50 + seed, Difficulty::Normal, GameMode::Survival);
        let player_feet = DVec3::new(-6.5, 64.0, 8.5);
        a.move_player(player_feet);
        let home = DVec3::new(17.5, 64.0, 8.5);
        let mob = a.adult(ENDERMAN, home);
        freeze(&mut a, mob);
        let player = a.player;
        crate::entity::mob::ai::provoke(a.world(), mob, player);
        for _ in 0..200 {
            let before = a.feet(mob);
            a.ticks(1);
            let after = a.feet(mob);
            if after.distance(before) > 1.0 && before.distance(player_feet) > 16.0 {
                ours_all += 1;
                ours_aimed += usize::from(aimed(before, after, player_feet));
            }
            if a.world().get::<AttackTarget>(mob).is_none() {
                break;
            }
        }
    }
    assert!(ours_all >= 10, "only {ours_all} teleports");
    assert!(
        same_rate((ours_aimed, ours_all), (bds_aimed, bds_all), 3.0),
        "aimed: ours {ours_aimed} of {ours_all}, BDS {bds_aimed} of {bds_all}"
    );
}
