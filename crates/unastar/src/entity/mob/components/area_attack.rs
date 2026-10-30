//! `minecraft:area_attack` (run by `mob::combat::area_attack`).

use bevy_ecs::prelude::*;
use unastar_data::actor_components::body::{self, ActorDamageCause};
use unastar_rules::filter::Filter;

use super::common::{filter, insert};
use super::hostile::HostileError;
use crate::entity::definition::{ComponentKindsBuilder, DefinitionError};
use crate::entity::mob::constants::TICKS_PER_SECOND;

/// `minecraft:area_attack`: every tick the mob damages each actor passing
/// `filter` (`other` = the actor) whose box touches its own box grown by
/// `range` on every side, at most once per `cooldown_ticks` per actor.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct AreaAttackConfig {
    pub cause: ActorDamageCause,
    pub damage: f32,
    pub range: f64,
    /// 0: every tick (the hurt cooldown still applies).
    pub cooldown_ticks: u64,
    pub filter: Filter,
    /// Play the attack sound on every hit.
    pub play_attack_sound: bool,
}

/// Register the area attack component kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, |b: body::AreaAttack| {
        if b.death_message_override.is_some() {
            return Err(HostileError::AreaAttackDeathMessage);
        }
        if b.use_self_as_damage_source.is_some() {
            return Err(HostileError::AreaAttackDamageSource);
        }
        Ok::<_, HostileError>(AreaAttackConfig {
            cause: b.cause.unwrap_or(ActorDamageCause::None),
            damage: b.damage_per_tick as f32,
            range: f64::from(b.damage_range),
            cooldown_ticks: (b.damage_cooldown.max(0.0) * TICKS_PER_SECOND) as u64,
            filter: match &b.entity_filter {
                Some(json) => filter(json)?,
                None => Filter::always(),
            },
            play_attack_sound: b.play_attack_sound,
        })
    })
}
