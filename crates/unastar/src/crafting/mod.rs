//! Frozen recipe catalog: one source of network ids for advertisement/execution.

pub mod execution;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use jolyne::valentine::{
    CerealizerNetworkItemInstanceDescriptorSerializedData as WireResult,
    CerealizerRecipeIngredientSerializedData as WireIngredient,
    CerealizerRecipeIngredientSerializedDataDescriptorItem as Descriptor,
    CerealizerRecipeUnlockingRequirementSerializedData as WireUnlock, ContainerMixDataEntry,
    CraftingDataPacket, EnumsRecipeUnlockingRequirementUnlockingContext as UnlockContext,
    MultiRecipePayload, PotionMixDataEntry, ShapedRecipePayload, ShapelessRecipePayload,
    SmithingTransformRecipePayload, SmithingTrimRecipePayload,
    TypedServerNetIdstructRecipeNetIdTag as RecipeNetId,
};
use unastar_data::recipes::native::{
    self, BuiltinIngredient, BuiltinResult, BuiltinShapeless, DescriptorKind,
};
use unastar_data::recipes::{Ingredient, RecipeKind, RecipeRegistry, ResultItem, Unlock};

use crate::item::user_data::network_user_data;
use crate::registry::{BlockRegistry, ItemRegistry};

/// Index into the same frozen source snapshot used to construct the packet.
#[derive(Debug, Clone, Copy)]
pub enum RecipeOrigin {
    Pack(usize),
    NativeShaped(usize),
    NativeShapeless(usize),
    UserData(usize),
    Multi(usize),
}

pub struct CraftingCatalog {
    packet: Arc<CraftingDataPacket>,
    origins: Vec<RecipeOrigin>,
}

impl CraftingCatalog {
    pub fn new(
        recipes: &RecipeRegistry,
        items: &ItemRegistry,
        blocks: &BlockRegistry,
    ) -> Result<Self> {
        let measured = native::builtins();
        let mut packet = CraftingDataPacket {
            clear_recipes: true,
            ..Default::default()
        };
        let mut origins = Vec::new();
        let mut net_id = |origin| {
            origins.push(origin);
            RecipeNetId {
                raw_id: origins.len() as u32,
            }
        };
        for (index, recipe) in recipes.recipes().iter().enumerate() {
            for tag in &recipe.tags {
                let id = recipe.identifier.as_str().into();
                let tag_wire = tag.as_str().into();
                match &recipe.kind {
                    RecipeKind::Shaped {
                        pattern,
                        key,
                        results,
                        assume_symmetry,
                    } => {
                        let width = pattern
                            .iter()
                            .map(|row| row.chars().count())
                            .max()
                            .unwrap_or(0);
                        let mut ingredients = Vec::with_capacity(width * pattern.len());
                        for row in pattern {
                            let mut symbols = row.chars();
                            for _ in 0..width {
                                let symbol = symbols.next().unwrap_or(' ');
                                ingredients.push(
                                    match key.get(&symbol).filter(|_| symbol != ' ') {
                                        Some(ingredient) => wire_ingredient(ingredient),
                                        None => empty_ingredient(),
                                    },
                                );
                            }
                        }
                        packet.shaped_recipes.push(ShapedRecipePayload {
                            recipe_id: id,
                            width: width as i32,
                            height: pattern.len() as i32,
                            ingredients,
                            results: wire_results(&recipe.identifier, results, items, blocks)?,
                            uuid: uuid::Uuid::new_v4(),
                            tag: tag_wire,
                            priority: recipe.priority,
                            assume_symmetry: *assume_symmetry,
                            unlocking_requirement: Some(wire_unlock(&recipe.unlock)?),
                            net_id: net_id(RecipeOrigin::Pack(index)),
                        });
                    }
                    RecipeKind::Shapeless {
                        ingredients,
                        results,
                    } => {
                        // Counted shapeless inputs occupy independent cells in BDS.
                        let ingredients = ingredients
                            .iter()
                            .flat_map(|ingredient| {
                                (0..ingredient.count()).map(move |_| {
                                    let mut wire = wire_ingredient(ingredient);
                                    wire.stack_size = 1;
                                    wire
                                })
                            })
                            .collect();
                        packet.shapeless_recipes.push(ShapelessRecipePayload {
                            recipe_id: id,
                            ingredients,
                            results: wire_results(&recipe.identifier, results, items, blocks)?,
                            uuid: uuid::Uuid::new_v4(),
                            tag: tag_wire,
                            priority: recipe.priority,
                            unlocking_requirement: Some(wire_unlock(&recipe.unlock)?),
                            net_id: net_id(RecipeOrigin::Pack(index)),
                        });
                    }
                    RecipeKind::Furnace { input, output } => {
                        packet.shapeless_recipes.push(ShapelessRecipePayload {
                            recipe_id: id,
                            ingredients: vec![wire_ingredient(input)],
                            results: wire_results(
                                &recipe.identifier,
                                std::slice::from_ref(output),
                                items,
                                blocks,
                            )?,
                            uuid: uuid::Uuid::new_v4(),
                            tag: tag_wire,
                            priority: recipe.priority,
                            unlocking_requirement: Some(wire_unlock(&recipe.unlock)?),
                            net_id: net_id(RecipeOrigin::Pack(index)),
                        });
                    }
                    RecipeKind::SmithingTransform {
                        template,
                        base,
                        addition,
                        result,
                    } => {
                        packet
                            .smithing_transform_recipes
                            .push(SmithingTransformRecipePayload {
                                recipe_id: id,
                                template_ingredient: wire_ingredient(template),
                                base_ingredient: wire_ingredient(base),
                                addition_ingredient: wire_ingredient(addition),
                                result: wire_result(result, items, blocks)?,
                                tag: tag_wire,
                                net_id: net_id(RecipeOrigin::Pack(index)),
                            });
                    }
                    RecipeKind::SmithingTrim {
                        template,
                        base,
                        addition,
                    } => {
                        packet
                            .smithing_trim_recipes
                            .push(SmithingTrimRecipePayload {
                                recipe_id: id,
                                template_ingredient: wire_ingredient(template),
                                base_ingredient: wire_ingredient(base),
                                addition_ingredient: wire_ingredient(addition),
                                tag: tag_wire,
                                net_id: net_id(RecipeOrigin::Pack(index)),
                            });
                    }
                    RecipeKind::BrewingMix {
                        input,
                        reagent,
                        output,
                    } => {
                        let from_item_aux = potion_aux(input)?;
                        let to_item_aux = potion_aux(output)?;
                        let reagent_item_id = item_id(items, reagent)?;
                        for container in [
                            "minecraft:potion",
                            "minecraft:splash_potion",
                            "minecraft:lingering_potion",
                        ] {
                            let container = item_id(items, container)?;
                            packet.potion_mixes.push(PotionMixDataEntry {
                                from_potion_id: container,
                                from_item_aux,
                                reagent_item_id,
                                reagent_item_aux: 0,
                                to_potion_id: container,
                                to_item_aux,
                            });
                        }
                    }
                    RecipeKind::BrewingContainer {
                        input,
                        reagent,
                        output,
                    } => {
                        packet.container_mixes.push(ContainerMixDataEntry {
                            from_item_id: item_id(items, input)?,
                            reagent_item_id: item_id(items, reagent)?,
                            to_item_id: item_id(items, output)?,
                        });
                    }
                }
            }
        }
        // Unlike independent station-tag rows, native variants share UUID groups.
        let mut groups = HashMap::new();
        for (index, native) in measured.shaped.iter().enumerate() {
            let recipe = native_shapeless(&native.recipe, items, &mut groups)?;
            packet.shaped_recipes.push(ShapedRecipePayload {
                recipe_id: recipe.recipe_id,
                width: i32::from(native.width),
                height: i32::from(native.height),
                ingredients: recipe.ingredients,
                results: recipe.results,
                uuid: recipe.uuid,
                tag: recipe.tag,
                priority: recipe.priority,
                assume_symmetry: native.assume_symmetry,
                unlocking_requirement: recipe.unlocking_requirement,
                net_id: net_id(RecipeOrigin::NativeShaped(index)),
            });
        }
        for (index, native) in measured.shapeless.iter().enumerate() {
            let mut recipe = native_shapeless(native, items, &mut groups)?;
            recipe.net_id = net_id(RecipeOrigin::NativeShapeless(index));
            packet.shapeless_recipes.push(recipe);
        }
        for (index, native) in measured.user_data_shapeless.iter().enumerate() {
            let mut recipe = native_shapeless(native, items, &mut groups)?;
            recipe.net_id = net_id(RecipeOrigin::UserData(index));
            packet.user_data_shapeless_recipes.push(recipe);
        }
        for (index, uuid) in measured.multi.iter().enumerate() {
            packet.multi_recipes.push(MultiRecipePayload {
                multi_recipe_uuid: uuid.parse().context("measured multi-recipe UUID")?,
                net_id: net_id(RecipeOrigin::Multi(index)),
            });
        }
        Ok(Self {
            packet: Arc::new(packet),
            origins,
        })
    }

    pub fn packet(&self) -> &Arc<CraftingDataPacket> {
        &self.packet
    }

    /// The advertisement in `ids`' block numbering. The catalog itself keeps
    /// canonical palette ordinals, which the executor compiles from.
    pub fn wire_packet(&self, ids: crate::registry::block::BlockNetworkIds) -> CraftingDataPacket {
        let mut packet = self.packet.as_ref().clone();
        // Item instance descriptors carry the block id as i32; hashes use all 32 bits.
        let translate = |result: &mut WireResult| {
            result.block_runtime_id =
                ids.item_block_to_wire(result.id, result.block_runtime_id as u32) as i32;
        };
        for recipe in packet
            .shaped_recipes
            .iter_mut()
            .chain(&mut packet.shaped_chemistry_recipes)
        {
            recipe.results.iter_mut().for_each(translate);
        }
        for recipe in packet
            .shapeless_recipes
            .iter_mut()
            .chain(&mut packet.user_data_shapeless_recipes)
            .chain(&mut packet.shapeless_chemistry_recipes)
        {
            recipe.results.iter_mut().for_each(translate);
        }
        for recipe in &mut packet.smithing_transform_recipes {
            translate(&mut recipe.result);
        }
        packet
    }

    pub fn origin(&self, net_id: u32) -> Option<RecipeOrigin> {
        self.origins.get(net_id.checked_sub(1)? as usize).copied()
    }
}

fn item_id(items: &ItemRegistry, name: &str) -> Result<i32> {
    items
        .get_by_name(name)
        .map(|item| item.network_id())
        .with_context(|| format!("recipe references unknown item {name}"))
}

fn potion_aux(name: &str) -> Result<i32> {
    native::potions()
        .types
        .get(&name.to_ascii_lowercase())
        .copied()
        .map(i32::from)
        .with_context(|| format!("recipe references unmeasured potion type {name}"))
}

fn empty_ingredient() -> WireIngredient {
    WireIngredient {
        descriptor: Vec::new(),
        aux_value: 32767,
        stack_size: 0,
    }
}

fn wire_ingredient(ingredient: &Ingredient) -> WireIngredient {
    let (key, value, aux, count) = match ingredient {
        Ingredient::Item { name, data, count } => {
            ("name", name.as_str(), data.map_or(32767, i32::from), *count)
        }
        Ingredient::Tag { tag, count } => ("item_tag", tag.as_str(), 32767, *count),
    };
    WireIngredient {
        descriptor: vec![Descriptor {
            key: key.into(),
            value: value.into(),
        }],
        aux_value: aux,
        stack_size: i32::from(count),
    }
}

fn native_ingredient(ingredient: &BuiltinIngredient) -> WireIngredient {
    let key = match ingredient.0 {
        DescriptorKind::Empty => return empty_ingredient(),
        DescriptorKind::Item => "name",
        DescriptorKind::Tag => "item_tag",
    };
    WireIngredient {
        descriptor: vec![Descriptor {
            key: key.into(),
            value: ingredient.1.as_str().into(),
        }],
        aux_value: i32::from(ingredient.2),
        stack_size: i32::from(ingredient.3),
    }
}

fn wire_unlock(unlock: &Unlock) -> Result<WireUnlock> {
    let (unlocking_context, unlocking_ingredients) = match unlock {
        Unlock::Always => (UnlockContext::Alwaysunlocked, None),
        Unlock::Items(items) => (
            UnlockContext::None,
            Some(items.iter().map(wire_ingredient).collect()),
        ),
        Unlock::Context(context) => (
            match context.as_str() {
                "PlayerInWater" => UnlockContext::Playerinwater,
                "PlayerHasManyItems" => UnlockContext::Playerhasmanyitems,
                _ => bail!("unknown recipe unlocking context {context}"),
            },
            None,
        ),
    };
    Ok(WireUnlock {
        unlocking_context,
        unlocking_ingredients,
    })
}

fn wire_results(
    origin: &str,
    results: &[ResultItem],
    items: &ItemRegistry,
    blocks: &BlockRegistry,
) -> Result<Vec<WireResult>> {
    let metadata = native::builtins().result_user_data.get(origin);
    results
        .iter()
        .map(|result| {
            let mut wire = wire_result(result, items, blocks)?;
            // Constructor metadata may enrich a matching result, never replace the
            // authoritative loaded identity, count or number of result stacks.
            if let Some(data) = metadata
                .and_then(|rows| {
                    rows.iter()
                        .find(|row| row.0 == result.name && row.1 == result.data)
                })
                .and_then(|row| row.4.as_ref())
            {
                wire.user_data_buffer = bytes::Bytes::from_static(data);
            }
            Ok(wire)
        })
        .collect()
}

fn wire_result(
    result: &ResultItem,
    items: &ItemRegistry,
    blocks: &BlockRegistry,
) -> Result<WireResult> {
    let stack = items
        .stack_with_block(&result.name, result.count, blocks)
        .with_context(|| format!("recipe references unknown item {}", result.name))?
        .with_damage(result.data);
    let key = format!("{}:{}", result.name, result.data);
    let block_runtime_id = match native::builtins().result_block_states.get(&key) {
        Some(measured) => measured.map_or(0, |state| state as i32),
        None => stack.runtime_id,
    };
    Ok(WireResult {
        id: stack.item().expect("registered result type").network_id(),
        auxvalue: result.data as u32,
        stacksize: u16::from(result.count),
        block_runtime_id,
        user_data_buffer: stack.user_data(),
    })
}

fn native_result(result: &BuiltinResult, items: &ItemRegistry) -> Result<WireResult> {
    Ok(WireResult {
        id: item_id(items, &result.0)?,
        auxvalue: result.1 as u32,
        stacksize: u16::from(result.2),
        block_runtime_id: result.3.map_or(0, |state| state as i32),
        user_data_buffer: network_user_data(&result.0, result.4.as_deref()),
    })
}

fn native_shapeless<'a>(
    native: &'a BuiltinShapeless,
    items: &ItemRegistry,
    groups: &mut HashMap<&'a str, uuid::Uuid>,
) -> Result<ShapelessRecipePayload> {
    Ok(ShapelessRecipePayload {
        recipe_id: native.identifier.as_str().into(),
        ingredients: native.ingredients.iter().map(native_ingredient).collect(),
        results: native
            .results
            .iter()
            .map(|result| native_result(result, items))
            .collect::<Result<_>>()?,
        uuid: *groups
            .entry(native.group.as_str())
            .or_insert_with(uuid::Uuid::new_v4),
        tag: "crafting_table".into(),
        priority: native.priority,
        unlocking_requirement: Some(WireUnlock {
            unlocking_context: UnlockContext::None,
            unlocking_ingredients: Some(native.unlock.iter().map(native_ingredient).collect()),
        }),
        net_id: RecipeNetId::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::LazyLock;

    fn catalog() -> &'static CraftingCatalog {
        static SNAPSHOT: LazyLock<(RecipeRegistry, CraftingCatalog)> = LazyLock::new(|| {
            let mut items = ItemRegistry::new();
            items.load_vanilla();
            let blocks = BlockRegistry::vanilla();
            let recipes = RecipeRegistry::vanilla().unwrap();
            let catalog = CraftingCatalog::new(&recipes, &items, blocks).unwrap();
            (recipes, catalog)
        });
        &SNAPSHOT.1
    }

    #[test]
    fn counted_shapeless_inputs_occupy_separate_crafting_cells() {
        let soup = catalog()
            .packet
            .shapeless_recipes
            .iter()
            .find(|recipe| recipe.recipe_id == "minecraft:beetroot_soup")
            .unwrap();
        assert_eq!(soup.ingredients.len(), 7);
        assert!(
            soup.ingredients
                .iter()
                .all(|ingredient| ingredient.stack_size == 1)
        );
        let names: Vec<&str> = soup
            .ingredients
            .iter()
            .map(|ingredient| ingredient.descriptor[0].value.as_ref())
            .collect();
        assert_eq!(
            names,
            [
                "minecraft:bowl",
                "minecraft:beetroot",
                "minecraft:beetroot",
                "minecraft:beetroot",
                "minecraft:beetroot",
                "minecraft:beetroot",
                "minecraft:beetroot",
            ]
        );
    }

    #[test]
    fn banner_result_carries_the_native_type_compound() {
        let banner = catalog()
            .packet
            .shaped_recipes
            .iter()
            .find(|recipe| recipe.recipe_id == "minecraft:black_banner")
            .unwrap();
        // BDS capture: NBT Type=0, followed by empty place/destroy lists.
        assert_eq!(
            banner.results[0].user_data_buffer.as_ref(),
            [
                255, 255, 1, 10, 0, 0, 3, 4, 0, 84, 121, 112, 101, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0,
            ]
        );
    }

    #[test]
    fn legacy_and_deprecated_furnace_unlock_requirements_differ_on_wire() {
        let packet = &catalog().packet;
        let barrel = packet
            .shaped_recipes
            .iter()
            .find(|recipe| recipe.recipe_id == "minecraft:barrel")
            .unwrap();
        let old = barrel.unlocking_requirement.as_ref().unwrap();
        assert_eq!(old.unlocking_context, UnlockContext::None);
        assert_eq!(old.unlocking_ingredients, Some(Vec::new()));
        let furnace = packet
            .shapeless_recipes
            .iter()
            .find(|recipe| {
                recipe.recipe_id == "minecraft:furnace_log_oak" && recipe.tag == "deprecated"
            })
            .unwrap();
        let deprecated = furnace.unlocking_requirement.as_ref().unwrap();
        assert_eq!(deprecated.unlocking_context, UnlockContext::Alwaysunlocked);
        assert_eq!(deprecated.unlocking_ingredients, None);
    }

    #[test]
    fn a_recipe_at_two_stations_has_independent_wire_uuids() {
        let rows = &catalog().packet.shapeless_recipes;
        let furnace = rows
            .iter()
            .find(|recipe| recipe.recipe_id == "minecraft:furnace_beef" && recipe.tag == "furnace")
            .unwrap();
        let smoker = rows
            .iter()
            .find(|recipe| recipe.recipe_id == "minecraft:furnace_beef" && recipe.tag == "smoker")
            .unwrap();
        assert_ne!(furnace.uuid, smoker.uuid);
    }
}

#[cfg(test)]
mod authority_tests {
    use super::*;
    use jolyne::valentine::bedrock::codec::BedrockCodec;
    use unastar_data::recipes::parse_recipe_document;

    fn advertised(recipes: RecipeRegistry) -> CraftingDataPacket {
        let mut items = ItemRegistry::new();
        items.load_vanilla();
        let blocks = BlockRegistry::vanilla();
        let catalog = CraftingCatalog::new(&recipes, &items, blocks).unwrap();
        let mut bytes = Vec::new();
        catalog.packet().encode(&mut bytes).unwrap();
        let mut input = bytes.as_slice();
        let packet = CraftingDataPacket::decode(&mut input, ()).unwrap();
        assert!(input.is_empty());
        packet
    }

    #[test]
    fn measured_banner_metadata_cannot_replace_loaded_results_or_their_counts() {
        let mut recipes = unastar_data::recipes::decode_vanilla().unwrap();
        let banner = recipes
            .iter_mut()
            .find(|recipe| recipe.identifier == "minecraft:black_banner")
            .unwrap();
        let RecipeKind::Shaped { results, .. } = &mut banner.kind else {
            panic!("shaped banner recipe");
        };
        *results = vec![
            ResultItem {
                name: "minecraft:banner".into(),
                data: 0,
                count: 2,
            },
            ResultItem {
                name: "minecraft:egg".into(),
                data: 0,
                count: 3,
            },
        ];
        let packet = advertised(RecipeRegistry::from_recipes(recipes).unwrap());
        let banner = packet
            .shaped_recipes
            .iter()
            .find(|recipe| recipe.recipe_id == "minecraft:black_banner")
            .unwrap();
        assert_eq!(
            banner
                .results
                .iter()
                .map(|result| (
                    result.id,
                    result.stacksize,
                    result.auxvalue,
                    result.block_runtime_id
                ))
                .collect::<Vec<_>>(),
            [(577, 2, 0, 0), (393, 3, 0, 0)]
        );
        assert_eq!(
            banner.results[0].user_data_buffer.as_ref(),
            [
                255, 255, 1, 10, 0, 0, 3, 4, 0, 84, 121, 112, 101, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0,
            ]
        );
        // Egg construction is independently present in the pinned native
        // CreativeContent capture, but absent from its CraftingData results.
        assert_eq!(banner.results[1].user_data_buffer.as_ref(), [0; 10]);
    }

    #[test]
    fn uncaptured_outputs_use_shared_block_and_nonblock_constructors() {
        let recipe = parse_recipe_document(
            "addon.json",
            &serde_json::json!({
                "format_version": "1.26.50",
                "minecraft:recipe_shapeless": {
                    "description": {"identifier": "addon:constructor_outputs"},
                    "tags": ["crafting_table"],
                    "ingredients": [{"item": "minecraft:stone"}],
                    "unlock": [{"item": "minecraft:stone"}],
                    "result": [
                        {"item": "minecraft:egg", "count": 4},
                        {"item": "minecraft:barrier", "count": 5}
                    ]
                }
            }),
        )
        .unwrap();
        let packet = advertised(RecipeRegistry::from_recipes(vec![recipe]).unwrap());
        let recipe = packet
            .shapeless_recipes
            .iter()
            .find(|recipe| recipe.recipe_id == "addon:constructor_outputs")
            .unwrap();
        assert_eq!(recipe.results[0].id, 393);
        assert_eq!(recipe.results[0].stacksize, 4);
        assert_eq!(recipe.results[0].block_runtime_id, 0);
        assert_eq!(recipe.results[0].user_data_buffer.as_ref(), [0; 10]);
        let blocks = BlockRegistry::vanilla();
        let state = blocks
            .default_state_id_by_name("minecraft:barrier")
            .unwrap() as i32;
        assert_ne!(state, 0);
        assert_eq!(recipe.results[1].id, -161);
        assert_eq!(recipe.results[1].stacksize, 5);
        assert_eq!(recipe.results[1].block_runtime_id, state);
    }
}
