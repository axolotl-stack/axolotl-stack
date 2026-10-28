//! Runtime state for the behaviour-pack components a vanilla mob uses.
//!
//! Every component the cow declares is registered here as a typed
//! [`ComponentKindsBuilder`] kind: the authored JSON is decoded into the
//! generated body (`unastar_data::actor_components::body`, unknown fields
//! rejected) and compiled once at startup into a plain ECS component. Item
//! names, filters and Molang are parsed here too, so no JSON or name lookup
//! happens on a tick. Systems in `entity::mob::{ai, combat, interact}`
//! consume these components.
//!
//! A few components are accepted without server-side behaviour; see
//! [`ACCEPTED_WITHOUT_BEHAVIOUR`] for the list and the reason for each.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use bevy_ecs::world::EntityWorldMut;
use unastar_data::actor_components::body::{
    self, ActorDamageCause, BreedItem, BreedsWith, EventTrigger, FeedItem, FilterSubject,
    InheritedProperties, ItemDescriptor, MolangString, MolangValue,
};
use unastar_data::actor_components::{ActorComponent, ComponentBody, FilterJson};
use unastar_rules::filter::{Filter, FilterRegistry};
use unastar_rules::molang::{Expr, QueryContext, QueryRegistry, parse};

use super::ageing::BabyAge;
use super::constants;
use crate::entity::components::AirSupply;
use crate::entity::definition::ComponentKindsBuilder;

/// An item a component matches: `wheat`, `minecraft:wheat` or `bucket:0`.
/// A trailing numeric segment is the legacy aux value; omitted means any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemMatcher {
    pub identifier: Arc<str>,
    pub aux: Option<i16>,
}

impl ItemMatcher {
    pub fn parse(text: &str) -> Result<Self, String> {
        let (name, aux) = match text.rsplit_once(':') {
            Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => {
                let aux = tail
                    .parse::<i16>()
                    .map_err(|_| format!("item aux value out of range in {text:?}"))?;
                (head, Some(aux))
            }
            _ => (text, None),
        };
        if name.is_empty() {
            return Err(format!("empty item name in {text:?}"));
        }
        let identifier = if name.contains(':') {
            Arc::from(name)
        } else {
            Arc::from(format!("minecraft:{name}"))
        };
        Ok(Self { identifier, aux })
    }

    pub fn matches(&self, item_id: &str, aux: i16) -> bool {
        self.identifier.as_ref() == item_id && self.aux.is_none_or(|want| want == aux)
    }
}

fn items<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<Arc<[ItemMatcher]>, String> {
    names
        .into_iter()
        .map(ItemMatcher::parse)
        .collect::<Result<Vec<_>, _>>()
        .map(Into::into)
}

/// An event this mob fires on itself, resolved by name when it fires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelfEvent(pub Arc<str>);

fn self_event(trigger: &EventTrigger) -> Result<SelfEvent, String> {
    match trigger {
        EventTrigger::Event(name) => Ok(SelfEvent(Arc::from(name.as_str()))),
        EventTrigger::Trigger(trigger) => {
            if trigger.target != FilterSubject::SelfValue {
                return Err(format!(
                    "event {} targets {:?}; only self is supported",
                    trigger.event, trigger.target
                ));
            }
            if trigger.filters.is_some() {
                return Err(format!(
                    "filtered trigger {} is not supported",
                    trigger.event
                ));
            }
            Ok(SelfEvent(Arc::from(trigger.event.as_str())))
        }
    }
}

/// The priority and speed every movement goal shares.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GoalBase {
    pub priority: i32,
    pub speed_multiplier: f32,
}

/// `minecraft:behavior.float`: request a jump in liquid.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct FloatGoal {
    pub priority: i32,
    pub chance_per_tick: f32,
}

/// `minecraft:behavior.panic`: run from recent damage.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct PanicGoal {
    pub base: GoalBase,
    pub force: bool,
    pub ignore_mob_damage: bool,
    pub prefer_water: bool,
    /// Causes that trigger the goal; `None` means any cause.
    pub damage_sources: Option<Arc<[ActorDamageCause]>>,
}

/// `minecraft:behavior.breed`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BreedGoal {
    pub base: GoalBase,
}

/// `minecraft:behavior.tempt`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct TemptGoal {
    pub base: GoalBase,
    pub items: Arc<[ItemMatcher]>,
    /// Search radius in blocks (a non-positive authored value means 16).
    pub within_radius: f32,
    pub can_get_scared: bool,
    pub can_tempt_vertically: bool,
    pub can_tempt_while_ridden: bool,
    /// Extra stop distance beyond touching (a negative value means 1.5).
    pub stop_distance: f32,
}

/// `minecraft:behavior.follow_parent`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct FollowParentGoal {
    pub base: GoalBase,
}

/// `minecraft:behavior.random_stroll`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct RandomStrollGoal {
    pub base: GoalBase,
    pub xz_dist: i32,
    pub y_dist: i32,
    /// 1-in-`interval` chance per tick to start; 0 starts every tick.
    pub interval: i32,
}

/// `minecraft:behavior.look_at_player`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct LookAtPlayerGoal {
    pub priority: i32,
    pub look_distance: f32,
    pub probability: f32,
    /// Half of the authored horizontal/vertical view angles, degrees.
    pub half_angle_horizontal: f32,
    pub half_angle_vertical: f32,
    /// Look duration in ticks (`look_time` is authored in seconds).
    pub look_ticks: (i32, i32),
}

/// `minecraft:behavior.random_look_around`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct RandomLookAroundGoal {
    pub priority: i32,
    pub probability: f32,
    /// Look duration in ticks (authored directly in ticks).
    pub look_ticks: (i32, i32),
    pub angle_range: (f32, f32),
}

/// One `eat_and_replace_block_pairs` entry, resolved against the block
/// registry: any state of `eat` becomes the default state of `replace`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EatBlockPair {
    /// Registry id of the eaten block type.
    pub eat: u32,
    /// Runtime id of the replacement's default state.
    pub replace: u32,
}

/// `minecraft:behavior.eat_block`.
#[derive(Component, Debug, Clone)]
pub struct EatBlockGoal {
    pub priority: i32,
    pub(crate) controls: crate::entity::goal_selector::GoalControls,
    /// Chance per attempt; `None` (an empty expression) never eats.
    pub success_chance: Option<Arc<Expr>>,
    /// Ticks from starting to eat until the block is eaten.
    pub eat_ticks: u32,
    pub pairs: Arc<[EatBlockPair]>,
    pub on_eat: Option<SelfEvent>,
}

/// `minecraft:navigation.walk`: path permissions.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct WalkNavigationConfig {
    pub avoid_water: bool,
    pub avoid_damage_blocks: bool,
    pub can_path_over_water: bool,
    pub can_sink: bool,
    pub can_walk: bool,
    pub can_jump: bool,
}

/// `minecraft:movement.basic`: the move control's turn limit, degrees/tick.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct BasicMovementConfig {
    pub max_turn: f32,
}

/// `minecraft:jump.static`: initial jump velocity.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct JumpConfig {
    pub jump_power: f32,
}

/// `minecraft:breathable`. Applying it also (re)creates the [`AirSupply`] pool.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct BreathableConfig {
    /// Seconds of air.
    pub total_supply: i32,
    /// Seconds between suffocation damage once out of air.
    pub suffocate_time: i32,
    pub breathes_air: bool,
    pub breathes_water: bool,
}

/// `minecraft:nameable`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct NameableConfig {
    pub allow_name_tag_renaming: bool,
    pub always_show: bool,
}

/// One `minecraft:hurt_on_condition` row.
#[derive(Debug, Clone, PartialEq)]
pub struct HurtCondition {
    pub filter: Filter,
    pub cause: Option<ActorDamageCause>,
    pub damage_per_tick: i32,
}

/// `minecraft:hurt_on_condition`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct HurtOnConditionConfig {
    pub conditions: Arc<[HurtCondition]>,
}

/// `minecraft:despawn`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct DespawnConfig {
    /// `(min_distance, max_distance)` blocks, when `despawn_from_distance` is present.
    pub from_distance: Option<(i32, i32)>,
    pub from_chance: bool,
    pub from_inactivity: bool,
    /// Seconds without a player within `min_distance` before the random roll.
    pub inactivity_seconds: i32,
    /// 1-in-N chance per check once inactive.
    pub random_chance: i32,
}

/// `minecraft:pushable_by_entity` / `minecraft:pushable_by_block` presence.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Pushable {
    pub by_entity: bool,
    pub by_block: bool,
}

/// One accepted `feed_items` entry of `minecraft:ageable`.
#[derive(Debug, Clone, PartialEq)]
pub struct FeedEntry {
    pub item: ItemMatcher,
    /// Fraction of `duration` removed from the remaining growth time.
    pub growth: f32,
}

/// `minecraft:ageable`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct AgeableConfig {
    /// Seconds until the baby grows up.
    pub duration: f32,
    pub feed_items: Arc<[FeedEntry]>,
    pub pause_growth_items: Arc<[ItemMatcher]>,
    pub reset_growth_items: Arc<[ItemMatcher]>,
    pub grow_up: Option<SelfEvent>,
}

/// `minecraft:breedable`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct BreedableConfig {
    pub require_tame: bool,
    pub breed_items: Arc<[ItemMatcher]>,
    /// Actor identifiers this mob may mate with.
    pub breeds_with: Arc<[Arc<str>]>,
    /// Cooldown after mating, seconds.
    pub breed_cooldown: f32,
    pub extra_baby_chance: f32,
}

/// `minecraft:offspring`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct OffspringConfig {
    /// `(mate identifier, baby identifier)`.
    pub pairs: Arc<[(Arc<str>, Arc<str>)]>,
    /// Properties copied from a parent to the baby.
    pub property_inheritance: Arc<[Arc<str>]>,
}

/// `minecraft:experience_reward`, compiled Molang.
#[derive(Component, Debug, Clone)]
pub struct ExperienceRewardConfig {
    pub on_bred: Option<Arc<Expr>>,
    pub on_death: Option<Arc<Expr>>,
}

/// `minecraft:loot`: the table name as the loot registry keys it
/// (`entities/cow` for `loot_tables/entities/cow.json`).
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct LootConfig {
    pub table: Arc<str>,
}

/// One `minecraft:interact` entry (the subset vanilla passive mobs use).
#[derive(Debug, Clone, PartialEq)]
pub struct Interaction {
    pub filter: Option<Filter>,
    pub use_item: bool,
    pub swing: bool,
    pub transform_to_item: Option<ItemMatcher>,
    /// Durability the held item loses.
    pub hurt_item: i32,
    /// Loot dropped at the mob.
    pub spawn_items: Option<SpawnItems>,
    /// Fired on the mob once the interaction ran.
    pub on_interact: Option<SelfEvent>,
    /// Ticks the mob ignores rows with a cooldown after this row ran.
    pub cooldown_ticks: u32,
    pub play_sounds: Arc<[Arc<str>]>,
    pub interact_text: Arc<str>,
}

/// An interaction's `spawn_items`: a loot table rolled for the mob.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnItems {
    /// The table name as the loot registry keys it.
    pub table: Arc<str>,
    /// Height above the mob's feet the items appear at.
    pub y_offset: f32,
}

/// `minecraft:interact`.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct InteractConfig {
    pub interactions: Arc<[Interaction]>,
}

/// `minecraft:spawn_egg_interaction`: using this mob's spawn egg on it
/// spawns a baby.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnEggInteraction;

/// Components accepted so the vanilla definition compiles, with no
/// server-side behaviour implemented yet, and why that is acceptable now.
/// The decisions doc's D12 table mirrors this list.
pub const ACCEPTED_WITHOUT_BEHAVIOUR: &[(ActorComponent, &str)] = &[
    (
        ActorComponent::ConditionalBandwidthOptimization,
        "network send-rate tuning only; unastar replicates every significant change",
    ),
    (
        ActorComponent::Leashable,
        "leads are not implemented; using a lead on the mob does nothing",
    ),
    (
        ActorComponent::LeashableTo,
        "leads are not implemented; nothing can be leashed to the mob",
    ),
    (
        ActorComponent::Balloonable,
        "balloons are an Education Edition item absent from vanilla survival",
    ),
    (
        ActorComponent::Rideable,
        "riding is not implemented; baby undead never mount the mob",
    ),
    (
        ActorComponent::BehaviorMountPathing,
        "only runs while ridden, and riding is not implemented",
    ),
    (
        ActorComponent::Boostable,
        "boosting speeds up a ridden mob, and riding is not implemented",
    ),
    (
        ActorComponent::ItemControllable,
        "steering items only act on a ridden mob, and riding is not implemented",
    ),
    (
        ActorComponent::BehaviorControlledByPlayer,
        "only runs while a player rides the mob, and riding is not implemented",
    ),
    (
        ActorComponent::Transformation,
        "vanilla passive mobs only gain it when struck by lightning (the pig \
         turning into a zombified piglin), and lightning is not implemented",
    ),
    (
        ActorComponent::GameEventMovementTracking,
        "emits flap/move/swim game events, which only vibration listeners \
         (sculk sensors, wardens, allays) consume; vibrations are not implemented",
    ),
];

/// Register every mob component kind (besides the core kinds in
/// `definition_components`, flags and families).
pub fn register_component_kinds(builder: &mut ComponentKindsBuilder) -> Result<(), String> {
    insert(builder, |b: body::BehaviorFloat| {
        Ok(FloatGoal {
            priority: b.priority,
            chance_per_tick: b.chance_per_tick_to_float,
        })
    })?;
    insert(builder, |b: body::BehaviorPanic| {
        Ok(PanicGoal {
            base: GoalBase {
                priority: b.priority,
                speed_multiplier: b.speed_multiplier,
            },
            force: b.force,
            ignore_mob_damage: b.ignore_mob_damage,
            prefer_water: b.prefer_water,
            damage_sources: (!b.damage_sources.is_empty()
                && !b.damage_sources.contains(&ActorDamageCause::All))
            .then(|| b.damage_sources.into()),
        })
    })?;
    insert(builder, |b: body::BehaviorBreed| {
        Ok(BreedGoal {
            base: GoalBase {
                priority: b.priority,
                speed_multiplier: b.speed_multiplier,
            },
        })
    })?;
    insert(builder, |b: body::BehaviorTempt| {
        let names = b
            .items
            .iter()
            .map(|item| match item {
                ItemDescriptor::String(name) => Ok(name.as_str()),
                ItemDescriptor::Map(_) => {
                    Err("tag-based tempt items are not implemented".to_owned())
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(TemptGoal {
            base: GoalBase {
                priority: b.priority,
                speed_multiplier: b.speed_multiplier,
            },
            items: items(names)?,
            within_radius: if b.within_radius > 0.0 {
                b.within_radius
            } else {
                constants::TEMPT_DEFAULT_RADIUS
            },
            can_get_scared: b.can_get_scared,
            can_tempt_vertically: b.can_tempt_vertically,
            can_tempt_while_ridden: b.can_tempt_while_ridden,
            stop_distance: if b.stop_distance < 0.0 {
                constants::TEMPT_DEFAULT_STOP_DISTANCE
            } else {
                b.stop_distance
            },
        })
    })?;
    insert(builder, |b: body::BehaviorFollowParent| {
        Ok(FollowParentGoal {
            base: GoalBase {
                priority: b.priority,
                speed_multiplier: b.speed_multiplier,
            },
        })
    })?;
    insert(builder, |b: body::BehaviorRandomStroll| {
        Ok(RandomStrollGoal {
            base: GoalBase {
                priority: b.priority,
                speed_multiplier: b.speed_multiplier,
            },
            xz_dist: b.xz_dist.max(1),
            y_dist: b.y_dist.max(1),
            interval: b.interval,
        })
    })?;
    insert(builder, |b: body::BehaviorLookAtPlayer| {
        Ok(LookAtPlayerGoal {
            priority: b.priority,
            look_distance: b.look_distance,
            probability: b.probability,
            half_angle_horizontal: b.angle_of_view_horizontal as f32 / 2.0,
            half_angle_vertical: b.angle_of_view_vertical as f32 / 2.0,
            look_ticks: (
                (b.look_time.min * 20.0) as i32,
                (b.look_time.max * 20.0) as i32,
            ),
        })
    })?;
    insert(builder, |b: body::BehaviorRandomLookAround| {
        Ok(RandomLookAroundGoal {
            priority: b.priority,
            probability: b.probability,
            look_ticks: (b.look_time.min, b.look_time.max),
            angle_range: (
                b.min_angle_of_view_horizontal,
                b.max_angle_of_view_horizontal,
            ),
        })
    })?;
    insert(builder, |b: body::BehaviorEatBlock| {
        use crate::entity::goal_selector::GoalControls;
        use body::GoalsControlFlags as Flag;
        let controls = if b.control_flags.is_empty() {
            constants::EAT_BLOCK_DEFAULT_CONTROLS
        } else {
            b.control_flags
                .iter()
                .map(|flag| match flag {
                    Flag::Move => GoalControls::MOVE,
                    Flag::Look => GoalControls::LOOK,
                    Flag::Jump => GoalControls::JUMP,
                })
                .fold(GoalControls::NONE, GoalControls::union)
        };
        let pairs = b
            .eat_and_replace_block_pairs
            .iter()
            .map(|pair| {
                Ok(EatBlockPair {
                    eat: block_entry(&pair.eat_block)?.id,
                    replace: block_entry(&pair.replace_block)?.default_state_id,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(EatBlockGoal {
            priority: b.priority,
            controls,
            success_chance: molang(&b.success_chance)?,
            eat_ticks: (b.time_until_eat.max(0.0) * constants::TICKS_PER_SECOND) as u32,
            pairs: pairs.into(),
            on_eat: (!b.on_eat.event.is_empty())
                .then(|| self_event(&EventTrigger::Trigger(b.on_eat.clone())))
                .transpose()?,
        })
    })?;
    insert(builder, |b: body::NavigationWalk| {
        Ok(WalkNavigationConfig {
            avoid_water: b.avoid_water,
            avoid_damage_blocks: b.avoid_damage_blocks,
            can_path_over_water: b.can_path_over_water,
            can_sink: b.can_sink,
            can_walk: b.can_walk,
            can_jump: b.can_jump,
        })
    })?;
    insert(builder, |b: body::MovementBasic| {
        Ok(BasicMovementConfig {
            max_turn: b.max_turn,
        })
    })?;
    insert(builder, |b: body::JumpStatic| {
        Ok(JumpConfig {
            jump_power: b.jump_power,
        })
    })?;
    builder.register(
        |b: body::Breathable| {
            Ok(BreathableConfig {
                total_supply: b.total_supply,
                suffocate_time: b.suffocate_time,
                breathes_air: b.breathes_air,
                breathes_water: b.breathes_water,
            })
        },
        apply_breathable,
        |entity| {
            entity.remove::<(BreathableConfig, AirSupply)>();
        },
    )?;
    insert(builder, |b: body::Nameable| {
        if b.name_actions.is_some() || b.default_trigger.is_some() {
            return Err("name actions are not implemented".into());
        }
        Ok(NameableConfig {
            allow_name_tag_renaming: b.allow_name_tag_renaming,
            always_show: b.always_show,
        })
    })?;
    insert(builder, |b: body::HurtOnCondition| {
        let conditions = b
            .damage_conditions
            .unwrap_or_default()
            .into_iter()
            .map(|row| {
                Ok(HurtCondition {
                    filter: match &row.filters {
                        Some(json) => filter(json)?,
                        None => Filter::always(),
                    },
                    cause: row.cause,
                    damage_per_tick: row.damage_per_tick,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(HurtOnConditionConfig {
            conditions: conditions.into(),
        })
    })?;
    // The inactivity anchor comes and goes with the despawn rules.
    builder.register(
        |b: body::Despawn| {
            if b.filters.is_some() {
                return Err("despawn filters are not implemented".into());
            }
            Ok(DespawnConfig {
                from_distance: b
                    .despawn_from_distance
                    .map(|d| (d.min_distance, d.max_distance)),
                from_chance: b.despawn_from_chance,
                from_inactivity: b.despawn_from_inactivity,
                inactivity_seconds: b.min_range_inactivity_timer,
                random_chance: b.min_range_random_chance,
            })
        },
        |config: &DespawnConfig, entity| {
            let inactivity = super::despawn::Inactivity::starting_now(entity.world());
            entity.insert((*config, inactivity));
        },
        |entity| {
            entity.remove::<(DespawnConfig, super::despawn::Inactivity)>();
        },
    )?;
    builder.register(
        |_: body::PushableByEntity| Ok(()),
        |_, entity| set_pushable(entity, |p| p.by_entity = true),
        |entity| set_pushable(entity, |p| p.by_entity = false),
    )?;
    builder.register(
        |_: body::PushableByBlock| Ok(()),
        |_, entity| set_pushable(entity, |p| p.by_block = true),
        |entity| set_pushable(entity, |p| p.by_block = false),
    )?;
    builder.register(
        |b: body::Ageable| {
            let feed_items = b
                .feed_items
                .unwrap_or_default()
                .iter()
                .map(|entry| {
                    Ok(match entry {
                        FeedItem::Item(name) => FeedEntry {
                            item: ItemMatcher::parse(name)?,
                            growth: constants::AGEABLE_FEED_GROWTH,
                        },
                        FeedItem::Entry(entry) => {
                            if entry.result_item.is_some() {
                                return Err("feed result items are not implemented".to_owned());
                            }
                            FeedEntry {
                                item: ItemMatcher::parse(&entry.item)?,
                                growth: entry.growth.unwrap_or(constants::AGEABLE_FEED_GROWTH),
                            }
                        }
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            if b.drop_items.is_some()
                || b.transform_to_item.is_some()
                || b.interact_filters.is_some()
            {
                return Err(
                    "ageable drop/transform items and interact filters are not implemented".into(),
                );
            }
            Ok(AgeableConfig {
                duration: b.duration,
                feed_items: feed_items.into(),
                pause_growth_items: items(
                    b.pause_growth_items.iter().flatten().map(String::as_str),
                )?,
                reset_growth_items: items(
                    b.reset_growth_items.iter().flatten().map(String::as_str),
                )?,
                grow_up: b.grow_up.as_ref().map(self_event).transpose()?,
            })
        },
        // Growth starts with the component; a re-applied composition (or a
        // restored save) keeps the running deadline. A world without the
        // server tick clock counts from tick 0.
        |value: &AgeableConfig, entity| {
            entity.insert(value.clone());
            if !entity.contains::<BabyAge>() {
                let now = entity
                    .world()
                    .get_resource::<crate::ecs::resources::TickCounter>()
                    .map_or(0, |tick| tick.current);
                entity.insert(BabyAge::new(value, now));
            }
        },
        |entity| {
            entity.remove::<(AgeableConfig, BabyAge)>();
        },
    )?;
    insert(builder, |b: body::Breedable| {
        let breed_items = b
            .breed_items
            .unwrap_or_default()
            .iter()
            .map(|item| match item {
                BreedItem::Item(name) => ItemMatcher::parse(name),
                BreedItem::Entry(entry) if entry.result_item.is_none() => {
                    ItemMatcher::parse(&entry.item)
                }
                BreedItem::Entry(_) => Err("breed result items are not implemented".to_owned()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let breeds_with: Vec<Arc<str>> = match b.breeds_with {
            None => Vec::new(),
            Some(BreedsWith::ByMate(mates)) => {
                mates.keys().map(|k| Arc::from(k.as_str())).collect()
            }
            Some(BreedsWith::One(entry)) => entry
                .mate_type
                .iter()
                .map(|m| Arc::from(m.as_str()))
                .collect(),
            Some(BreedsWith::Many(entries)) => entries
                .iter()
                .filter_map(|e| e.mate_type.as_deref())
                .map(Arc::from)
                .collect(),
        };
        if b.love_filters.is_some() || b.environment_requirements.is_some() || b.causes_pregnancy {
            return Err(
                "love filters, environment requirements and pregnancy are not implemented".into(),
            );
        }
        Ok(BreedableConfig {
            require_tame: b.require_tame,
            breed_items: breed_items.into(),
            breeds_with: breeds_with.into(),
            breed_cooldown: b.breed_cooldown,
            extra_baby_chance: b.extra_baby_chance,
        })
    })?;
    insert(builder, |b: body::Offspring| {
        let pairs: Vec<(Arc<str>, Arc<str>)> = b
            .offspring_pairs
            .unwrap_or_default()
            .into_iter()
            .map(|(mate, baby)| (Arc::from(mate), Arc::from(baby)))
            .collect();
        let property_inheritance: Vec<Arc<str>> = match b.property_inheritance {
            None => Vec::new(),
            Some(InheritedProperties::Names(names)) => names.into_iter().map(Arc::from).collect(),
            Some(InheritedProperties::ByProperty(map)) => map.into_keys().map(Arc::from).collect(),
        };
        Ok(OffspringConfig {
            pairs: pairs.into(),
            property_inheritance: property_inheritance.into(),
        })
    })?;
    insert(builder, |b: body::ExperienceReward| {
        Ok(ExperienceRewardConfig {
            on_bred: molang(&b.on_bred)?,
            on_death: molang(&b.on_death)?,
        })
    })?;
    insert(builder, |b: body::Loot| {
        let path = b.table.ok_or_else(|| "loot requires `table`".to_owned())?;
        Ok(LootConfig {
            table: loot_table(&path)?,
        })
    })?;
    insert(builder, |b: body::Interact| {
        let interactions = b
            .interactions
            .iter()
            .map(compile_interaction)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(InteractConfig {
            interactions: interactions.into(),
        })
    })?;
    builder.register(
        |_: body::SpawnEggInteraction| Ok(SpawnEggInteraction),
        |value, entity| {
            entity.insert(*value);
        },
        |entity| {
            entity.remove::<SpawnEggInteraction>();
        },
    )?;
    inert::<body::ConditionalBandwidthOptimization>(builder)?;
    inert::<body::Leashable>(builder)?;
    inert::<body::LeashableTo>(builder)?;
    inert::<body::Balloonable>(builder)?;
    inert::<body::Rideable>(builder)?;
    inert::<body::BehaviorMountPathing>(builder)?;
    inert::<body::Boostable>(builder)?;
    inert::<body::ItemControllable>(builder)?;
    inert::<body::BehaviorControlledByPlayer>(builder)?;
    inert::<body::Transformation>(builder)?;
    inert::<body::GameEventMovementTracking>(builder)?;
    Ok(())
}

/// Vanilla's interaction rows use only a subset of the published fields;
/// anything that would need an unimplemented effect rejects the definition.
/// `vibration` is accepted: nothing listens for vibrations yet, so emitting
/// one has no observable effect.
fn compile_interaction(row: &body::InteractionData) -> Result<Interaction, String> {
    if row.give_item
        || row.take_item
        || row.admire
        || row.barter
        || row.health_amount != 0
        || !row.spawn_entities.is_empty()
        || !row.add_items.table.is_empty()
        || row.drop_item_slot.is_some()
        || row.equip_item_slot.is_some()
        || row.repair_entity_item.slot.is_some()
        || row.particle_on_start.particle_type != body::ParticleType::None
        || row.cooldown_after_being_attacked != 0.0
    {
        return Err(
            "interaction effects beyond item use/transform/wear, loot drops and events are not \
             implemented"
                .into(),
        );
    }
    if row.hurt_item < 0 {
        return Err(format!("negative hurt_item {}", row.hurt_item));
    }
    let on_interact = if row.on_interact.event.is_empty() {
        None
    } else if row.on_interact.target != FilterSubject::SelfValue {
        return Err(format!(
            "interaction event {} targets {:?}; only self is supported",
            row.on_interact.event, row.on_interact.target
        ));
    } else {
        Some(SelfEvent(Arc::from(row.on_interact.event.as_str())))
    };
    Ok(Interaction {
        filter: row.on_interact.filters.as_ref().map(filter).transpose()?,
        use_item: row.use_item,
        swing: row.swing,
        transform_to_item: (!row.transform_to_item.is_empty())
            .then(|| ItemMatcher::parse(&row.transform_to_item))
            .transpose()?,
        hurt_item: row.hurt_item,
        spawn_items: (!row.spawn_items.table.is_empty())
            .then(|| {
                Ok::<_, String>(SpawnItems {
                    table: loot_table(&row.spawn_items.table)?,
                    y_offset: row.spawn_items.y_offset,
                })
            })
            .transpose()?,
        on_interact,
        cooldown_ticks: (row.cooldown.max(0.0) * constants::TICKS_PER_SECOND) as u32,
        play_sounds: row
            .play_sounds
            .iter()
            .map(|s| Arc::from(s.as_str()))
            .collect(),
        interact_text: Arc::from(row.interact_text.as_str()),
    })
}

/// A behaviour-pack loot table path as the loot registry keys it
/// (`entities/cow` for `loot_tables/entities/cow.json`); unknown tables
/// reject the definition.
fn loot_table(path: &str) -> Result<Arc<str>, String> {
    let table = path
        .strip_prefix("loot_tables/")
        .unwrap_or(path)
        .trim_end_matches(".json");
    if unastar_data::loot::get(table).is_none() {
        return Err(format!(
            "loot table {path} is not in the embedded loot registry"
        ));
    }
    Ok(Arc::from(table))
}

/// The block a behaviour-pack block name means: `minecraft:` is implied and
/// legacy names ([`constants::LEGACY_BLOCK_NAMES`]) resolve to today's block.
fn block_entry(name: &str) -> Result<&'static crate::registry::block::BlockEntry, String> {
    let name = if name.contains(':') {
        name.to_owned()
    } else {
        format!("minecraft:{name}")
    };
    let name = constants::LEGACY_BLOCK_NAMES
        .iter()
        .find(|(legacy, _)| *legacy == name)
        .map_or(name.as_str(), |(_, modern)| *modern);
    crate::registry::BlockRegistry::vanilla()
        .get_by_name(name)
        .ok_or_else(|| format!("unknown block {name}"))
}

fn apply_breathable(value: &BreathableConfig, entity: &mut EntityWorldMut<'_>) {
    let max_ticks = (value.total_supply * 20).max(0) as u32;
    entity.insert((
        *value,
        AirSupply {
            current_ticks: max_ticks,
            max_ticks,
        },
    ));
}

fn set_pushable(entity: &mut EntityWorldMut<'_>, edit: impl FnOnce(&mut Pushable)) {
    if !entity.contains::<Pushable>() {
        entity.insert(Pushable::default());
    }
    edit(&mut entity.get_mut::<Pushable>().expect("inserted above"));
}

/// A plain config component: compile from the typed body, insert a clone,
/// remove on composition change.
pub(super) fn insert<B: ComponentBody, T: Component + Clone>(
    builder: &mut ComponentKindsBuilder,
    compile: impl Fn(B) -> Result<T, String> + Send + Sync + 'static,
) -> Result<(), String> {
    builder.register(
        compile,
        |value: &T, entity| {
            entity.insert(value.clone());
        },
        |entity| {
            entity.remove::<T>();
        },
    )
}

/// See [`ACCEPTED_WITHOUT_BEHAVIOUR`].
fn inert<B: ComponentBody>(builder: &mut ComponentKindsBuilder) -> Result<(), String> {
    debug_assert!(
        ACCEPTED_WITHOUT_BEHAVIOUR
            .iter()
            .any(|(component, _)| *component == B::COMPONENT)
    );
    builder.register(|_: B| Ok(()), |_, _| {}, |_| {})
}

pub(super) fn filter(json: &FilterJson) -> Result<Filter, String> {
    let mut diagnostics = unastar_rules::Diagnostics::default();
    let filter = Filter::parse(json, FilterRegistry::vanilla(), "filters", &mut diagnostics)
        .map_err(|e| e.to_string())?;
    if !diagnostics.items.is_empty() {
        return Err(format!("filter diagnostics {:?}", diagnostics.items));
    }
    super::archetype::check_filter(&filter)?;
    Ok(filter)
}

/// A Molang value; an empty string (the published default) is `None`.
pub(super) fn molang(value: &MolangValue) -> Result<Option<Arc<Expr>>, String> {
    let source = match value {
        MolangValue::Float(n) => n.to_string(),
        MolangValue::MolangString(MolangString::String(s)) => s.clone(),
        MolangValue::MolangString(MolangString::MolangExpressionObject(o)) => o.expression.clone(),
    };
    if source.trim().is_empty() {
        return Ok(None);
    }
    let mut expr = parse(&source).map_err(|e| format!("molang {source:?}: {e}"))?;
    expr.bind(QueryRegistry::vanilla(), QueryContext::Runtime)
        .map_err(|e| format!("molang {source:?}: {e}"))?;
    Ok(Some(Arc::new(expr)))
}
