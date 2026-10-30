//! Encoding a mob as Bedrock actor NBT (the tag table is in the `persistence` module docs).

use std::collections::HashMap;

use bevy_ecs::prelude::*;
use glam::DVec3;
use zuri_nbt::{NBTTag, encoding::LittleEndian, tag};

use crate::ecs::resources::TickCounter;
use crate::entity::components::{Health, OnGround, Position, Rotation, Velocity};
use crate::entity::definition::DefinitionState;
use crate::entity::definition::properties::{ActorProperties, NamedPropertyValue};
use crate::entity::mob::MobKind;
use crate::entity::mob::components::spawn_entity::SpawnEntityTimers;
use crate::entity::mob::life::ageing::BabyAge;
use crate::entity::mob::state::{BreedCooldown, InLove, Persistent};

use super::HEALTH_ATTRIBUTE;

pub(super) fn list<T: Into<NBTTag>>(values: impl IntoIterator<Item = T>) -> NBTTag {
    NBTTag::List(tag::List(values.into_iter().map(Into::into).collect()))
}

/// Encode one mob as Bedrock actor NBT. `mob` must be a mob entity.
pub(super) fn encode_mob(world: &World, mob: Entity, unique_id: i64) -> Vec<u8> {
    let archetype = &world.get::<MobKind>(mob).expect("saved actors are mobs").0;
    let state = world
        .get::<DefinitionState>(mob)
        .expect("every mob has a definition");
    let position = world
        .get::<Position>(mob)
        .expect("every mob has a position")
        .0;
    let rotation = world
        .get::<Rotation>(mob)
        .expect("every mob has a rotation");
    let velocity = world.get::<Velocity>(mob).map_or(DVec3::ZERO, |v| v.0);

    let mut fields = HashMap::new();
    let mut put = |name: &str, value: NBTTag| {
        fields.insert(name.to_owned(), value);
    };
    put(
        "identifier",
        NBTTag::String(archetype.identifier.as_ref().into()),
    );
    put("UniqueID", NBTTag::Long(unique_id.into()));
    put(
        "Pos",
        list(position.to_array().map(|v| tag::Float(v as f32))),
    );
    put(
        "Rotation",
        list([tag::Float(rotation.yaw), tag::Float(rotation.pitch)]),
    );
    put(
        "Motion",
        list(velocity.to_array().map(|v| tag::Float(v as f32))),
    );
    put(
        "OnGround",
        NBTTag::Byte(u8::from(world.get::<OnGround>(mob).is_some_and(|g| g.0)).into()),
    );
    put(
        "definitions",
        list(
            std::iter::once(format!("+{}", archetype.identifier))
                .chain(
                    state
                        .group_history()
                        .map(|(group, added)| format!("{}{group}", if added { '+' } else { '-' })),
                )
                .map(tag::String),
        ),
    );
    if let Some(health) = world.get::<Health>(mob) {
        let attribute = tag::Compound::builder()
            .with_string("Name", HEALTH_ATTRIBUTE)
            .with_float("Base", health.max)
            .with_float("Current", health.current)
            .with_float("Max", health.max)
            .with_float("Min", 0.0)
            .with_float("DefaultMax", health.max)
            .with_float("DefaultMin", 0.0)
            .build();
        put("Attributes", list([attribute]));
    }
    if let Some(properties) = world.get::<ActorProperties>(mob) {
        let mut saved = HashMap::new();
        for (name, value) in properties.named_values() {
            let value = match value {
                NamedPropertyValue::Bool(v) => NBTTag::Byte(u8::from(v).into()),
                NamedPropertyValue::Int(v) => NBTTag::Int(v.into()),
                NamedPropertyValue::Float(v) => NBTTag::Float(v.into()),
                NamedPropertyValue::Enum(v) => NBTTag::String(v.into()),
            };
            saved.insert(name.to_owned(), value);
        }
        put("properties", NBTTag::Compound(saved.into()));
    }
    put(
        "Persistent",
        NBTTag::Byte(u8::from(world.get::<Persistent>(mob).is_some()).into()),
    );
    if let Some(colour) = crate::entity::mob::components::color::color_of(world, mob) {
        put("Color", NBTTag::Byte(colour.into()));
    }
    let now = current_tick(world);
    if let Some(love) = world.get::<InLove>(mob)
        && love.until_tick > now
    {
        put(
            "InLove",
            NBTTag::Int(remaining_ticks(love.until_tick, now).into()),
        );
    }
    if let Some(cooldown) = world.get::<BreedCooldown>(mob)
        && cooldown.until_tick > now
    {
        put(
            "BreedCooldown",
            NBTTag::Int(remaining_ticks(cooldown.until_tick, now).into()),
        );
    }
    // `Never` is recreated by the ageable composition and not stored.
    let growth = match world.get::<BabyAge>(mob) {
        Some(BabyAge::Growing { grow_tick }) => Some((remaining_ticks(*grow_tick, now), false)),
        Some(BabyAge::Paused { remaining }) => Some((remaining_ticks(*remaining, 0), true)),
        Some(BabyAge::Never) | None => None,
    };
    if let Some((remaining, paused)) = growth {
        put("Age", NBTTag::Int((-remaining).into()));
        put("GrowthPaused", NBTTag::Byte(u8::from(paused).into()));
    }
    if let Some(timers) = world.get::<SpawnEntityTimers>(mob) {
        put("entries", timers.to_nbt(now));
    }
    for (name, value) in crate::entity::mob::gear::persist::save(world, mob) {
        put(name, value);
    }

    let mut out = Vec::new();
    NBTTag::Compound(fields.into())
        .write(&mut out, LittleEndian)
        .expect("actor NBT lists are homogeneous and write into a Vec");
    out
}

pub(super) fn current_tick(world: &World) -> u64 {
    world
        .get_resource::<TickCounter>()
        .map_or(0, |tick| tick.current)
}

/// Ticks from `now` until the deadline `until_tick`, as BDS stores timers.
fn remaining_ticks(until_tick: u64, now: u64) -> i32 {
    i32::try_from(until_tick.saturating_sub(now)).unwrap_or(i32::MAX)
}
