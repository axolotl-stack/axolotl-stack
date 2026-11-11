//! Hit shapes and effects: `minecraft:custom_hit_test`,
//! `minecraft:mob_effect_immunity` and `minecraft:spell_effects`, plus the
//! registration of `minecraft:follow_range` and
//! `minecraft:knockback_resistance` (`mob::attributes`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use bevy_ecs::world::EntityWorldMut;
use glam::DVec3;
use thiserror::Error;
use unastar_data::actor_components::body::{self, Vec3};

use super::common::insert;
use crate::entity::components::EffectType;
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::registry::collision::Aabb;

/// Why one of these bodies cannot be compiled.
#[derive(Debug, Error)]
pub enum StatsError {
    #[error("unknown mob effect {0:?}")]
    UnknownEffect(String),
    #[error("spell_effects: instant effect {0:?} is not implemented for mobs")]
    InstantSpellEffect(String),
    #[error("spell_effects: {0} is not a duration in seconds")]
    SpellDuration(f32),
    #[error("hitbox {width} × {height} is not a box")]
    Hitbox { width: f32, height: f32 },
}

impl ComponentError for StatsError {}

/// One `minecraft:custom_hit_test` box: `width` × `height` centred on the
/// actor's position plus `pivot`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitBox {
    pub width: f64,
    pub height: f64,
    pub pivot: DVec3,
}

impl HitBox {
    /// The box around an actor whose feet are at `feet`.
    pub fn at(&self, feet: DVec3) -> Aabb {
        let centre = feet + self.pivot;
        let half = DVec3::new(self.width, self.height, self.width) / 2.0;
        Aabb {
            min: centre - half,
            max: centre + half,
        }
    }
}

/// `minecraft:custom_hit_test`: boxes a player's hit may land on besides the
/// collision box. Viewers get them as the `HITBOX` actor data: a compound
/// with a `Hitboxes` list of `{MaxX, MaxY, MaxZ, MinX, MinY, MinZ, PivotX,
/// PivotY, PivotZ}` relative to the actor's position (an empty compound
/// without the component), as the native zoglin summon carries them.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct CustomHitTest(pub Arc<[HitBox]>);

/// The `HITBOX` actor data for `boxes`.
pub fn hitbox_actor_data(boxes: &[HitBox]) -> bytes::Bytes {
    use std::collections::BTreeMap;
    use unastar_packs::nbt::{Tag, encode_network_fields};
    if boxes.is_empty() {
        return bytes::Bytes::from_static(&[10, 0, 0]);
    }
    let list = boxes
        .iter()
        .map(|hitbox| {
            let local = hitbox.at(DVec3::ZERO);
            let float = |v: f64| Tag::Float(v as f32);
            Tag::Compound(BTreeMap::from([
                ("MaxX".to_owned(), float(local.max.x)),
                ("MaxY".to_owned(), float(local.max.y)),
                ("MaxZ".to_owned(), float(local.max.z)),
                ("MinX".to_owned(), float(local.min.x)),
                ("MinY".to_owned(), float(local.min.y)),
                ("MinZ".to_owned(), float(local.min.z)),
                ("PivotX".to_owned(), float(hitbox.pivot.x)),
                ("PivotY".to_owned(), float(hitbox.pivot.y)),
                ("PivotZ".to_owned(), float(hitbox.pivot.z)),
            ]))
        })
        .collect();
    let mut bytes = vec![10, 0];
    bytes.extend(
        encode_network_fields(&BTreeMap::from([("Hitboxes".to_owned(), Tag::List(list))]))
            .expect("hitbox floats encode"),
    );
    bytes.push(0);
    bytes::Bytes::from(bytes)
}

fn set_hitbox_data(entity: &mut EntityWorldMut<'_>, boxes: &[HitBox]) {
    use jolyne::valentine::bedrock::codec::Nbt;
    use jolyne::valentine::types::{
        DataItemCompoundTagPayload, DataItemEntryPayload, EnumsDataItemType,
    };
    if let Some(mut metadata) =
        entity.get_mut::<crate::entity::definition::metadata::EntityMetadata>()
    {
        metadata.set_payload(
            jolyne::valentine::actor_data_i_ds::HITBOX,
            DataItemEntryPayload::DataItemCompoundTagPayload(DataItemCompoundTagPayload {
                type_: EnumsDataItemType::Compoundtag,
                value: Nbt(hitbox_actor_data(boxes)),
            }),
        );
    }
}

/// `minecraft:mob_effect_immunity`: effects never applied to the mob.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct MobEffectImmunity(pub Arc<[EffectType]>);

/// `minecraft:spell_effects`: effects given to and taken from the mob when
/// the component is added. Effect names the game does not know are dropped,
/// as BDS drops them: the zombie villager's cure gives `heal`, which is not
/// an effect name (instant health is `instant_health`), so the cure heals
/// nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct SpellEffectsConfig {
    /// `(effect, amplifier, duration ticks)`.
    pub add: Arc<[(EffectType, u8, u32)]>,
    pub remove: Arc<[EffectType]>,
}

/// Whether `mob` is immune to `effect` (`minecraft:mob_effect_immunity`).
pub fn immune_to_effect(world: &World, mob: Entity, effect: EffectType) -> bool {
    world
        .get::<MobEffectImmunity>(mob)
        .is_some_and(|immunity| immunity.0.contains(&effect))
}

fn vec3(v: &Vec3) -> DVec3 {
    match v {
        Vec3::Array([x, y, z]) => DVec3::new(f64::from(*x), f64::from(*y), f64::from(*z)),
        Vec3::Vec3Object(v) => DVec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z)),
    }
}

/// Register the kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    crate::entity::mob::attributes::register_component_kinds(builder)?;
    builder.register(
        |b: body::CustomHitTest| compile_hit_test(b).map_err(Into::into),
        |config: &CustomHitTest, entity| {
            set_hitbox_data(entity, &config.0);
            entity.insert(config.clone());
        },
        |entity| {
            entity.remove::<CustomHitTest>();
            set_hitbox_data(entity, &[]);
        },
    )?;
    insert(builder, |b: body::MobEffectImmunity| {
        let effects = b
            .mob_effects
            .unwrap_or_default()
            .iter()
            .map(|name| {
                crate::server::game::effects::effect_by_name(name)
                    .ok_or_else(|| StatsError::UnknownEffect(name.clone()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok::<_, StatsError>(MobEffectImmunity(effects.into()))
    })?;
    builder.register(
        |b: body::SpellEffects| compile_spell_effects(b).map_err(Into::into),
        |config: &SpellEffectsConfig, entity| apply_spell_effects(config, entity),
        |_| {},
    )
}

fn compile_spell_effects(b: body::SpellEffects) -> Result<SpellEffectsConfig, StatsError> {
    use crate::server::game::effects::effect_by_name;
    let mut add = Vec::new();
    for spell in b.add_effects.unwrap_or_default() {
        let Some(effect) = effect_by_name(&spell.effect) else {
            continue;
        };
        if matches!(
            effect,
            EffectType::INSTANT_HEALTH | EffectType::INSTANT_DAMAGE
        ) {
            return Err(StatsError::InstantSpellEffect(spell.effect));
        }
        if !spell.duration.is_finite() || spell.duration < 0.0 {
            return Err(StatsError::SpellDuration(spell.duration));
        }
        let ticks = (spell.duration * crate::entity::mob::constants::TICKS_PER_SECOND) as u32;
        add.push((effect, spell.amplifier.clamp(0, 255) as u8, ticks));
    }
    let remove = b
        .remove_effects
        .unwrap_or_default()
        .iter()
        .filter_map(|name| crate::server::game::effects::effect_by_name(name))
        .collect::<Vec<_>>();
    Ok(SpellEffectsConfig {
        add: add.into(),
        remove: remove.into(),
    })
}

/// Give and take the configured effects through the shared effect system
/// (`entity::effects`; immune effects are not given).
fn apply_spell_effects(config: &SpellEffectsConfig, entity: &mut EntityWorldMut<'_>) {
    let mob = entity.id();
    entity.world_scope(|world| {
        for effect in config.remove.iter() {
            crate::entity::effects::take(world, mob, *effect);
        }
        for &(effect, amplifier, ticks) in config.add.iter() {
            crate::entity::effects::give(
                world,
                mob,
                effect,
                crate::entity::components::ActiveEffect::new(amplifier, ticks),
            );
        }
    });
}

fn compile_hit_test(b: body::CustomHitTest) -> Result<CustomHitTest, StatsError> {
    let boxes = b
        .hitboxes
        .iter()
        .map(|hitbox| {
            if !(hitbox.width > 0.0 && hitbox.height > 0.0) {
                return Err(StatsError::Hitbox {
                    width: hitbox.width,
                    height: hitbox.height,
                });
            }
            Ok(HitBox {
                width: f64::from(hitbox.width),
                height: f64::from(hitbox.height),
                pivot: vec3(&hitbox.pivot),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(CustomHitTest(boxes.into()))
}
