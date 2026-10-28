//! Firing a mob's authored events and answering its filters.
//!
//! [`fire_event`] evaluates one bound event, applies its effects in authored
//! order (component group changes go through the definition stack's apply
//! barrier, property assignments apply immediately) and then dispatches the
//! deferred `trigger`s whose filters pass, depth-first, with a recursion bound.

use bevy_ecs::prelude::*;
use glam::IVec3;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use unastar_rules::filter::{BuiltinTest, Fact, FilterContext, FilterTest, Subject};
use unastar_rules::molang::QueryId;
use unastar_rules::molang::eval::{MolangContext, Value as MolangValue};

use super::MobKind;
use super::archetype::MobArchetype;
use crate::entity::EntityRandom;
use crate::entity::components::Position;
use crate::entity::definition::{GroupChange, queue_group_change};
use crate::entity::events::{EventContext, EventEffect, EventHandle, EventOutcome, TriggerEvent};
use crate::entity::flags::ActorFlags;
use crate::entity::properties::{ActorProperties, PropertyValue};
use crate::entity::systems::BlockSource;
use crate::world::ecs::{ChunkData, ChunkManager, world_to_chunk_coords, world_to_local_coords};

/// Bound on nested `trigger` dispatch. Vanilla chains are at most three deep;
/// a cycle in addon data stops here instead of hanging the tick.
const MAX_TRIGGER_DEPTH: usize = 16;

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

    fn in_liquid(&self, entity: Entity, lava: bool) -> Option<bool> {
        let position: IVec3 = self.world.get::<Position>(entity)?.0.floor().as_ivec3();
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
            BuiltinTest::HasEquipment => {
                let want = super::config::ItemMatcher::parse(test.value.as_str()?).ok()?;
                let domain = test.domain_str().unwrap_or("any");
                if !matches!(domain, "hand" | "any") {
                    return None;
                }
                let stack = held_item(self.world, entity);
                Fact::Matches(stack.is_some_and(|stack| want.matches(&stack.item_id, stack.damage)))
            }
            _ => return None,
        })
    }
}

impl EventContext for MobFacts<'_> {
    fn filter(&self, binding: crate::entity::events::FilterBinding) -> Result<bool, String> {
        let kind = self
            .world
            .get::<MobKind>(self.entity)
            .ok_or_else(|| "filter evaluated on an actor without a mob archetype".to_owned())?;
        let filter = kind
            .0
            .filters
            .get(binding.0 as usize)
            .ok_or_else(|| format!("unknown filter binding {}", binding.0))?;
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
    let Some(families) = world.get::<crate::entity::families::ActorFamilies>(entity) else {
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
) -> Result<(), String> {
    let archetype = world
        .get::<MobKind>(entity)
        .map(|kind| std::sync::Arc::clone(&kind.0))
        .ok_or_else(|| "actor has no mob archetype".to_owned())?;
    let seed = world.resource_mut::<EntityRandom>().0.r#gen::<u64>();
    let mut rng = StdRng::seed_from_u64(seed);
    fire_inner(world, entity, &archetype, event, other, &mut rng, 0)
}

fn fire_inner(
    world: &mut World,
    entity: Entity,
    archetype: &MobArchetype,
    event: &EventHandle,
    other: Option<Entity>,
    rng: &mut StdRng,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_TRIGGER_DEPTH {
        return Err(format!(
            "{}: event triggers nested deeper than {MAX_TRIGGER_DEPTH}",
            archetype.identifier
        ));
    }
    let mut outcome = EventOutcome::default();
    let mut scratch = Vec::new();
    {
        let facts = MobFacts {
            world,
            entity,
            other,
        };
        archetype
            .events
            .evaluate(
                event,
                &facts,
                &mut || rng.r#gen(),
                &mut outcome,
                &mut scratch,
            )
            .map_err(|e| e.to_string())?;
    }
    for effect in &outcome.effects {
        match *effect {
            EventEffect::AddGroup(group) => {
                queue_group_change(world, entity, GroupChange::Add(group.clone()))?
            }
            EventEffect::RemoveGroup(group) => {
                queue_group_change(world, entity, GroupChange::Remove(group.clone()))?
            }
            EventEffect::SetProperty(assignment) => {
                let mut properties = world
                    .get_mut::<ActorProperties>(entity)
                    .ok_or_else(|| "actor has no property state".to_owned())?;
                properties
                    .apply(assignment, &mut PropertyMolang { rng })
                    .map_err(|e| e.to_string())?;
            }
            EventEffect::Action { kind, .. } => {
                return Err(format!(
                    "event action {} has no runtime binding",
                    kind.key()
                ));
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
            if trigger.passes(&facts).map_err(|e| e.to_string())? {
                match &trigger.event {
                    TriggerEvent::Internal(handle) => due.push(handle.clone()),
                    TriggerEvent::External(_) => {
                        return Err("external event triggers are not bound".into());
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
