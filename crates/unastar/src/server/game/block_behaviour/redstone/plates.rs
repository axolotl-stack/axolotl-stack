//! Pressure plates (`redstone_signal`).
//!
//! An entity whose box reaches into the plate's sensing box (x and z
//! 0.125..0.875, y 0..0.25 of the cell; provisional: Minecraft Wiki)
//! presses it, as BDS 1.26.51 counts them:
//!
//! - wooden plates (every wood): any entity, items included;
//! - stone and polished blackstone plates: players and mobs, never items
//!   (which plates count mobs is provisional: Minecraft Wiki);
//! - weighted plates: `⌈n / max × 15⌉` for `n` entities (at most `max`),
//!   `max` 15 for the light (gold) plate and 150 for the heavy (iron) one.
//!
//! Experience orbs and spectators never press a plate. A pressed wooden or
//! stone plate stores `redstone_signal` 1 and gives 15; a weighted plate
//! stores and gives its level.
//!
//! [`press_plates`] presses a released plate the tick an entity enters it.
//! A pressed plate looks again every 20 game ticks (10 for weighted
//! plates) from the press and releases at the first look that finds
//! nothing, as BDS 1.26.51 does: an item killed or a player stepping off
//! 59 ticks after the press releases the plate one tick later, at the
//! third look; one leaving after 47 ticks waits 14 ticks for it. Turning on
//! plays `pressure_plate.click_on`, turning off `pressure_plate.click_off`
//! at (x + 0.5, y + 0.1, z + 0.5), carrying the plate's new state. The
//! plate gives weak power around and strong power to the block below.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};

use super::super::with;
use super::{Kind, Plate, kind, power};
use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::{
    DroppedItem, GameMode, PhysicsBody, Player, PlayerSession, Position,
};
use crate::entity::mob::MobKind;
use crate::entity::mob::state::Dead;
use crate::entity::xp_orb::XpOrb;
use crate::registry::block::StateValue;
use crate::registry::collision::Aabb;

/// Game ticks between the looks of a pressed wooden or stone plate.
pub const PLATE_INTERVAL: u64 = 20;
/// Game ticks between the looks of a pressed weighted plate.
pub const WEIGHTED_INTERVAL: u64 = 10;

/// What presses plates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presser {
    /// Players and mobs.
    Living,
    /// Items and every other entity with a body.
    Other,
}

/// Box and kind of every entity that can press a plate.
fn pressers(world: &mut World) -> Vec<(Aabb, Presser)> {
    let mut query = world.query::<(
        &Position,
        Option<&PhysicsBody>,
        Has<Player>,
        Has<PlayerSession>,
        Option<&GameMode>,
        Has<MobKind>,
        Has<DroppedItem>,
        Has<Dead>,
        Has<XpOrb>,
    )>();
    query
        .iter(world)
        .filter_map(
            |(position, body, player, session, mode, mob, item, dead, orb)| {
                if !position.0.is_finite() || dead || orb {
                    return None;
                }
                if player || session {
                    if mode == Some(&GameMode::Spectator) {
                        return None;
                    }
                    let feet = position.0 - DVec3::Y * PLAYER_EYE_HEIGHT;
                    return Some((Aabb::from_feet(feet, 0.6, 1.8), Presser::Living));
                }
                let body = body?;
                let presser = if mob && !item {
                    Presser::Living
                } else {
                    Presser::Other
                };
                Some((
                    Aabb::from_feet(position.0, body.width, body.height),
                    presser,
                ))
            },
        )
        .collect()
}

/// The sensing box of a plate at `pos`.
fn sensing_box(pos: IVec3) -> Aabb {
    let base = pos.as_dvec3();
    Aabb {
        min: base + DVec3::new(0.125, 0.0, 0.125),
        max: base + DVec3::new(0.875, 0.25, 0.875),
    }
}

/// The signal a plate of `plate` kind at `pos` gets from `pressers`.
fn signal(plate: Plate, pos: IVec3, pressers: &[(Aabb, Presser)]) -> u8 {
    let sensing = sensing_box(pos);
    let on = pressers.iter().filter(|(aabb, presser)| {
        aabb.intersects(&sensing) && (plate != Plate::Stone || *presser == Presser::Living)
    });
    match plate {
        Plate::Wooden | Plate::Stone => u8::from(on.count() > 0),
        Plate::Light => weighted(on.count(), LIGHT_MAX_WEIGHT),
        Plate::Heavy => weighted(on.count(), HEAVY_MAX_WEIGHT),
    }
}

/// Entities a light weighted plate counts up to.
pub const LIGHT_MAX_WEIGHT: usize = 15;
/// Entities a heavy weighted plate counts up to.
pub const HEAVY_MAX_WEIGHT: usize = 150;

/// `⌈n / max × 15⌉` with `n` capped at `max`.
fn weighted(n: usize, max: usize) -> u8 {
    (n.min(max) * usize::from(power::MAX)).div_ceil(max) as u8
}

/// Look at the plate at `pos`: set its signal from the entities on it and,
/// while pressed, look again after its interval.
pub fn check(world: &mut World, pos: IVec3) {
    let pressers = pressers(world);
    check_with(world, pos, &pressers);
}

fn check_with(world: &mut World, pos: IVec3, pressers: &[(Aabb, Presser)]) {
    let Some((entry, state)) = power::at(world, pos) else {
        return;
    };
    let Some(Kind::Plate(plate)) = kind(entry) else {
        return;
    };
    let current = power::level(entry, state);
    let target = signal(plate, pos, pressers);
    if target != current {
        let next = with(
            entry,
            state,
            super::SIGNAL,
            StateValue::Int(i32::from(target)),
        );
        crate::server::game::blocks::set_block(world, pos, next, false);
        if (target > 0) != (current > 0) {
            let sound = if target > 0 {
                "pressure_plate.click_on"
            } else {
                "pressure_plate.click_off"
            };
            crate::entity::mob::net::sound::play_level_sound(
                world,
                None,
                pos.as_dvec3() + DVec3::new(0.5, 0.1, 0.5),
                sound,
                crate::server::game::types::block_network_ids(world).to_wire(next) as i32,
            );
        }
    }
    if target > 0 {
        let interval = match plate {
            Plate::Light | Plate::Heavy => WEIGHTED_INTERVAL,
            Plate::Wooden | Plate::Stone => PLATE_INTERVAL,
        };
        super::super::schedule_tick(world, pos, interval);
    }
}

/// Exclusive system: a released plate under an entity's feet is pressed
/// the tick the entity enters it.
pub fn press_plates(world: &mut World) {
    let pressers = pressers(world);
    let mut cells: Vec<IVec3> = Vec::new();
    for (aabb, _) in &pressers {
        let y = aabb.min.y.floor() as i32;
        let (x0, x1) = (aabb.min.x.floor() as i32, aabb.max.x.floor() as i32);
        let (z0, z1) = (aabb.min.z.floor() as i32, aabb.max.z.floor() as i32);
        for x in x0..=x1 {
            for z in z0..=z1 {
                let cell = IVec3::new(x, y, z);
                if !cells.contains(&cell) {
                    cells.push(cell);
                }
            }
        }
    }
    for cell in cells {
        let released = power::at(world, cell).is_some_and(|(entry, state)| {
            matches!(kind(entry), Some(Kind::Plate(_))) && power::level(entry, state) == 0
        });
        if released {
            check_with(world, cell, &pressers);
        }
    }
}
