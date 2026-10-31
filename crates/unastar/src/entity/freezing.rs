//! Living actors accumulate cold in powder snow, thaw after leaving it and
//! take periodic freezing damage at full exposure.

use bevy_ecs::prelude::*;
use body::ActorDamageCause;
use glam::DVec3;
use jolyne::valentine::types::{ActorRuntimeId, PropertySyncData};
use jolyne::valentine::{McpePacket, SetActorDataPacket, actor_data_i_ds, actor_flags};
use unastar_data::actor_components::{ActorComponent, body};

use crate::ecs::resources::TickCounter;
use crate::entity::components::{
    ArmourInventory, Health, PhysicsBody, Player, Position, RuntimeEntityId,
};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::{ComponentKindsBuilder, DefinitionError, DefinitionState};
use crate::entity::mob::components::{KnockbackPower, common::insert_infallible};
use crate::entity::mob::gear::{GearSlot, MobEquipment};
use crate::entity::mob::state::Dead;
use crate::entity::mob::{MobKind, combat};
use crate::entity::systems::BlockSource;
use crate::registry::collision::Aabb;

const FREEZE_INCREMENT: f32 = 1.0 / 140.0;
const THAW_INCREMENT: f32 = 2.0 / 140.0;
const DAMAGE_INTERVAL: u64 = 40;
const MOVEMENT_PENALTY: f32 = 0.05;

/// A definition immune to accumulating cold from powder snow.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreezingImmune;

/// A definition taking five times the ordinary periodic freezing damage.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct FreezingVulnerable;

/// Current cold exposure, independent of the actor's authored components.
/// Retaining the zero state allows movement and renderer state to be restored
/// exactly once on the final thaw tick.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
#[component(storage = "SparseSet")]
pub struct FreezingState {
    strength: f32,
}

/// A persisted cold value is outside the finite renderer scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("cold exposure must be finite and between zero and one")]
pub struct InvalidColdExposure;

impl FreezingState {
    /// Validate persisted or snapshot exposure before applying any actor state.
    pub fn from_strength(strength: f32) -> Result<Self, InvalidColdExposure> {
        if !strength.is_finite() || !(0.0..=1.0).contains(&strength) {
            return Err(InvalidColdExposure);
        }
        Ok(Self { strength })
    }
    /// Cold exposure on the renderer's zero-to-one scale.
    pub fn strength(self) -> f32 {
        self.strength
    }

    /// Whether the exposure is at the damage and shaking threshold.
    pub fn is_frozen(self) -> bool {
        (self.strength - 1.0).abs() <= f32::EPSILON
    }

    /// The cold contribution to movement, preserving the caller's base speed.
    pub fn movement(self, base: f32) -> f32 {
        (base + self.movement_adjustment()).max(0.0)
    }

    /// The additive movement modifier represented by this cold exposure.
    pub fn movement_adjustment(self) -> f32 {
        -self.strength * MOVEMENT_PENALTY
    }

    /// One server tick of exposure or thawing, bounded to the renderer scale.
    fn advance(self, exposed: bool, immune: bool) -> Self {
        Self {
            strength: if exposed && !immune {
                (self.strength + FREEZE_INCREMENT).min(1.0)
            } else {
                (self.strength - THAW_INCREMENT).max(0.0)
            },
        }
    }
}

/// Refresh renderer, flags and movement after a validated snapshot restores
/// cold state. Actors quarantined without their position or health are skipped.
pub fn refresh(world: &mut World, actor: Entity) {
    if world.get::<Position>(actor).is_none() || world.get::<Health>(actor).is_none() {
        return;
    }
    let state = world
        .get::<FreezingState>(actor)
        .copied()
        .unwrap_or_default();
    publish(world, actor, state);
}

/// Register both cold-condition marker factories with real runtime effects.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert_infallible(builder, |_: body::FreezingImmune| FreezingImmune)?;
    insert_infallible(builder, |_: body::FreezingVulnerable| FreezingVulnerable)
}

/// Whether an equipped armour piece protects its wearer against freezing.
fn leather(identifier: &str) -> bool {
    matches!(
        identifier,
        "minecraft:leather_helmet"
            | "minecraft:leather_chestplate"
            | "minecraft:leather_leggings"
            | "minecraft:leather_boots"
    )
}

/// Read actual equipped armour; held and carried items do not grant immunity.
fn protected_by_equipment(world: &World, actor: Entity) -> bool {
    if world.get::<ArmourInventory>(actor).is_some_and(|armour| {
        armour
            .slots()
            .iter()
            .any(|item| !item.is_empty() && leather(item.item_id()))
    }) {
        return true;
    }
    world.get::<MobEquipment>(actor).is_some_and(|gear| {
        [
            GearSlot::Head,
            GearSlot::Chest,
            GearSlot::Legs,
            GearSlot::Feet,
        ]
        .into_iter()
        .any(|slot| {
            let item = gear.get(slot);
            !item.is_empty() && leather(item.item_id())
        })
    })
}

/// Test the loaded block volumes the actor actually overlaps, excluding
/// touching faces so standing next to powder snow is not exposure.
fn in_powder_snow(world: &World, bounds: Aabb, powder_snow: u32) -> bool {
    let min = (bounds.min + DVec3::splat(0.001)).floor().as_ivec3();
    let max = (bounds.max - DVec3::splat(0.001)).floor().as_ivec3();
    (min.x..=max.x).any(|x| {
        (min.y..=max.y).any(|y| {
            (min.z..=max.z).any(|z| world.block_at(glam::IVec3::new(x, y, z)) == Some(powder_snow))
        })
    })
}

/// Publish the cold state using each actor lane's normal wire representation.
fn publish(world: &mut World, actor: Entity, state: FreezingState) {
    if world.get::<Player>(actor).is_some() {
        let Some(runtime_id) = world.get::<RuntimeEntityId>(actor).map(|id| id.0 as u64) else {
            return;
        };
        let using = world
            .get::<crate::server::game::item_use::ItemUsing>(actor)
            .is_some();
        let flags = crate::server::game::item_use::player_flags(world, actor, using);
        let mut metadata = EntityMetadata::with_capacity(2);
        metadata.set_long(actor_data_i_ds::RESERVED_0, flags);
        metadata.set_float(actor_data_i_ds::FREEZING_EFFECT_STRENGTH, state.strength);
        crate::entity::player_life::send_to_player_and_viewers(
            world,
            actor,
            &McpePacket::from(SetActorDataPacket {
                target_runtime_id: ActorRuntimeId {
                    actor_runtime_id: runtime_id,
                },
                actor_data: metadata.snapshot(),
                synched_properties: PropertySyncData::default(),
                tick: crate::entity::components::input_tick_of(world, actor),
            }),
        );
        crate::entity::player_life::attributes::send_attributes(
            world,
            actor,
            vec![crate::entity::player_life::attributes::movement_with_cold(
                state,
            )],
        );
    } else {
        let authored_shaking = world
            .get::<DefinitionState>(actor)
            .is_some_and(|definition| definition.has_actor_component(ActorComponent::IsShaking));
        if let Some(mut flags) = world.get_mut::<ActorFlags>(actor) {
            flags
                .update(actor_flags::SHAKING, authored_shaking || state.is_frozen())
                .expect("generated shaking flag");
        }
        crate::entity::mob::derive::derive_entity(world, actor);
    }
}

type LivingActors = QueryState<
    (
        Entity,
        &'static Position,
        Option<&'static PhysicsBody>,
        Option<&'static FreezingState>,
    ),
    (
        Or<(With<Player>, With<MobKind>)>,
        With<Health>,
        Without<Dead>,
        Without<crate::entity::components::Projectile>,
    ),
>;

/// Advance loaded living actors once per server tick. Damage goes through the
/// existing authoritative damage paths, preserving game-mode, timer and sensor
/// rules. An actor's lifetime ticks determine the periodic damage cadence;
/// manually constructed actors without an age use the world tick instead.
pub fn tick(
    world: &mut World,
    actors: &mut LivingActors,
    mut due: Local<Vec<(Entity, Aabb, FreezingState)>>,
) {
    let Some(powder_snow) = world
        .registry()
        .default_state_id_by_name("minecraft:powder_snow")
    else {
        return;
    };
    let player_rules = world.resource::<crate::entity::player_life::PlayerRules>();
    let (player_width, player_height) = (player_rules.width, player_rules.height);
    due.extend(
        actors
            .iter(world)
            .filter_map(|(actor, position, body, state)| {
                if world.get::<Health>(actor).is_none_or(Health::is_dead) {
                    return None;
                }
                let (feet, width, height) = if world.get::<Player>(actor).is_some() {
                    (
                        position.0 - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT,
                        player_width,
                        player_height,
                    )
                } else {
                    let body = body?;
                    (position.0, body.width, body.height)
                };
                Some((
                    actor,
                    Aabb::from_feet(feet, width, height),
                    state.copied().unwrap_or_default(),
                ))
            }),
    );
    let now = world.resource::<TickCounter>().current;
    for (actor, bounds, before) in due.drain(..) {
        let immune =
            world.get::<FreezingImmune>(actor).is_some() || protected_by_equipment(world, actor);
        let exposed = in_powder_snow(world, bounds, powder_snow);
        let next = before.advance(exposed, immune);
        if next != before {
            world.entity_mut(actor).insert(next);
            publish(world, actor, next);
        }
        let actor_tick = world
            .get::<crate::entity::components::Age>(actor)
            .map_or(now, |age| age.0);
        if !next.is_frozen() || immune || !actor_tick.is_multiple_of(DAMAGE_INTERVAL) {
            continue;
        }
        let damage = if world.get::<FreezingVulnerable>(actor).is_some() {
            5.0
        } else {
            1.0
        };
        let hit = combat::Hit {
            damage,
            cause: ActorDamageCause::Freezing,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: KnockbackPower::DEFAULT,
        };
        if world.get::<Player>(actor).is_some() {
            crate::entity::player_life::hurt_player(world, actor, &hit);
        } else {
            combat::hurt_mob(world, actor, hit);
        }
    }
}

/// Restore ordinary movement and renderer state when a player respawns.
pub(crate) fn reset_player(world: &mut World, player: Entity) {
    if world
        .get::<FreezingState>(player)
        .is_some_and(|state| state.strength > 0.0)
    {
        let state = FreezingState::default();
        world.entity_mut(player).insert(state);
        publish(world, player, state);
    }
}

#[cfg(test)]
mod tests;
