//! `/recipe give|take <player> <recipe|*>`: the recipe book.
//!
//! Measured on BDS 1.26.51 (command capture, `recipe` segment; the second
//! player's join for the initial list):
//!
//! - the recipes are the `UnlockableRecipeValues` of the command syntax
//!   (another id answers `commands.recipes.notFound`);
//! - `give` sends `UnlockedRecipes` `Newlyunlockedrecipes` with the recipe,
//!   or with every recipe not yet unlocked in syntax order for `*`, and
//!   answers `commands.recipes.unlockedRecipe [player, recipe]` /
//!   `unlockedAllRecipes [player]`; a recipe already unlocked answers
//!   `playerHasRecipe`;
//! - `take` sends `Removeunlockedrecipes`, or for `*`
//!   `Removeallunlockedrecipes` followed by the initial recipes again
//!   (`Initiallyunlockedrecipes`), and answers `removedRecipe` /
//!   `removedAllRecipes`; a recipe not unlocked answers
//!   `playerDoesNotHaveRecipe`;
//! - an unknown player answers `noTargetMatch` then
//!   `commands.generic.player.notFound`.
//!
//! Every player starts with [`INITIAL_RECIPES`]; the list is saved with the
//! player record. Items reaching an inventory do not unlock recipes yet (BDS
//! does, with the `recipesUnlock` rule).

use crate::command::prelude::*;
use crate::entity::components::Player;
use jolyne::valentine::UnlockedRecipesPacket;
use jolyne::valentine::types::EnumsUnlockedRecipesPacketPayloadPacketType as UnlockType;

/// The recipes every player has from the start, in the order BDS lists them
/// (measured: its join and `/recipe take @s *`). They are the recipes the
/// vanilla data marks `AlwaysUnlocked` and the furnace recipes without an
/// unlock condition.
pub const INITIAL_RECIPES: [&str; 20] = [
    "minecraft:WorkBench_recipeId_from_oak",
    "minecraft:furnace_acacia_wood",
    "minecraft:furnace_birch_wood",
    "minecraft:furnace_dark_oak_wood",
    "minecraft:furnace_jungle_wood",
    "minecraft:furnace_log2_acacia",
    "minecraft:furnace_log2_dark_oak",
    "minecraft:furnace_log_birch",
    "minecraft:furnace_log_jungle",
    "minecraft:furnace_log_oak",
    "minecraft:furnace_log_spruce",
    "minecraft:furnace_oak_wood",
    "minecraft:furnace_spruce_wood",
    "minecraft:furnace_stripped_acacia_wood",
    "minecraft:furnace_stripped_birch_wood",
    "minecraft:furnace_stripped_dark_oak_wood",
    "minecraft:furnace_stripped_jungle_wood",
    "minecraft:furnace_stripped_oak_wood",
    "minecraft:furnace_stripped_spruce_wood",
    "minecraft:WorkBench_recipeId",
];

/// The recipes a player has unlocked, in unlock order.
#[derive(bevy_ecs::prelude::Component, Debug, Clone, PartialEq, Eq)]
pub struct UnlockedRecipes(pub Vec<String>);

impl Default for UnlockedRecipes {
    fn default() -> Self {
        Self(INITIAL_RECIPES.iter().map(|r| (*r).to_owned()).collect())
    }
}

/// The join's `UnlockedRecipes`: the player's recipes, as initially
/// unlocked ones.
pub fn join_packet(unlocked: &UnlockedRecipes) -> UnlockedRecipesPacket {
    packet(UnlockType::Initiallyunlockedrecipes, unlocked.0.clone())
}

fn packet(kind: UnlockType, recipes: Vec<String>) -> UnlockedRecipesPacket {
    UnlockedRecipesPacket {
        packet_type: kind,
        unlocked_recipes_list: recipes.into_iter().map(Into::into).collect(),
    }
}

overloads! {
    pub enum RecipeArgs for "recipe" {
        0 => Give { give: EnumValue, player: Target, recipe: SoftEnumValue },
        1 => Take { take: EnumValue, player: Target, recipe: SoftEnumValue },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: RecipeArgs) -> CommandResult {
    let (give, target, recipe) = match args {
        RecipeArgs::Give { player, recipe, .. } => (true, player, recipe),
        RecipeArgs::Take { player, recipe, .. } => (false, player, recipe),
    };
    let players = match ctx.players(Some(&target)) {
        Ok(players) => players,
        Err(mut output) => {
            if output
                .messages
                .first()
                .is_some_and(|m| m.key == "commands.generic.noTargetMatch")
            {
                output.add_failure("commands.generic.player.notFound", vec![]);
            }
            return Err(output);
        }
    };
    let all: Vec<String> = ctx
        .schema
        .syntax()
        .soft_enums
        .iter()
        .find(|e| e.name == "UnlockableRecipeValues")
        .map(|e| e.values.iter().filter(|v| *v != "*").cloned().collect())
        .unwrap_or_default();
    let wildcard = &*recipe == "*";
    if !wildcard && !all.iter().any(|r| *r == *recipe) {
        return failure!("commands.recipes.notFound");
    }
    let mut output = CommandOutput::default();
    for &player in &players {
        if ctx.world.get::<Player>(player).is_none() {
            continue;
        }
        let name = ctx.name(player);
        let mut unlocked = ctx
            .world
            .get::<UnlockedRecipes>(player)
            .cloned()
            .unwrap_or_default();
        match (give, wildcard) {
            (true, true) => {
                // Syntax order, once each (the syntax lists the recipes
                // furnaces, blast furnaces and smokers share once per
                // station).
                let mut new: Vec<String> = Vec::new();
                for recipe in &all {
                    if !unlocked.0.contains(recipe) && !new.contains(recipe) {
                        new.push(recipe.clone());
                    }
                }
                if !new.is_empty() {
                    unlocked.0.extend(new.iter().cloned());
                    ctx.send(player, packet(UnlockType::Newlyunlockedrecipes, new));
                }
                output.add_success("commands.recipes.unlockedAllRecipes", vec![name]);
            }
            (true, false) => {
                if unlocked.0.iter().any(|r| *r == *recipe) {
                    output.add_failure(
                        "commands.recipes.playerHasRecipe",
                        vec![name, recipe.0.clone()],
                    );
                } else {
                    unlocked.0.push(recipe.0.clone());
                    ctx.send(
                        player,
                        packet(UnlockType::Newlyunlockedrecipes, vec![recipe.0.clone()]),
                    );
                    output.add_success(
                        "commands.recipes.unlockedRecipe",
                        vec![name, recipe.0.clone()],
                    );
                }
            }
            (false, true) => {
                unlocked = UnlockedRecipes::default();
                ctx.send(player, packet(UnlockType::Removeallunlockedrecipes, vec![]));
                ctx.send(player, join_packet(&unlocked));
                output.add_success("commands.recipes.removedAllRecipes", vec![name]);
            }
            (false, false) => match unlocked.0.iter().position(|r| *r == *recipe) {
                Some(index) => {
                    unlocked.0.remove(index);
                    ctx.send(
                        player,
                        packet(UnlockType::Removeunlockedrecipes, vec![recipe.0.clone()]),
                    );
                    output.add_success(
                        "commands.recipes.removedRecipe",
                        vec![name, recipe.0.clone()],
                    );
                }
                None => output.add_failure(
                    "commands.recipes.playerDoesNotHaveRecipe",
                    vec![name, recipe.0.clone()],
                ),
            },
        }
        ctx.world.entity_mut(player).insert(unlocked);
    }
    if output.is_success() {
        Ok(output)
    } else {
        Err(output)
    }
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use super::*;
    use crate::command::exec::evidence::{Replay, assert_same_packets, segment};
    use jolyne::valentine::McpePacketData;

    #[test]
    fn recipe_matches_the_bds_capture() {
        // BDS's player had unlocked recipes from the items earlier segments
        // gave it, so its `give *` list is shorter; it is compared below.
        let give_all = "/recipe give @s *";
        let steps = Replay::new().assert_segment("recipe", &[]);
        assert_same_packets(&steps, &["UnlockedRecipesPacket"], &[give_all]);
        let first = steps
            .iter()
            .find(|s| s.captured.command == give_all)
            .expect("give *");
        let lists = |packets: Vec<&McpePacketData>| -> Vec<Vec<String>> {
            packets
                .into_iter()
                .filter_map(|p| match p {
                    McpePacketData::UnlockedRecipesPacket(p) => Some(
                        p.unlocked_recipes_list
                            .iter()
                            .map(|r| r.to_string())
                            .collect(),
                    ),
                    _ => None,
                })
                .collect()
        };
        let (ours, theirs) = (
            lists(first.sent_named("UnlockedRecipesPacket")),
            lists(first.captured.named("UnlockedRecipesPacket")),
        );
        // The same order, BDS's being ours without what it had unlocked.
        let kept: Vec<&String> = ours[0].iter().filter(|r| theirs[0].contains(r)).collect();
        assert_eq!(kept, theirs[0].iter().collect::<Vec<_>>());
    }

    #[test]
    fn the_join_lists_the_initial_recipes_like_bds() {
        let join = segment("players")
            .into_iter()
            .find(|c| c.command == "!join Alice")
            .expect("Alice's join");
        // BDS's order of the initial recipes changes from list to list
        // (`take *` listed them in INITIAL_RECIPES' order): compare the set.
        let sorted = |packet: &McpePacketData| match packet {
            McpePacketData::UnlockedRecipesPacket(p) => {
                let mut list: Vec<String> = p
                    .unlocked_recipes_list
                    .iter()
                    .map(|r| r.to_string())
                    .collect();
                list.sort();
                (format!("{:?}", p.packet_type), list)
            }
            other => panic!("not UnlockedRecipes: {other:?}"),
        };
        let theirs = join.client_named("Alice", "UnlockedRecipesPacket");
        let ours = McpePacketData::UnlockedRecipesPacket(join_packet(&UnlockedRecipes::default()));
        assert_eq!(theirs.len(), 1);
        assert_eq!(sorted(theirs[0]), sorted(&ours));
    }
}
