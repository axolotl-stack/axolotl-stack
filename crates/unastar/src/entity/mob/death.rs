//! Mob death: the death animation, loot and experience.
//!
//! When a mob's health reaches zero it is marked [`Dead`] (AI, interaction
//! and despawn skip it), its viewers get `ActorEvent` death and its loot
//! table is rolled and dropped at once. The body stays for the death
//! animation, [`constants::DEATH_TICKS`] after the death tick (the native
//! death counter is 1 on the death tick and the body goes once it passes
//! 20); then the mob's `experience_reward.on_death` is spawned as
//! experience orbs and the entity is removed (viewers get `RemoveActor`).

use bevy_ecs::prelude::*;
use jolyne::valentine::actor_flags;
use jolyne::valentine::types::EnumsActorEvent;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tracing::warn;
use unastar_rules::molang::eval::{MolangContext, Value as MolangValue, evaluate_num};
use unastar_rules::molang::{BuiltinQuery, Expr, QueryId};

use super::combat::send_actor_event;
use super::config::{ExperienceRewardConfig, LootConfig};
use super::events::{held_item, is_family};
use super::state::{Dead, LastHurt};
use super::{MobKind, constants};
use crate::ecs::resources::TickCounter;
use crate::entity::components::{Health, OnFire, OnGround, Position};
use crate::entity::flags::ActorFlags;
use crate::entity::xp_orb::spawn_experience;
use crate::entity::{EntityRandom, spawn_item_drop};
use crate::item::enchant::{Enchantment, enchantment_level};
use crate::server::game::types::RecipeRegistryResource;
use crate::server::game::{LootContext, loot_stacks};

/// The death animation of a [`Dead`] mob: the server tick its body is
/// removed at.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct Dying {
    pub remove_at_tick: u64,
}

/// Whether the last hit on `mob` came from a player recently enough to
/// count as a player kill ([`constants::LAST_HIT_BY_PLAYER_TICKS`]).
pub fn last_hit_by_player(world: &World, mob: Entity) -> bool {
    let now = world.resource::<TickCounter>().current;
    world.get::<LastHurt>(mob).is_some_and(|hurt| {
        hurt.by_player && now.saturating_sub(hurt.tick) < constants::LAST_HIT_BY_PLAYER_TICKS
    })
}

/// Start the death of `mob`: mark it, tell viewers and drop its loot.
pub fn begin_death(world: &mut World, mob: Entity) {
    if world.get::<Dead>(mob).is_some() || world.get::<MobKind>(mob).is_none() {
        return;
    }
    let now = world.resource::<TickCounter>().current;
    world.entity_mut(mob).insert((
        Dead,
        Dying {
            remove_at_tick: now + u64::from(constants::DEATH_TICKS),
        },
    ));
    send_actor_event(world, mob, EnumsActorEvent::Death, 0);
    drop_loot(world, mob);
}

/// Roll the mob's `minecraft:loot` table and drop the result at its feet.
/// The killer's held item supplies looting; `on_fire` is the mob's fire
/// state (cooked drops through `furnace_smelt`).
fn drop_loot(world: &mut World, mob: Entity) {
    let Some(name) = world.get::<LootConfig>(mob).map(|loot| loot.table.clone()) else {
        return;
    };
    let Some(table) = unastar_data::loot::get(&name) else {
        warn!(table = %name, "Mob loot table is not in the loot registry");
        return;
    };
    let Some(position) = world.get::<Position>(mob).map(|p| p.0) else {
        return;
    };
    let killed_by_player = last_hit_by_player(world, mob);
    let tool = world
        .get::<LastHurt>(mob)
        .and_then(|hurt| hurt.attacker)
        .filter(|_| killed_by_player)
        .and_then(|killer| held_item(world, killer))
        .cloned();
    let on_fire = world.get::<OnFire>(mob).is_some_and(OnFire::is_on_fire);
    let recipes = world
        .get_resource::<RecipeRegistryResource>()
        .map(|r| r.0.clone());
    let seed = world
        .get_resource_or_init::<EntityRandom>()
        .0
        .r#gen::<u64>();
    let ctx = LootContext {
        tool: tool.as_ref(),
        block: None,
        killed_by_player,
        looting: tool
            .as_ref()
            .map_or(0, |tool| enchantment_level(tool, Enchantment::Looting)),
        on_fire: Some(on_fire),
        recipes: recipes.as_deref(),
    };
    let drops = match unastar_rules::loot::roll(
        unastar_data::loot::vanilla(),
        table,
        &ctx,
        &mut StdRng::seed_from_u64(seed),
    ) {
        Ok(drops) => drops,
        Err(error) => {
            warn!(table = %name, %error, "Mob loot table cannot be evaluated");
            return;
        }
    };
    for stack in loot_stacks(world, drops) {
        spawn_item_drop(world, position, stack, None);
    }
}

/// Evaluate an `experience_reward` expression for `mob`.
///
/// The Molang value is rounded to the nearest whole point (provisional:
/// the cow's `Math.Random(1,3)` must reach the 1–3 experience the Minecraft
/// Wiki lists for cows); negative results give none.
pub fn experience_reward(world: &mut World, mob: Entity, expr: &Expr) -> u32 {
    let seed = world
        .get_resource_or_init::<EntityRandom>()
        .0
        .r#gen::<u64>();
    let mut ctx = RewardMolang {
        world,
        mob,
        last_hit_by_player: last_hit_by_player(world, mob),
        rng: StdRng::seed_from_u64(seed),
    };
    match evaluate_num(expr, &mut ctx) {
        Ok(value) => value.round().max(0.0) as u32,
        Err(error) => {
            warn!(%error, "Experience reward cannot be evaluated");
            0
        }
    }
}

/// Molang facts about a mob for its experience reward.
struct RewardMolang<'w> {
    world: &'w World,
    mob: Entity,
    last_hit_by_player: bool,
    rng: StdRng,
}

fn molang_bool(value: bool) -> MolangValue {
    MolangValue::Num(if value { 1.0 } else { 0.0 })
}

impl MolangContext for RewardMolang<'_> {
    fn query(&mut self, id: QueryId, args: &[MolangValue]) -> Option<MolangValue> {
        Some(match BuiltinQuery::from_id(id)? {
            BuiltinQuery::LastHitByPlayer => molang_bool(self.last_hit_by_player),
            BuiltinQuery::IsBaby => molang_bool(
                self.world
                    .get::<ActorFlags>(self.mob)
                    .is_some_and(|flags| flags.get(actor_flags::BABY).unwrap_or(false)),
            ),
            BuiltinQuery::IsOnGround => {
                molang_bool(self.world.get::<OnGround>(self.mob).is_some_and(|g| g.0))
            }
            // Mobs hold no equipment in unastar.
            BuiltinQuery::EquipmentCount => MolangValue::Num(0.0),
            BuiltinQuery::HasAnyFamily => molang_bool(args.iter().any(|arg| match arg {
                MolangValue::Str(family) => is_family(self.world, self.mob, family),
                _ => false,
            })),
            _ => return None,
        })
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

/// System (exclusive): mobs whose health reached zero outside
/// [`super::combat::hurt_mob`] (fire, effects) die too.
#[allow(clippy::type_complexity)]
pub fn die_at_zero_health(
    world: &mut World,
    mobs: &mut QueryState<(Entity, &Health), (With<MobKind>, Without<Dead>)>,
    mut dead: Local<Vec<Entity>>,
) {
    dead.extend(
        mobs.iter(world)
            .filter(|(_, health)| health.is_dead())
            .map(|(entity, _)| entity),
    );
    for mob in dead.drain(..) {
        begin_death(world, mob);
    }
}

/// System (exclusive): at the end of a death animation spawn the
/// experience reward and remove the body.
pub fn tick_dying(
    world: &mut World,
    dying: &mut QueryState<(Entity, &Dying)>,
    mut finished: Local<Vec<Entity>>,
) {
    let now = world.resource::<TickCounter>().current;
    finished.extend(
        dying
            .iter(world)
            .filter(|(_, death)| now >= death.remove_at_tick)
            .map(|(entity, _)| entity),
    );
    for mob in finished.drain(..) {
        let reward = world
            .get::<ExperienceRewardConfig>(mob)
            .and_then(|config| config.on_death.clone());
        let position = world.get::<Position>(mob).map(|p| p.0);
        if let (Some(expr), Some(position)) = (reward, position) {
            let amount = experience_reward(world, mob, &expr);
            spawn_experience(world, position, amount);
        }
        world.despawn(mob);
    }
}
