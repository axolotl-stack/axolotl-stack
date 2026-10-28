//! Fall damage of mobs.
//!
//! The physics step keeps each mob's [`FallDistance`] and records it on the
//! landing tick. [`apply_fall_damage`] then applies the native landing rule:
//! the block `0.2` below the feet may adjust the distance and multiplier
//! ([`LandingRule`]); a mob landing after more than 3 blocks on a solid
//! block shows ground dust; the damage is
//! `ceil((distance - 3 - jump boost) × fall multiplier × block multiplier)`
//! where the fall multiplier is the mob's `FALL_DAMAGE_MULTIPLIER` actor
//! data. Positive damage plays `fall.big` (above 4) or `fall.small` at the
//! feet, hurts the mob (its damage sensor may cancel that) and plays the
//! landing block's `fall` sound.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::actor_data_i_ds as ids;
use jolyne::valentine::types::{DataItemEntryPayload, EnumsActorEvent};
use unastar_data::actor_components::body::ActorDamageCause;

use super::MobKind;
use super::combat::{Hit, KnockbackPower, hurt_mob, send_actor_event};
use super::constants::{
    FALL_BIG_SOUND_DAMAGE, FALL_DUSTLESS_BLOCKS, FALL_LANDING_DISTANCE_SCALES, FALL_LANDING_PROBE,
    FALL_SAFE_DISTANCE, STALAGMITE_EXTRA_DISTANCE, STALAGMITE_MULTIPLIER,
};
use super::sound::play_level_sound;
use super::state::Dead;
use crate::entity::components::{EffectType, Effects, FallDistance, Position};
use crate::entity::metadata::EntityMetadata;
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::block::StateValue;

/// How a landing block changes a fall.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LandingRule {
    /// Distance and multiplier unchanged.
    Default,
    /// Liquids: no fall damage.
    NoDamage,
    /// The distance is scaled (hay 0.2, honey and beds 0.5; slime, powder
    /// snow and end portals 0).
    ScaleDistance(f64),
    /// An upward pointed dripstone tip: `distance + 2`, multiplier 2, cause
    /// stalagmite.
    Stalagmite,
}

/// Landing rule and dust flag per block runtime ID.
pub struct LandingTable {
    rules: Vec<(LandingRule, bool)>,
}

impl LandingTable {
    pub fn new(registry: &BlockRegistry) -> Self {
        let mut rules = Vec::new();
        for entry in registry.iter() {
            let scaled = FALL_LANDING_DISTANCE_SCALES
                .iter()
                .find(|(name, _)| *name == entry.string_id)
                .map(|(_, scale)| *scale);
            let dust = !FALL_DUSTLESS_BLOCKS.contains(&entry.string_id.as_str());
            for id in entry.min_state_id..=entry.max_state_id {
                let index = id as usize;
                if rules.len() <= index {
                    rules.resize(index + 1, (LandingRule::Default, true));
                }
                let rule = if registry.liquid_state(id).is_some() {
                    LandingRule::NoDamage
                } else if entry.string_id == "minecraft:pointed_dripstone" {
                    let tip = entry.state_value(id, "dripstone_thickness")
                        == Some(StateValue::Str("tip"));
                    let hanging = entry.state_value(id, "hanging") == Some(StateValue::Byte(1));
                    if tip && !hanging {
                        LandingRule::Stalagmite
                    } else {
                        LandingRule::Default
                    }
                } else {
                    scaled.map_or(LandingRule::Default, LandingRule::ScaleDistance)
                };
                rules[index] = (rule, dust);
            }
        }
        Self { rules }
    }

    fn get(&self, runtime_id: u32) -> (LandingRule, bool) {
        self.rules
            .get(runtime_id as usize)
            .copied()
            .unwrap_or((LandingRule::Default, true))
    }
}

/// One landing to resolve (the system's scratch list).
pub struct Landing {
    mob: Entity,
    feet: DVec3,
    distance: f64,
}

/// System (exclusive, after physics): resolve this tick's landings.
#[allow(clippy::type_complexity)]
pub fn apply_fall_damage(
    world: &mut World,
    mobs: &mut QueryState<(Entity, &Position, &FallDistance), (With<MobKind>, Without<Dead>)>,
    mut table: Local<Option<LandingTable>>,
    mut due: Local<Vec<Landing>>,
) {
    due.extend(mobs.iter(world).filter_map(|(mob, position, fall)| {
        fall.landed.map(|distance| Landing {
            mob,
            feet: position.0,
            distance,
        })
    }));
    if due.is_empty() {
        return;
    }
    let table = table.get_or_insert_with(|| LandingTable::new(world.registry()));
    for landing in due.drain(..) {
        land(world, table, landing);
    }
}

fn land(world: &mut World, table: &LandingTable, landing: Landing) {
    let probe = IVec3::new(
        landing.feet.x.floor() as i32,
        (landing.feet.y - FALL_LANDING_PROBE).floor() as i32,
        landing.feet.z.floor() as i32,
    );
    let block = world.block_at(probe);
    let (rule, dust) = block.map_or((LandingRule::Default, false), |id| table.get(id));
    let (distance, multiplier, cause) = match rule {
        LandingRule::NoDamage => return,
        LandingRule::Default => (landing.distance, 1.0, ActorDamageCause::Fall),
        LandingRule::ScaleDistance(scale) => {
            (landing.distance * scale, 1.0, ActorDamageCause::Fall)
        }
        LandingRule::Stalagmite => (
            landing.distance + STALAGMITE_EXTRA_DISTANCE,
            STALAGMITE_MULTIPLIER,
            ActorDamageCause::Stalagmite,
        ),
    };
    if dust && distance > FALL_SAFE_DISTANCE {
        send_actor_event(world, landing.mob, EnumsActorEvent::GroundDust, 0);
    }
    let jump_boost = world
        .get::<Effects>(landing.mob)
        .and_then(|effects| effects.get(EffectType::JUMP_BOOST))
        .map_or(0.0, |effect| f64::from(effect.level) + 1.0);
    let actor_multiplier = world
        .get::<EntityMetadata>(landing.mob)
        .and_then(|metadata| match metadata.get(ids::FALL_DAMAGE_MULTIPLIER) {
            Some(DataItemEntryPayload::DataItemFloatPayload(value)) => Some(f64::from(value.value)),
            _ => None,
        })
        .unwrap_or(1.0);
    let damage =
        ((distance - FALL_SAFE_DISTANCE - jump_boost) * actor_multiplier * multiplier).ceil();
    if damage <= 0.0 {
        return;
    }
    let sound = if damage > FALL_BIG_SOUND_DAMAGE {
        "fall.big"
    } else {
        "fall.small"
    };
    play_level_sound(world, Some(landing.mob), landing.feet, sound, -1);
    hurt_mob(
        world,
        landing.mob,
        Hit {
            damage: damage as f32,
            cause,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: KnockbackPower::DEFAULT,
        },
    );
    if let Some(block) = block {
        play_level_sound(world, Some(landing.mob), landing.feet, "fall", block as i32);
    }
}
