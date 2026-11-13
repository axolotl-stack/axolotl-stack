//! Status effect names and the player entry points of the shared effect
//! system (`entity::effects`): food, milk and other item use give, take and
//! clear effects through [`add_effect`], [`remove_effect`] and
//! [`clear_effects`]. The effect step, what effects do to a player and the
//! `MobEffect` packets are `entity::effects`, shared with mobs.

use bevy_ecs::prelude::*;

use crate::entity::components::{
    ActiveEffect, EffectType, Effects, Health, Hunger, RuntimeEntityId,
};

/// Bedrock effect identifiers and their protocol ids.
const EFFECT_NAMES: [(&str, u8); 36] = [
    ("speed", 1),
    ("slowness", 2),
    ("haste", 3),
    ("mining_fatigue", 4),
    ("strength", 5),
    ("instant_health", 6),
    ("instant_damage", 7),
    ("jump_boost", 8),
    ("nausea", 9),
    ("regeneration", 10),
    ("resistance", 11),
    ("fire_resistance", 12),
    ("water_breathing", 13),
    ("invisibility", 14),
    ("blindness", 15),
    ("night_vision", 16),
    ("hunger", 17),
    ("weakness", 18),
    ("poison", 19),
    ("wither", 20),
    ("health_boost", 21),
    ("absorption", 22),
    ("saturation", 23),
    ("levitation", 24),
    ("fatal_poison", 25),
    ("conduit_power", 26),
    ("slow_falling", 27),
    ("bad_omen", 28),
    ("village_hero", 29),
    ("darkness", 30),
    ("trial_omen", 31),
    ("wind_charged", 32),
    ("weaving", 33),
    ("oozing", 34),
    ("infested", 35),
    ("raid_omen", 36),
];

/// The effect named `name` (`minecraft:` prefix optional, case-insensitive
/// as pack JSON is lenient here).
pub fn effect_by_name(name: &str) -> Option<EffectType> {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    EFFECT_NAMES
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(_, id)| EffectType(*id))
}

/// Give `player` an effect (amplifier 0-based) through the shared effect
/// system. Instant health and an instant saturation from food act at once
/// and are not stored.
pub fn add_effect(
    world: &mut World,
    player: Entity,
    effect: EffectType,
    amplifier: u8,
    duration_ticks: u32,
) {
    match effect {
        EffectType::INSTANT_HEALTH => {
            heal(world, player, (4u32 << amplifier.min(30)) as f32);
            return;
        }
        EffectType::SATURATION if duration_ticks <= 1 => {
            saturate(world, player, amplifier);
            return;
        }
        _ => {}
    }
    crate::entity::effects::give(
        world,
        player,
        effect,
        ActiveEffect::new(amplifier, duration_ticks),
    );
}

/// Remove one effect from `player`.
pub fn remove_effect(world: &mut World, player: Entity, effect: EffectType) {
    crate::entity::effects::take(world, player, effect);
}

/// Remove every effect (milk).
pub fn clear_effects(world: &mut World, player: Entity) {
    crate::entity::effects::clear(world, player);
}

fn heal(world: &mut World, player: Entity, amount: f32) {
    if let Some(mut health) = world.get_mut::<Health>(player) {
        health.heal(amount);
    }
}

/// Saturation: `amplifier + 1` food points and twice that saturation.
fn saturate(world: &mut World, player: Entity, amplifier: u8) {
    if let Some(mut hunger) = world.get_mut::<Hunger>(player) {
        hunger.eat(i32::from(amplifier) + 1, 1.0);
    }
}

/// A player's visible-effects actor data (`VISIBLE_MOB_EFFECTS`): the shared
/// packing of [`crate::entity::effects::pack_visible_effects`] without
/// invisibility. Measured (`/effect` capture): speed alone packs 2, instant
/// health 12, effects with hidden particles nothing. Invisibility with
/// particles packs nothing either (the player actor-data capture's `/effect
/// invisibility` sends no visible-effects update); [INFERENCE] that is the
/// effect itself, not every effect while invisible.
pub fn visible_effects(effects: Option<&Effects>) -> i64 {
    effects.map_or(0, |effects| {
        crate::entity::effects::pack_visible_effects(
            effects
                .active
                .iter()
                .filter(|(effect, _)| **effect != EffectType::INVISIBILITY),
        )
    })
}

/// The visible-effects value a player's viewers last heard of (0 at join).
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SentVisibleEffects(pub i64);

/// System: after this tick's effect changes, tell every player whose
/// visible effects changed ([`update_visible_effects`]).
pub fn sync_visible_effects(world: &mut World) {
    let players: Vec<Entity> = world
        .query_filtered::<Entity, With<crate::entity::components::Player>>()
        .iter(world)
        .collect();
    for player in players {
        update_visible_effects(world, player);
    }
}

/// Tell `player` and its viewers its visible-effects actor data when it
/// changed since they last heard, as BDS follows a `MobEffect` with
/// `SetActorData` when that value changes (`/effect` runs it right after
/// its `MobEffect` packets).
pub fn update_visible_effects(world: &mut World, player: Entity) {
    use jolyne::valentine::actor_data_i_ds as actor_data_ids;
    use jolyne::valentine::types::{
        ActorRuntimeId, DataItemEntry, DataItemEntryPayload, DataItemInt64Payload,
        EnumsDataItemType, PropertySyncData, SynchedActorDataCopyableDataList,
    };
    let value = visible_effects(world.get::<Effects>(player));
    if world
        .get::<SentVisibleEffects>(player)
        .map_or(0, |sent| sent.0)
        == value
    {
        return;
    }
    world.entity_mut(player).insert(SentVisibleEffects(value));
    let Some(runtime_id) = world.get::<RuntimeEntityId>(player).map(|r| r.0) else {
        return;
    };
    let packet = jolyne::valentine::McpePacket::from(jolyne::valentine::SetActorDataPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id as u64,
        },
        actor_data: SynchedActorDataCopyableDataList {
            data: vec![DataItemEntry {
                id: actor_data_ids::VISIBLE_MOB_EFFECTS,
                payload: DataItemEntryPayload::DataItemInt64Payload(DataItemInt64Payload {
                    type_: EnumsDataItemType::Int64,
                    value,
                }),
            }],
        },
        synched_properties: PropertySyncData::default(),
        tick: crate::entity::components::input_tick_of(world, player),
    });
    crate::entity::player_life::send_to_player_and_viewers(world, player, &packet);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_names_resolve_to_protocol_ids() {
        assert_eq!(
            effect_by_name("regeneration"),
            Some(EffectType::REGENERATION)
        );
        assert_eq!(
            effect_by_name("minecraft:absorption"),
            Some(EffectType::ABSORPTION)
        );
        assert_eq!(effect_by_name("fatal_poison"), Some(EffectType(25)));
        assert_eq!(effect_by_name("not_an_effect"), None);
    }
}
