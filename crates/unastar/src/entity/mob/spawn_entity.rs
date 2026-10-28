//! `minecraft:spawn_entity`: periodically drop an item (the chicken's egg).
//!
//! Native rule: every rule (the root one and each `entities` entry) has its
//! own timer, running from the first tick the component exists whatever its
//! filters say. An idle timer rolls `min + random(max - min)` ticks (upper
//! bound exclusive; `min_wait_time`/`max_wait_time` are seconds × 20) and
//! the rolling tick counts as the first, so a rule fires exactly that many
//! ticks after its previous fire. When it fires its filters are evaluated:
//! passing, the `spawn_sound` plays at the mob's mid-body and `num_to_spawn`
//! items drop at its feet with the ordinary item toss, then
//! `spawn_item_event` fires on the mob; failing, nothing happens. A
//! `single_use` rule stops after its first fire, even a filtered-out one.
//! There is no age gate: vanilla puts the component in the adult group.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use rand::Rng;
use thiserror::Error;
use tracing::warn;
use unastar_data::actor_components::body::{self, ActorDefinitionTrigger, FilterSubject};
use unastar_rules::filter::Filter;
use zuri_nbt::{NBTTag, tag};

use super::config::ItemMatcher;
use super::events::{MobFacts, fire_event};
use super::sound::{body_centre, play_level_sound};
use super::state::Dead;
use super::{MobKind, constants};
use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::Position;
use crate::entity::definition::ComponentKindsBuilder;
use crate::item::ItemStack;
use crate::registry::ItemRegistry;

/// Why a `minecraft:spawn_entity` body cannot be compiled.
#[derive(Debug, Error)]
pub enum SpawnEntityError {
    #[error("spawning actors (spawn_entity {0}) is not implemented; only items are")]
    ActorSpawn(String),
    #[error("leashing spawned actors is not implemented")]
    Leash,
    #[error("unknown spawn item {0}")]
    UnknownItem(String),
    #[error("spawn_item_event {0} targets another actor; only self is implemented")]
    OtherTarget(String),
    #[error("filtered spawn_item_event triggers are not implemented")]
    FilteredEvent,
    #[error("item: {0}")]
    Item(String),
    #[error("filter: {0}")]
    Filter(String),
}

/// One compiled spawn rule.
#[derive(Debug, Clone)]
pub struct SpawnRule {
    pub min_ticks: u32,
    pub max_ticks: u32,
    pub filter: Option<Filter>,
    /// Item identifier dropped, one per spawn.
    pub item: Arc<str>,
    pub sound: Arc<str>,
    pub count: u32,
    pub single_use: bool,
    /// Event fired on the mob after the items dropped.
    pub item_event: Option<Arc<str>>,
}

/// `minecraft:spawn_entity`.
#[derive(Component, Debug, Clone)]
pub struct SpawnEntityConfig {
    pub rules: Arc<[SpawnRule]>,
}

/// One rule's timer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnTimer {
    /// Rolls a new wait on the next tick.
    Idle,
    /// Fires on this server tick.
    Due(u64),
    /// A single-use rule that already fired.
    Finished,
}

/// The timers of [`SpawnEntityConfig`]'s rules, in rule order.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct SpawnEntityTimers(pub Box<[SpawnTimer]>);

impl SpawnEntityTimers {
    /// The actor NBT `entries` list as BDS saves it at the end of a tick;
    /// `now` is the next tick to run (saves happen between ticks). Per rule
    /// `SpawnTimer` (Int, ticks until it fires, counting the tick it fires
    /// on; 0 rolls a new wait on the next tick) and `StopSpawning` (Byte, a
    /// fired single-use rule).
    pub fn to_nbt(&self, now: u64) -> NBTTag {
        let entries = self
            .0
            .iter()
            .map(|timer| {
                let (remaining, stopped) = match *timer {
                    SpawnTimer::Idle => (0, false),
                    SpawnTimer::Due(tick) => (
                        i32::try_from((tick + 1).saturating_sub(now)).unwrap_or(i32::MAX),
                        false,
                    ),
                    SpawnTimer::Finished => (0, true),
                };
                NBTTag::Compound(
                    tag::Compound::builder()
                        .with_int("SpawnTimer", remaining)
                        .with_byte("StopSpawning", u8::from(stopped))
                        .build(),
                )
            })
            .collect();
        NBTTag::List(tag::List(entries))
    }

    /// Restore timers saved by [`Self::to_nbt`]; `now` is the next tick to
    /// run. Like BDS the load is lenient: element `i` fills rule `i`,
    /// non-compound elements and extra elements are skipped, missing or
    /// mistyped keys count as 0/false; without an `entries` list a top-level
    /// `SpawnTimer` (older saves) fills rule 0. A remaining count `N` fires
    /// `N - 1` ticks after `now`, the restore tick being the first counted.
    pub fn restore(&mut self, entries: Option<&NBTTag>, legacy_timer: Option<i32>, now: u64) {
        let timer = |remaining: i32, stopped: bool| {
            if stopped {
                SpawnTimer::Finished
            } else if remaining > 0 {
                SpawnTimer::Due(now + remaining as u64 - 1)
            } else {
                SpawnTimer::Idle
            }
        };
        match entries {
            Some(NBTTag::List(list)) => {
                for (slot, entry) in self.0.iter_mut().zip(&list.0) {
                    let NBTTag::Compound(entry) = entry else {
                        continue;
                    };
                    let remaining = match entry.0.get("SpawnTimer") {
                        Some(NBTTag::Int(value)) => value.0,
                        _ => 0,
                    };
                    let stopped =
                        matches!(entry.0.get("StopSpawning"), Some(NBTTag::Byte(v)) if v.0 != 0);
                    *slot = timer(remaining, stopped);
                }
            }
            _ => {
                if let (Some(remaining), Some(slot)) = (legacy_timer, self.0.first_mut()) {
                    *slot = timer(remaining, false);
                }
            }
        }
    }
}

pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
    items: &Arc<ItemRegistry>,
) -> Result<(), String> {
    let items = Arc::clone(items);
    builder.register(
        move |b: body::SpawnEntity| compile(b, &items).map_err(|e| e.to_string()),
        |config: &SpawnEntityConfig, entity| {
            let timers = SpawnEntityTimers(vec![SpawnTimer::Idle; config.rules.len()].into());
            entity.insert((config.clone(), timers));
        },
        |entity| {
            entity.remove::<(SpawnEntityConfig, SpawnEntityTimers)>();
        },
    )
}

/// The fields a root rule and an `entities` entry share.
struct RuleFields {
    filters: Option<unastar_data::actor_components::FilterJson>,
    max_wait_time: i32,
    min_wait_time: i32,
    num_to_spawn: i32,
    should_leash: bool,
    single_use: bool,
    spawn_entity: Option<String>,
    spawn_item: String,
    spawn_item_event: Option<ActorDefinitionTrigger>,
    spawn_sound: String,
}

fn compile(
    b: body::SpawnEntity,
    items: &ItemRegistry,
) -> Result<SpawnEntityConfig, SpawnEntityError> {
    let rules = match b.entities {
        Some(entries) => entries
            .into_iter()
            .map(|e| {
                compile_rule(
                    items,
                    RuleFields {
                        filters: e.filters,
                        max_wait_time: e.max_wait_time,
                        min_wait_time: e.min_wait_time,
                        num_to_spawn: e.num_to_spawn,
                        should_leash: e.should_leash,
                        single_use: e.single_use,
                        spawn_entity: e.spawn_entity,
                        spawn_item: e.spawn_item,
                        spawn_item_event: e.spawn_item_event,
                        spawn_sound: e.spawn_sound,
                    },
                )
            })
            .collect::<Result<Vec<_>, _>>()?,
        None => vec![compile_rule(
            items,
            RuleFields {
                filters: b.filters,
                max_wait_time: b.max_wait_time,
                min_wait_time: b.min_wait_time,
                num_to_spawn: b.num_to_spawn,
                should_leash: b.should_leash,
                single_use: b.single_use,
                spawn_entity: b.spawn_entity,
                spawn_item: b.spawn_item,
                spawn_item_event: b.spawn_item_event,
                spawn_sound: b.spawn_sound,
            },
        )?],
    };
    Ok(SpawnEntityConfig {
        rules: rules.into(),
    })
}

fn compile_rule(items: &ItemRegistry, f: RuleFields) -> Result<SpawnRule, SpawnEntityError> {
    if let Some(actor) = f.spawn_entity.filter(|actor| !actor.is_empty()) {
        return Err(SpawnEntityError::ActorSpawn(actor));
    }
    if f.should_leash {
        return Err(SpawnEntityError::Leash);
    }
    let item = ItemMatcher::parse(&f.spawn_item).map_err(SpawnEntityError::Item)?;
    if items.get_by_name(&item.identifier).is_none() {
        return Err(SpawnEntityError::UnknownItem(f.spawn_item));
    }
    let item_event = match f.spawn_item_event {
        Some(trigger) if !trigger.event.is_empty() => {
            if trigger.target != FilterSubject::SelfValue {
                return Err(SpawnEntityError::OtherTarget(trigger.event));
            }
            if trigger.filters.is_some() {
                return Err(SpawnEntityError::FilteredEvent);
            }
            Some(Arc::from(trigger.event.as_str()))
        }
        _ => None,
    };
    let ticks = |seconds: i32| (seconds.max(0) as u32) * constants::TICKS_PER_SECOND as u32;
    Ok(SpawnRule {
        min_ticks: ticks(f.min_wait_time),
        max_ticks: ticks(f.max_wait_time),
        filter: f
            .filters
            .as_ref()
            .map(super::config::filter)
            .transpose()
            .map_err(SpawnEntityError::Filter)?,
        item: item.identifier,
        sound: Arc::from(f.spawn_sound.as_str()),
        count: f.num_to_spawn.max(0) as u32,
        single_use: f.single_use,
        item_event,
    })
}

/// System (exclusive, `EntityLogicSet`): advance spawn timers and fire the
/// rules that are due.
#[allow(clippy::type_complexity)]
pub fn tick_spawn_entity(
    world: &mut World,
    mobs: &mut QueryState<
        (Entity, &SpawnEntityConfig, &mut SpawnEntityTimers),
        (With<MobKind>, Without<Dead>),
    >,
    mut due: Local<Vec<(Entity, SpawnRule)>>,
) {
    let now = world.resource::<TickCounter>().current;
    let mut rng = world.remove_resource::<EntityRandom>().unwrap_or_default();
    for (mob, config, mut timers) in mobs.iter_mut(world) {
        for (rule, timer) in config.rules.iter().zip(timers.0.iter_mut()) {
            if *timer == SpawnTimer::Idle {
                let wait = if rule.max_ticks > rule.min_ticks {
                    rule.min_ticks + rng.0.gen_range(0..rule.max_ticks - rule.min_ticks)
                } else {
                    rule.min_ticks
                };
                // The rolling tick counts as the first tick of the wait.
                *timer = SpawnTimer::Due(now + u64::from(wait.max(1)) - 1);
            }
            if let SpawnTimer::Due(tick) = *timer
                && now >= tick
            {
                *timer = if rule.single_use {
                    SpawnTimer::Finished
                } else {
                    SpawnTimer::Idle
                };
                due.push((mob, rule.clone()));
            }
        }
    }
    world.insert_resource(rng);
    for (mob, rule) in due.drain(..) {
        fire(world, mob, &rule);
    }
}

fn fire(world: &mut World, mob: Entity, rule: &SpawnRule) {
    if world.get::<Dead>(mob).is_some() {
        return;
    }
    let passes = rule.filter.as_ref().is_none_or(|filter| {
        filter.evaluate(&MobFacts {
            world,
            entity: mob,
            other: None,
        })
    });
    let (true, Some(feet), Some(centre)) = (
        passes,
        world.get::<Position>(mob).map(|p| p.0),
        body_centre(world, mob),
    ) else {
        return;
    };
    play_level_sound(world, Some(mob), centre, &rule.sound, -1);
    for _ in 0..rule.count {
        crate::entity::spawn_item_drop(world, feet, ItemStack::new(&*rule.item, 1), None);
    }
    let Some(name) = &rule.item_event else {
        return;
    };
    let Some(handle) = world
        .get::<MobKind>(mob)
        .and_then(|kind| kind.0.events.event(name))
    else {
        warn!(event = %name, "spawn_item_event names an event the mob does not define");
        return;
    };
    if let Err(error) = fire_event(world, mob, &handle, None) {
        warn!(event = %name, %error, "spawn_item_event failed");
    }
}
