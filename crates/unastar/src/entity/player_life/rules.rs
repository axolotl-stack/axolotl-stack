//! The player's behaviour-pack facts the life systems use, compiled once from
//! the vanilla `minecraft:player` definition: its collision box, its
//! `minecraft:hurt_on_condition` rows (lava) and its
//! `minecraft:experience_reward` death payout.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::{ComponentBody, body, decode};
use unastar_data::actor_definitions::ActorDefinition;
use unastar_data::actor_identifiers::ActorType;
use unastar_rules::filter::{BuiltinTest, Filter, Subject};
use unastar_rules::molang::{BuiltinQuery, Expr};

use crate::entity::mob::components::hurt_on_condition::{
    self, HurtCondition, HurtOnConditionError,
};
use crate::entity::mob::components::loot::{self, RewardError};

/// Why the player definition cannot drive the life systems.
#[derive(Debug, Error)]
pub enum PlayerRulesError {
    #[error("the vanilla pack has no {0} definition")]
    MissingDefinition(&'static str),
    #[error("player definition lacks {0}")]
    MissingComponent(&'static str),
    #[error("player {component}: {message}")]
    Decode {
        component: &'static str,
        message: String,
    },
    #[error(transparent)]
    HurtOnCondition(#[from] HurtOnConditionError),
    #[error(transparent)]
    Reward(#[from] RewardError),
    #[error("player filter test {test} on {subject} is not implemented")]
    UnsupportedFilterTest { test: String, subject: &'static str },
    #[error("player experience reward uses unsupported query.{0}")]
    UnsupportedQuery(Arc<str>),
}

/// Compiled player facts (a resource inserted by `player_life::install`).
#[derive(Resource, Debug, Clone)]
pub struct PlayerRules {
    /// Collision box width and height in blocks.
    pub width: f64,
    pub height: f64,
    /// `minecraft:hurt_on_condition` rows, checked every tick.
    pub hurt_conditions: Arc<[HurtCondition]>,
    /// `minecraft:experience_reward` `on_death`: the experience a dying
    /// player drops.
    pub experience_on_death: Option<Arc<Expr>>,
}

impl PlayerRules {
    /// Compile the vanilla player definition.
    pub fn vanilla() -> Result<Self, PlayerRulesError> {
        let name = ActorType::Player.name();
        let definition = unastar_data::actor_definitions::get(name)
            .ok_or(PlayerRulesError::MissingDefinition(name))?;
        Self::compile(definition)
    }

    pub fn compile(definition: &ActorDefinition) -> Result<Self, PlayerRulesError> {
        let collision: body::CollisionBox = component(definition)?;
        let hurt = hurt_on_condition::compile(component(definition)?)?;
        for condition in hurt.conditions.iter() {
            check_filter(&condition.filter)?;
        }
        let reward = loot::compile_experience_reward(component(definition)?)?;
        if let Some(expr) = &reward.on_death {
            check_queries(expr)?;
        }
        Ok(Self {
            width: f64::from(collision.width.max(0.0)),
            height: f64::from(collision.height.max(0.0)),
            hurt_conditions: hurt.conditions,
            experience_on_death: reward.on_death,
        })
    }
}

fn component<B: ComponentBody>(definition: &ActorDefinition) -> Result<B, PlayerRulesError> {
    let name = B::COMPONENT.name();
    let body = definition
        .components
        .get(name)
        .ok_or(PlayerRulesError::MissingComponent(name))?;
    decode::<B>(body).map_err(|message| PlayerRulesError::Decode {
        component: name,
        message,
    })
}

/// Filter tests the player facts answer (see `environment::PlayerFacts`).
fn check_filter(filter: &Filter) -> Result<(), PlayerRulesError> {
    let mut failure = None;
    filter.for_each_test(&mut |test| {
        let supported = matches!(
            (test.builtin(), test.subject),
            (
                Some(BuiltinTest::InLava | BuiltinTest::InWater),
                Subject::Self_
            )
        );
        if !supported && failure.is_none() {
            failure = Some(PlayerRulesError::UnsupportedFilterTest {
                test: test.name.clone(),
                subject: test.subject.name(),
            });
        }
    });
    failure.map_or(Ok(()), Err)
}

/// Molang queries the player experience context answers
/// (`death::PlayerMolang`).
fn check_queries(expr: &Expr) -> Result<(), PlayerRulesError> {
    let mut failure = None;
    expr.walk(&mut |node| {
        if let Expr::Query(query, _) = node
            && query.id.and_then(BuiltinQuery::from_id) != Some(BuiltinQuery::PlayerLevel)
            && failure.is_none()
        {
            failure = Some(PlayerRulesError::UnsupportedQuery(query.name.clone()));
        }
    });
    failure.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_player_definition_compiles() {
        let rules = PlayerRules::vanilla().expect("vanilla player rules");
        assert_eq!(
            (rules.width, rules.height),
            (0.6_f32 as f64, 1.8_f32 as f64)
        );
        assert_eq!(rules.hurt_conditions.len(), 1);
        assert_eq!(rules.hurt_conditions[0].damage_per_tick, 4);
        assert!(rules.experience_on_death.is_some());
    }
}
