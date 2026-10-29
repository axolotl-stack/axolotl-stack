//! Firing a mob's authored events and answering its filters.
//!
//! [`fire_event`] evaluates one bound event, applies its effects in authored
//! order (component group changes go through the definition stack's apply
//! barrier, property assignments apply immediately) and then dispatches the
//! deferred `trigger`s whose filters pass, depth-first, with a recursion bound.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use thiserror::Error;
use unastar_data::actor_components::ActorComponent;
use unastar_rules::filter::{BuiltinTest, Fact, FilterContext, FilterTest, Subject};
use unastar_rules::molang::QueryId;
use unastar_rules::molang::eval::{MolangContext, Value as MolangValue};

use super::MobKind;
use super::archetype::MobArchetype;
use crate::entity::EntityRandom;
use crate::entity::components::{PhysicsBody, Player, PlayerState, Position};
use crate::entity::definition::events::{
    EventContext, EventEffect, EventEvaluationError, EventHandle, EventOutcome, FilterBinding,
    TriggerEvent,
};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::properties::{ActorProperties, PropertyError, PropertyValue};
use crate::entity::definition::{DefinitionState, GroupChange, queue_group_change};
use crate::entity::systems::BlockSource;
use crate::world::ecs::{ChunkData, ChunkManager, world_to_chunk_coords, world_to_local_coords};

/// Bound on nested `trigger` dispatch. Vanilla chains are at most three deep;
/// a cycle in addon data stops here instead of hanging the tick.
const MAX_TRIGGER_DEPTH: usize = 16;

/// Why a bound filter cannot be answered for a mob.
#[derive(Debug, PartialEq, Eq, Error)]
pub enum MobFilterError {
    #[error("filter evaluated on an actor without a mob archetype")]
    NotAMob,
    #[error("unknown filter binding {0}")]
    UnknownBinding(u32),
}

/// Why firing an event failed.
#[derive(Debug, Error)]
pub enum EventError {
    #[error("actor has no mob archetype")]
    NotAMob,
    #[error("{identifier}: event triggers nested deeper than {MAX_TRIGGER_DEPTH}")]
    TooDeep { identifier: std::sync::Arc<str> },
    #[error(transparent)]
    Evaluation(#[from] EventEvaluationError<MobFilterError>),
    #[error(transparent)]
    Property(#[from] PropertyError),
}

/// Facts about a mob (and optionally the other actor of an interaction) for
/// filter evaluation.
pub struct MobFacts<'w> {
    pub world: &'w World,
    pub entity: Entity,
    pub other: Option<Entity>,
}

impl MobFacts<'_> {
    fn subject(&self, subject: Subject) -> Option<Entity> {
        match subject {
            Subject::Self_ => Some(self.entity),
            Subject::Other => self.other,
            _ => None,
        }
    }

    fn biome_tags(&self, entity: Entity) -> Option<&'static [String]> {
        let position = self.world.get::<Position>(entity)?.0.floor().as_ivec3();
        let (cx, cz) = world_to_chunk_coords(position.x, position.z);
        let chunk = self.world.get::<ChunkData>(
            self.world
                .resource::<ChunkManager>()
                .get_by_coords(cx, cz)?,
        )?;
        let (lx, ly, lz) = world_to_local_coords(position.x, position.y, position.z);
        let biome = chunk.inner.biome_at(lx, ly, lz)?;
        unastar_data::biomes::vanilla()
            .iter()
            .find(|row| row.id == biome)
            .map(|row| row.tags.as_slice())
    }

    /// The block-space feet of `entity` (a player's position is its eye).
    fn feet(&self, entity: Entity) -> Option<DVec3> {
        let position = self.world.get::<Position>(entity)?.0;
        Some(if self.world.get::<Player>(entity).is_some() {
            position - DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT
        } else {
            position
        })
    }

    fn in_liquid(&self, entity: Entity, lava: bool) -> Option<bool> {
        let position: IVec3 = self.feet(entity)?.floor().as_ivec3();
        let state = self.world.block_at(position)?;
        Some(
            self.world
                .registry()
                .liquid_state(state)
                .is_some_and(|liquid| {
                    matches!(
                        (liquid.kind, lava),
                        (crate::registry::block::LiquidKind::Lava, true)
                            | (crate::registry::block::LiquidKind::Water, false)
                    )
                }),
        )
    }

    /// Where the actor breathes: a player's eye, a mob's head at
    /// [`MOB_EYE_HEIGHT_RATIO`](super::constants::MOB_EYE_HEIGHT_RATIO) of
    /// its box height above the feet.
    fn breathing_point(&self, entity: Entity) -> Option<DVec3> {
        let position = self.world.get::<Position>(entity)?.0;
        if self.world.get::<Player>(entity).is_some() {
            return Some(position);
        }
        let height = self.world.get::<PhysicsBody>(entity)?.height;
        Some(position + DVec3::Y * height * super::constants::MOB_EYE_HEIGHT_RATIO)
    }

    /// `is_underwater`: the block at the breathing point (the actor's own x
    /// and z) is water and the point lies below that water's surface (see
    /// [`UNDERWATER_LEVEL_STEPS`](super::constants::UNDERWATER_LEVEL_STEPS)).
    /// Only the block itself counts: water a block is logged with does not.
    fn under_water(&self, entity: Entity) -> Option<bool> {
        let point = self.breathing_point(entity)?;
        let block = point.floor().as_ivec3();
        let state = self.world.block_at(block)?;
        let Some(liquid) = self.world.registry().liquid_state(state) else {
            return Some(false);
        };
        if !matches!(liquid.kind, crate::registry::block::LiquidKind::Water) {
            return Some(false);
        }
        let level = if liquid.depth < 8 {
            liquid.depth + 1
        } else {
            1
        };
        let surface = f64::from(block.y) + 1.0
            - f64::from(level - 1) / super::constants::UNDERWATER_LEVEL_STEPS;
        Some(point.y < surface)
    }

    fn property(&self, entity: Entity, test: &FilterTest) -> Option<PropertyValue> {
        let properties = self.world.get::<ActorProperties>(entity)?;
        let slot = properties.layout().slot(test.domain_str()?)?;
        properties.value(&slot).ok()
    }
}

impl FilterContext for MobFacts<'_> {
    fn fact(&self, subject: Subject, test: &FilterTest) -> Option<Fact> {
        let entity = self.subject(subject)?;
        Some(match test.builtin()? {
            BuiltinTest::HasBiomeTag => {
                let want = test.value.as_str()?;
                Fact::Matches(self.biome_tags(entity)?.iter().any(|tag| tag == want))
            }
            BuiltinTest::InLava => Fact::Bool(self.in_liquid(entity, true)?),
            BuiltinTest::InWater => Fact::Bool(self.in_liquid(entity, false)?),
            BuiltinTest::IsUnderwater => Fact::Bool(self.under_water(entity)?),
            // Riding is not implemented: no actor ever has a vehicle.
            BuiltinTest::IsVehicleFamily => Fact::Matches(false),
            BuiltinTest::IsBaby => Fact::Bool(
                self.world
                    .get::<ActorFlags>(entity)
                    .is_some_and(|f| f.get(jolyne::valentine::actor_flags::BABY).unwrap_or(false)),
            ),
            BuiltinTest::IsFamily => {
                let want = test.value.as_str()?;
                Fact::Matches(is_family(self.world, entity, want))
            }
            BuiltinTest::EnumProperty => {
                let properties = self.world.get::<ActorProperties>(entity)?;
                let slot = properties.layout().slot(test.domain_str()?)?;
                let value = properties.value(&slot).ok()?;
                let name = properties.layout().enum_value(&slot, value)?;
                Fact::Matches(Some(name) == test.value.as_str())
            }
            BuiltinTest::BoolProperty => match self.property(entity, test)? {
                PropertyValue::Bool(value) => Fact::Bool(value),
                _ => return None,
            },
            BuiltinTest::IntProperty => match self.property(entity, test)? {
                PropertyValue::Int(value) => Fact::Number(f64::from(value)),
                _ => return None,
            },
            // Riding is not implemented: no actor ever has a rider.
            BuiltinTest::RiderCount => Fact::Number(0.0),
            BuiltinTest::HasComponent => {
                let component = ActorComponent::from_name(test.value.as_str()?)?;
                Fact::Matches(
                    self.world
                        .get::<DefinitionState>(entity)
                        .is_some_and(|state| state.has_actor_component(component)),
                )
            }
            // Only players sneak.
            BuiltinTest::IsSneakHeld => Fact::Bool(
                self.world
                    .get::<PlayerState>(entity)
                    .is_some_and(|state| state.sneaking),
            ),
            BuiltinTest::IsDifficulty => {
                let want = test.value.as_str()?;
                Fact::Matches(world_difficulty(self.world)? == want)
            }
            BuiltinTest::HasEquipment => {
                let want = super::components::ItemMatcher::parse(test.value.as_str()?).ok()?;
                let domain = test.domain_str().unwrap_or("any");
                if self.world.get::<MobKind>(entity).is_some() {
                    let slots = super::gear::domain_slots(domain)?;
                    return Some(Fact::Matches(super::gear::has_equipment(
                        self.world, entity, slots, &want,
                    )));
                }
                if !matches!(domain, "hand" | "any") {
                    return None;
                }
                let stack = held_item(self.world, entity);
                Fact::Matches(
                    stack.is_some_and(|stack| want.matches(stack.item_id(), stack.damage)),
                )
            }
            _ => return None,
        })
    }
}

impl EventContext for MobFacts<'_> {
    type Error = MobFilterError;

    fn filter(&self, binding: FilterBinding) -> Result<bool, MobFilterError> {
        let kind = self
            .world
            .get::<MobKind>(self.entity)
            .ok_or(MobFilterError::NotAMob)?;
        let filter = kind
            .0
            .filters
            .get(binding.0 as usize)
            .ok_or(MobFilterError::UnknownBinding(binding.0))?;
        Ok(filter.evaluate(self))
    }
}

/// Whether `entity` is in the named family. Players are always `player`.
pub fn is_family(world: &World, entity: Entity, family: &str) -> bool {
    if world
        .get::<crate::entity::components::Player>(entity)
        .is_some()
    {
        return family == "player";
    }
    let Some(families) = world.get::<crate::entity::definition::families::ActorFamilies>(entity)
    else {
        return false;
    };
    let Some(id) = world
        .resource::<super::MobRegistry>()
        .families()
        .resolve(family)
    else {
        return false;
    };
    families.contains(&id).unwrap_or(false)
}

/// The world difficulty as filters name it.
fn world_difficulty(world: &World) -> Option<&'static str> {
    Some(
        world
            .get_resource::<crate::world::rules::Difficulty>()?
            .name(),
    )
}

/// The non-empty stack in a player's selected hotbar slot.
pub fn held_item(world: &World, player: Entity) -> Option<&crate::item::ItemStack> {
    let slot = world.get::<crate::entity::components::HeldSlot>(player)?.0;
    world
        .get::<crate::entity::components::MainInventory>(player)?
        .hotbar(slot)
        .filter(|stack| !stack.is_empty())
}

/// Molang context for property assignments: random numbers only. Queries
/// evaluate to 0 (the vanilla passive-mob events assign literals).
struct PropertyMolang<'a> {
    rng: &'a mut StdRng,
}

impl MolangContext for PropertyMolang<'_> {
    fn query(&mut self, _: QueryId, _: &[MolangValue]) -> Option<MolangValue> {
        None
    }
    fn variable(&self, _: &str) -> Option<MolangValue> {
        None
    }
    fn set_variable(&mut self, _: &str, _: MolangValue) {}
    fn context(&self, _: &str) -> Option<MolangValue> {
        None
    }
    fn this(&self) -> MolangValue {
        MolangValue::Num(0.0)
    }
    fn enter(&mut self, _: u64) -> bool {
        false
    }
    fn leave(&mut self) {}
    fn random(&mut self) -> f32 {
        self.rng.r#gen()
    }
}

/// Fire `event` on `entity` (and the self-triggers it causes). Group changes
/// take effect at the next definition apply barrier; call
/// [`crate::entity::definition::apply_pending_definitions`] to materialise
/// them immediately.
pub fn fire_event(
    world: &mut World,
    entity: Entity,
    event: &EventHandle,
    other: Option<Entity>,
) -> Result<(), EventError> {
    let archetype = world
        .get::<MobKind>(entity)
        .map(|kind| std::sync::Arc::clone(&kind.0))
        .ok_or(EventError::NotAMob)?;
    let seed = world.resource_mut::<EntityRandom>().0.r#gen::<u64>();
    let mut rng = StdRng::seed_from_u64(seed);
    fire_inner(world, entity, &archetype, event, other, &mut rng, 0)
}

/// The groups an event changes come from the archetype's own definition,
/// which `spawn_mob` attached to the mob before any event could fire.
const SAME_DEFINITION: &str = "a mob's definition stack is its archetype's definition";

fn fire_inner(
    world: &mut World,
    entity: Entity,
    archetype: &MobArchetype,
    event: &EventHandle,
    other: Option<Entity>,
    rng: &mut StdRng,
    depth: usize,
) -> Result<(), EventError> {
    if depth > MAX_TRIGGER_DEPTH {
        return Err(EventError::TooDeep {
            identifier: std::sync::Arc::clone(&archetype.identifier),
        });
    }
    let mut outcome = EventOutcome::default();
    let mut scratch = Vec::new();
    {
        let facts = MobFacts {
            world,
            entity,
            other,
        };
        archetype.events.evaluate(
            event,
            &facts,
            &mut || rng.r#gen(),
            &mut outcome,
            &mut scratch,
        )?;
    }
    for effect in &outcome.effects {
        match *effect {
            EventEffect::AddGroup(group) => {
                queue_group_change(world, entity, GroupChange::Add(group.clone()))
                    .expect(SAME_DEFINITION);
            }
            EventEffect::RemoveGroup(group) => {
                queue_group_change(world, entity, GroupChange::Remove(group.clone()))
                    .expect(SAME_DEFINITION);
            }
            EventEffect::SetProperty(assignment) => {
                world
                    .get_mut::<ActorProperties>(entity)
                    .expect("every mob carries ActorProperties from spawn_mob")
                    .apply(assignment, &mut PropertyMolang { rng })?;
            }
            EventEffect::Action { kind, .. } => {
                unreachable!(
                    "event action {} was bound, but mob archetypes bind no actions",
                    kind.key()
                );
            }
        }
    }
    let mut due = Vec::with_capacity(outcome.triggers.len());
    {
        let facts = MobFacts {
            world,
            entity,
            other,
        };
        for trigger in &outcome.triggers {
            if trigger.passes(&facts)? {
                match &trigger.event {
                    TriggerEvent::Internal(handle) => due.push(handle.clone()),
                    TriggerEvent::External(_) => {
                        unreachable!("mob archetypes bind no external event triggers")
                    }
                }
            }
        }
    }
    for handle in due {
        fire_inner(world, entity, archetype, &handle, other, rng, depth + 1)?;
    }
    Ok(())
}
