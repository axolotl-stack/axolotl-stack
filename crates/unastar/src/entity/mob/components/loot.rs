//! Reward components: `minecraft:loot` and `minecraft:experience_reward`
//! (paid out by `mob::combat::death` and `mob::life::breeding`).

use std::sync::Arc;

use bevy_ecs::prelude::*;
use thiserror::Error;
use unastar_data::actor_components::body;
use unastar_rules::molang::Expr;

use super::common::{MolangError, UnknownLootTable, insert, loot_table, molang};
use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};

/// Why a reward body cannot be compiled.
#[derive(Debug, Error)]
pub enum RewardError {
    #[error("loot requires `table`")]
    MissingTable,
    #[error(transparent)]
    Table(#[from] UnknownLootTable),
    #[error(transparent)]
    Molang(#[from] MolangError),
}

impl ComponentError for RewardError {}

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

/// Register the experience and loot component kinds.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
) -> Result<(), DefinitionError> {
    insert(builder, compile_experience_reward)?;
    insert(builder, |b: body::Loot| {
        let path = b.table.ok_or(RewardError::MissingTable)?;
        Ok::<_, RewardError>(LootConfig {
            table: loot_table(&path)?,
        })
    })
}

/// Compile a `minecraft:experience_reward` body (mobs through the component
/// registry; players from the vanilla player definition).
pub(crate) fn compile_experience_reward(
    b: body::ExperienceReward,
) -> Result<ExperienceRewardConfig, RewardError> {
    Ok(ExperienceRewardConfig {
        on_bred: molang(&b.on_bred)?,
        on_death: molang(&b.on_death)?,
    })
}
