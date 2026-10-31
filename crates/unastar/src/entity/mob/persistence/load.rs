//! Restoring a mob from Bedrock actor NBT.

use std::collections::HashMap;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use zuri_nbt::err::{NBTError, ReadError};
use zuri_nbt::{NBTTag, encoding::LittleEndian};

use crate::entity::components::{Health, OnGround, Rotation, Velocity};
use crate::entity::definition::properties::{ActorProperties, NamedPropertyValue, PropertyError};
use crate::entity::definition::{GroupChange, apply_pending_definitions, queue_group_change};
use crate::entity::mob::components::spawn_entity::SpawnEntityTimers;
use crate::entity::mob::life::ageing::BabyAge;
use crate::entity::mob::state::{BreedCooldown, InLove, Persistent};
use crate::entity::mob::{MobKind, MobSpawnError, SpawnCause, derive_entity, spawn_mob};

use super::save::current_tick;
use super::{HEALTH_ATTRIBUTE, LoadedActorIds, MobUniqueId};

/// Why stored actor NBT could not be turned back into a mob.
#[derive(Debug, thiserror::Error)]
pub enum ActorDecodeError {
    #[error("actor NBT: {0}")]
    Nbt(#[from] NBTError<ReadError>),
    #[error("trailing bytes after the actor compound")]
    TrailingBytes,
    #[error("actor root is not a compound")]
    NotACompound,
    #[error("actor needs {kind} {name}")]
    MissingField {
        name: &'static str,
        kind: &'static str,
    },
    #[error("actor {name} is not {kind}")]
    WrongType {
        name: &'static str,
        kind: &'static str,
    },
    #[error("actor {0} is outside its valid runtime bounds")]
    InvalidField(&'static str),
    #[error("actor {name} is not a list of {len} Floats")]
    WrongFloatList { name: &'static str, len: usize },
    #[error("definition entry {0:?} has no +/- prefix")]
    UnprefixedDefinition(String),
    #[error("{identifier} has no component group {group}")]
    UnknownGroup { identifier: Arc<str>, group: String },
    #[error("property {name} has an unsupported tag type")]
    PropertyTag { name: String },
    #[error(transparent)]
    Property(#[from] PropertyError),
    #[error(transparent)]
    Gear(#[from] crate::entity::mob::gear::persist::GearDecodeError),
    #[error("cannot spawn the actor: {0}")]
    Spawn(#[source] MobSpawnError),
}

/// Read access to a decoded actor compound.
struct Fields<'a>(&'a HashMap<String, NBTTag>);

impl Fields<'_> {
    fn string(&self, name: &'static str) -> Result<&str, ActorDecodeError> {
        match self.0.get(name) {
            Some(NBTTag::String(value)) => Ok(&value.0),
            None => Err(ActorDecodeError::MissingField {
                name,
                kind: "String",
            }),
            Some(_) => Err(ActorDecodeError::WrongType {
                name,
                kind: "a String",
            }),
        }
    }

    fn long(&self, name: &'static str) -> Result<i64, ActorDecodeError> {
        match self.0.get(name) {
            Some(NBTTag::Long(value)) => Ok(value.0),
            None => Err(ActorDecodeError::MissingField { name, kind: "Long" }),
            Some(_) => Err(ActorDecodeError::WrongType {
                name,
                kind: "a Long",
            }),
        }
    }

    fn int(&self, name: &'static str) -> Result<Option<i32>, ActorDecodeError> {
        match self.0.get(name) {
            None => Ok(None),
            Some(NBTTag::Int(value)) => Ok(Some(value.0)),
            Some(_) => Err(ActorDecodeError::WrongType {
                name,
                kind: "an Int",
            }),
        }
    }

    fn flag(&self, name: &'static str) -> Result<bool, ActorDecodeError> {
        match self.0.get(name) {
            None => Ok(false),
            Some(NBTTag::Byte(value)) => Ok(value.0 != 0),
            Some(_) => Err(ActorDecodeError::WrongType {
                name,
                kind: "a Byte",
            }),
        }
    }

    fn floats<const N: usize>(
        &self,
        name: &'static str,
    ) -> Result<Option<[f32; N]>, ActorDecodeError> {
        let Some(tag) = self.0.get(name) else {
            return Ok(None);
        };
        let invalid = || ActorDecodeError::WrongFloatList { name, len: N };
        let NBTTag::List(values) = tag else {
            return Err(invalid());
        };
        if values.0.len() != N {
            return Err(invalid());
        }
        let mut out = [0.0; N];
        for (slot, value) in out.iter_mut().zip(&values.0) {
            let NBTTag::Float(value) = value else {
                return Err(invalid());
            };
            *slot = value.0;
        }
        Ok(Some(out))
    }
}

/// Why a stored actor was not restored.
#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    /// A mob with this unique ID is already in the world.
    #[error("actor {0} is already loaded")]
    AlreadyLoaded(i64),
    /// The record is malformed or names an actor this server does not run.
    #[error(transparent)]
    Decode(#[from] ActorDecodeError),
}

/// Restore one mob from Bedrock actor NBT.
pub fn restore_mob(world: &mut World, nbt: &[u8]) -> Result<Entity, RestoreError> {
    let mut input = nbt;
    let root = NBTTag::read(&mut input, LittleEndian).map_err(ActorDecodeError::from)?;
    if !input.is_empty() {
        return Err(ActorDecodeError::TrailingBytes.into());
    }
    let NBTTag::Compound(root) = root else {
        return Err(ActorDecodeError::NotACompound.into());
    };
    let fields = Fields(&root.0);
    let identifier = fields.string("identifier")?;
    let unique_id = fields.long("UniqueID")?;
    world.init_resource::<LoadedActorIds>();
    if world.resource::<LoadedActorIds>().0.contains(&unique_id) {
        return Err(RestoreError::AlreadyLoaded(unique_id));
    }
    let [x, y, z] = fields
        .floats::<3>("Pos")?
        .ok_or(ActorDecodeError::MissingField {
            name: "Pos",
            kind: "list of 3 Floats",
        })?;
    let [yaw, pitch] = fields.floats::<2>("Rotation")?.unwrap_or_default();
    let motion = fields.floats::<3>("Motion")?.unwrap_or_default();
    let position = DVec3::new(f64::from(x), f64::from(y), f64::from(z));

    let mob = spawn_mob(world, identifier, position, yaw, SpawnCause::Loaded)
        .map_err(ActorDecodeError::Spawn)?;
    match restore_state(world, mob, &fields, pitch, motion) {
        Ok(()) => {
            world
                .entity_mut(mob)
                .remove::<super::Restoring>()
                .insert(MobUniqueId(unique_id));
            Ok(mob)
        }
        Err(error) => {
            world.despawn(mob);
            Err(error.into())
        }
    }
}

/// Re-apply the saved composition and runtime state to a freshly spawned mob.
fn restore_state(
    world: &mut World,
    mob: Entity,
    fields: &Fields<'_>,
    pitch: f32,
    motion: [f32; 3],
) -> Result<(), ActorDecodeError> {
    let archetype = Arc::clone(&world.get::<MobKind>(mob).expect("spawn_mob makes mobs").0);
    match fields.0.get("definitions") {
        None => {}
        Some(NBTTag::List(entries)) => {
            for entry in &entries.0 {
                let NBTTag::String(entry) = entry else {
                    return Err(ActorDecodeError::WrongType {
                        name: "definitions",
                        kind: "a list of Strings",
                    });
                };
                let (added, name) = match entry.0.split_at_checked(1) {
                    Some(("+", name)) => (true, name),
                    Some(("-", name)) => (false, name),
                    _ => return Err(ActorDecodeError::UnprefixedDefinition(entry.0.clone())),
                };
                if name == &*archetype.identifier {
                    continue;
                }
                let group = archetype.definition.group(name).ok_or_else(|| {
                    ActorDecodeError::UnknownGroup {
                        identifier: Arc::clone(&archetype.identifier),
                        group: name.to_owned(),
                    }
                })?;
                let change = if added {
                    GroupChange::Add(group)
                } else {
                    GroupChange::Remove(group)
                };
                queue_group_change(world, mob, change)
                    .expect("the group comes from the mob's own definition");
            }
        }
        Some(_) => {
            return Err(ActorDecodeError::WrongType {
                name: "definitions",
                kind: "a list",
            });
        }
    }
    apply_pending_definitions(world);

    let health = match fields.0.get("Attributes") {
        None => None,
        Some(NBTTag::List(attributes)) => attributes.0.iter().find_map(|attribute| {
            let NBTTag::Compound(attribute) = attribute else {
                return None;
            };
            let attribute = Fields(&attribute.0);
            if attribute.string("Name").ok()? != HEALTH_ATTRIBUTE {
                return None;
            }
            let float = |name| match attribute.0.get(name) {
                Some(NBTTag::Float(value)) => Some(value.0),
                _ => None,
            };
            Some((float("Current"), float("Max")))
        }),
        Some(_) => {
            return Err(ActorDecodeError::WrongType {
                name: "Attributes",
                kind: "a list",
            });
        }
    };
    let properties = match fields.0.get("properties") {
        None => None,
        Some(NBTTag::Compound(properties)) => Some(&properties.0),
        Some(_) => {
            return Err(ActorDecodeError::WrongType {
                name: "properties",
                kind: "a compound",
            });
        }
    };
    let persistent = fields.flag("Persistent")?;
    let in_love = fields.int("InLove")?;
    let cooldown = fields.int("BreedCooldown")?;
    let age = fields.int("Age")?;
    let growth_paused = fields.flag("GrowthPaused")?;
    let on_ground = fields.flag("OnGround")?;
    let color = match fields.0.get("Color") {
        None => None,
        Some(NBTTag::Byte(value)) => Some((i32::from(value.0) & 0xf) as u8),
        Some(_) => {
            return Err(ActorDecodeError::WrongType {
                name: "Color",
                kind: "a Byte",
            });
        }
    };

    let now = current_tick(world);
    let anger_calmed = fields.flag("UnastarAngerCalmed")?;
    let anger_deadline = match fields.0.get("UnastarAngerTicks") {
        None => None,
        Some(NBTTag::Long(ticks))
            if ticks.0 >= 0
                && world
                    .get::<crate::entity::mob::components::reactive::Angry>(mob)
                    .is_some() =>
        {
            Some(
                now.checked_add(ticks.0 as u64)
                    .ok_or(ActorDecodeError::InvalidField("UnastarAngerTicks"))?,
            )
        }
        Some(NBTTag::Long(_)) => return Err(ActorDecodeError::InvalidField("UnastarAngerTicks")),
        Some(_) => {
            return Err(ActorDecodeError::WrongType {
                name: "UnastarAngerTicks",
                kind: "a Long",
            });
        }
    };
    if anger_calmed && anger_deadline != Some(now) {
        return Err(ActorDecodeError::InvalidField("UnastarAngerCalmed"));
    }
    let mut entity = world.entity_mut(mob);
    if let Some(deadline) = anger_deadline {
        let mut anger = entity
            .get_mut::<crate::entity::mob::components::reactive::Angry>()
            .expect("validated anger");
        anger.until_tick = deadline;
        anger.calmed = anger_calmed;
        crate::entity::mob::components::reactive::angry_flag(&mut entity, !anger_calmed);
    }
    if let Some((current, max)) = health
        && let Some(mut component) = entity.get_mut::<Health>()
    {
        if let Some(max) = max {
            component.max = max;
        }
        if let Some(current) = current {
            component.current = current;
        }
    }
    if let Some(properties) = properties {
        let mut component = entity
            .get_mut::<ActorProperties>()
            .expect("every mob has properties");
        for (name, value) in properties {
            let value = match value {
                NBTTag::Byte(v) => NamedPropertyValue::Bool(v.0 != 0),
                NBTTag::Int(v) => NamedPropertyValue::Int(v.0),
                NBTTag::Float(v) => NamedPropertyValue::Float(v.0),
                NBTTag::String(v) => NamedPropertyValue::Enum(&v.0),
                _ => return Err(ActorDecodeError::PropertyTag { name: name.clone() }),
            };
            component.set_named(name, value)?;
        }
    }
    if let Some(mut rotation) = entity.get_mut::<Rotation>() {
        rotation.pitch = pitch;
    }
    if let Some(mut velocity) = entity.get_mut::<Velocity>() {
        velocity.0 = DVec3::from_array(motion.map(f64::from));
    }
    if let Some(mut ground) = entity.get_mut::<OnGround>() {
        ground.0 = on_ground;
    }
    // The composition re-applied its colour group; a dyed colour overrides it.
    if let Some(colour) = color {
        crate::entity::mob::components::color::set_color(
            &mut entity,
            crate::entity::mob::components::color::ActorColor(colour),
        );
    }
    if persistent {
        entity.insert(Persistent);
    }
    if let Some(ticks) = in_love.filter(|&ticks| ticks > 0) {
        entity.insert(InLove {
            until_tick: now + ticks as u64,
            player: None,
        });
    }
    if let Some(ticks) = cooldown.filter(|&ticks| ticks > 0) {
        entity.insert(BreedCooldown {
            until_tick: now + ticks as u64,
        });
    }
    // The ageable composition inserted a fresh growth state; overwrite it
    // with the saved one.
    if let Some(age) = age.filter(|&age| age <= 0)
        && entity.contains::<BabyAge>()
    {
        let remaining = u64::from(age.unsigned_abs());
        entity.insert(if growth_paused {
            BabyAge::Paused { remaining }
        } else {
            BabyAge::Growing {
                grow_tick: now + remaining,
            }
        });
    }
    if let Some(mut timers) = world.get_mut::<SpawnEntityTimers>(mob) {
        let legacy = match fields.0.get("SpawnTimer") {
            Some(NBTTag::Int(value)) => Some(value.0),
            _ => None,
        };
        timers.restore(fields.0.get("entries"), legacy, now);
    }
    crate::entity::mob::gear::persist::restore(world, mob, fields.0)?;
    derive_entity(world, mob);
    Ok(())
}
