//! Behaviour-pack recipe documents (`minecraft:recipe_*`).
//!
//! Vanilla recipes come from bedrock-samples, embedded at build time
//! (`build.rs`, D13) and decoded at startup ([`decode_vanilla`]). The loader
//! applies the rules BDS applies when it loads recipe files (items research
//! §2.2):
//! - `tags` is required; each tag yields its own entry downstream;
//! - from `format_version` 1.20 on, `unlock` is required (the recipe is
//!   rejected without it) except for recipes tagged only `deprecated`, which
//!   BDS sends as always unlocked (18 vanilla furnace recipes, capture
//!   `CraftingDataPacket` 1.26.51); older documents are always unlocked;
//! - `assume_symmetry` defaults to true, `priority` to 0;
//! - one identifier namespace across recipe kinds.
//!
//! bedrock-samples flattens BDS's versioned pack layers into one directory, so
//! an identifier can be defined by several files; the copy with the newest
//! `format_version` (the newest layer's) wins, and between copies of the same
//! version the later file in path order wins (`barrel_deprecated.json` over
//! `barrel.json`: BDS 1.26.51 sends the `deprecated` copies of the five
//! barrel recipes). The BDS diff check (`tests/bds_pack_diff.rs`) resolves the
//! same way and finds no differing recipe.

use std::collections::{BTreeMap, HashMap};

use serde_json::Value;

use crate::item_components::FormatVersion;

include!(concat!(env!("OUT_DIR"), "/vanilla_recipes.rs"));

/// An ingredient descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ingredient {
    /// A named item; `data` `None` matches any aux value.
    Item {
        name: String,
        data: Option<i16>,
        count: u8,
    },
    /// Any item carrying the item tag.
    Tag { tag: String, count: u8 },
}

impl Ingredient {
    pub fn count(&self) -> u8 {
        match self {
            Self::Item { count, .. } | Self::Tag { count, .. } => *count,
        }
    }
}

/// A result item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultItem {
    pub name: String,
    pub data: i16,
    pub count: u8,
}

/// When the player unlocks a recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unlock {
    /// Documents older than 1.20 and `{"context": "AlwaysUnlocked"}`.
    Always,
    /// `{"context": <name>}` other than `AlwaysUnlocked`.
    Context(String),
    /// Holding any of these items.
    Items(Vec<Ingredient>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecipeKind {
    Shaped {
        /// Rows of the pattern, as written (equal widths after padding).
        pattern: Vec<String>,
        key: BTreeMap<char, Ingredient>,
        results: Vec<ResultItem>,
        assume_symmetry: bool,
    },
    Shapeless {
        ingredients: Vec<Ingredient>,
        results: Vec<ResultItem>,
    },
    Furnace {
        input: Ingredient,
        output: ResultItem,
    },
    /// Potion type to potion type (`minecraft:potion_type:<name>`).
    BrewingMix {
        input: String,
        reagent: String,
        output: String,
    },
    /// Potion item to potion item (potion → splash → lingering).
    BrewingContainer {
        input: String,
        reagent: String,
        output: String,
    },
    SmithingTransform {
        template: Ingredient,
        base: Ingredient,
        addition: Ingredient,
        result: ResultItem,
    },
    SmithingTrim {
        template: Ingredient,
        base: Ingredient,
        addition: Ingredient,
    },
}

/// One loaded recipe document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRecipe {
    pub origin: String,
    pub identifier: String,
    pub format_version: FormatVersion,
    pub tags: Vec<String>,
    pub priority: i32,
    pub group: Option<String>,
    pub unlock: Unlock,
    pub kind: RecipeKind,
}

/// A document BDS would reject.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{origin}: {message}")]
pub struct RecipeError {
    pub origin: String,
    pub message: String,
}

fn error(origin: &str, message: impl Into<String>) -> RecipeError {
    RecipeError {
        origin: origin.to_string(),
        message: message.into(),
    }
}

/// First format version that requires `unlock`.
const UNLOCK_REQUIRED: FormatVersion = FormatVersion([1, 20, 0]);

fn count(origin: &str, value: Option<&Value>, default: u8) -> Result<u8, RecipeError> {
    match value {
        None => Ok(default),
        Some(v) => v
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .ok_or_else(|| error(origin, format!("invalid count {v}"))),
    }
}

fn data(origin: &str, value: Option<&Value>) -> Result<Option<i16>, RecipeError> {
    value
        .map(|v| {
            v.as_i64()
                .and_then(|n| i16::try_from(n).ok())
                .ok_or_else(|| error(origin, format!("invalid data {v}")))
        })
        .transpose()
}

fn ingredient(origin: &str, value: &Value) -> Result<Ingredient, RecipeError> {
    match value {
        Value::String(name) => Ok(Ingredient::Item {
            name: name.clone(),
            data: None,
            count: 1,
        }),
        Value::Object(map) => {
            let count = count(origin, map.get("count"), 1)?;
            if let Some(tag) = map.get("tag").and_then(Value::as_str) {
                return Ok(Ingredient::Tag {
                    tag: tag.to_string(),
                    count,
                });
            }
            let name = map
                .get("item")
                .and_then(Value::as_str)
                .ok_or_else(|| error(origin, format!("ingredient without item or tag: {value}")))?;
            Ok(Ingredient::Item {
                name: name.to_string(),
                data: data(origin, map.get("data"))?,
                count,
            })
        }
        _ => Err(error(origin, format!("invalid ingredient {value}"))),
    }
}

fn result_item(origin: &str, value: &Value) -> Result<ResultItem, RecipeError> {
    match value {
        Value::String(name) => Ok(ResultItem {
            name: name.clone(),
            data: 0,
            count: 1,
        }),
        Value::Object(map) => Ok(ResultItem {
            name: map
                .get("item")
                .and_then(Value::as_str)
                .ok_or_else(|| error(origin, "result without item"))?
                .to_string(),
            data: data(origin, map.get("data"))?.unwrap_or(0),
            count: count(origin, map.get("count"), 1)?,
        }),
        _ => Err(error(origin, format!("invalid result {value}"))),
    }
}

fn results(origin: &str, value: Option<&Value>) -> Result<Vec<ResultItem>, RecipeError> {
    match value {
        Some(Value::Array(items)) => items.iter().map(|v| result_item(origin, v)).collect(),
        Some(v) => Ok(vec![result_item(origin, v)?]),
        None => Err(error(origin, "missing result")),
    }
}

fn string(origin: &str, body: &Value, key: &str) -> Result<String, RecipeError> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| error(origin, format!("missing {key}")))
}

fn unlock(
    origin: &str,
    version: FormatVersion,
    deprecated: bool,
    value: Option<&Value>,
) -> Result<Unlock, RecipeError> {
    match value {
        None if version < UNLOCK_REQUIRED || deprecated => Ok(Unlock::Always),
        None => Err(error(origin, "format_version 1.20+ recipe without unlock")),
        Some(Value::Object(map)) => match map.get("context").and_then(Value::as_str) {
            Some("AlwaysUnlocked") => Ok(Unlock::Always),
            Some(context) => Ok(Unlock::Context(context.to_string())),
            None => Err(error(origin, "unlock object without context")),
        },
        Some(Value::Array(items)) => Ok(Unlock::Items(
            items
                .iter()
                .map(|v| ingredient(origin, v))
                .collect::<Result<_, _>>()?,
        )),
        Some(other) => Err(error(origin, format!("invalid unlock {other}"))),
    }
}

/// Parse one recipe document.
pub fn parse_recipe_document(origin: &str, document: &Value) -> Result<PackRecipe, RecipeError> {
    let map = document
        .as_object()
        .ok_or_else(|| error(origin, "document is not an object"))?;
    let format_version = map
        .get("format_version")
        .and_then(Value::as_str)
        .and_then(FormatVersion::parse)
        .ok_or_else(|| error(origin, "missing or malformed format_version"))?;
    let mut schema = map.iter().filter(|(k, _)| *k != "format_version");
    let (Some((kind_name, body)), None) = (schema.next(), schema.next()) else {
        return Err(error(origin, "expected one recipe schema key"));
    };
    let identifier = body
        .pointer("/description/identifier")
        .and_then(Value::as_str)
        .ok_or_else(|| error(origin, "missing description.identifier"))?
        .to_string();
    let tags: Vec<String> = body
        .get("tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(|t| t.as_str().map(str::to_string))
                .collect()
        })
        .filter(|tags: &Vec<String>| !tags.is_empty())
        .ok_or_else(|| error(origin, "missing tags"))?;
    let priority = body
        .get("priority")
        .map(|p| {
            p.as_i64()
                .and_then(|p| i32::try_from(p).ok())
                .ok_or_else(|| error(origin, "invalid priority"))
        })
        .transpose()?
        .unwrap_or(0);
    let group = body
        .get("group")
        .and_then(Value::as_str)
        .map(str::to_string);
    let kind = match kind_name.as_str() {
        "minecraft:recipe_shaped" => {
            let pattern: Vec<String> = body
                .get("pattern")
                .and_then(Value::as_array)
                .ok_or_else(|| error(origin, "missing pattern"))?
                .iter()
                .map(|row| {
                    row.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| error(origin, "invalid pattern row"))
                })
                .collect::<Result<_, _>>()?;
            if pattern.is_empty()
                || pattern.len() > 3
                || pattern.iter().any(|row| row.chars().count() > 3)
            {
                return Err(error(
                    origin,
                    "pattern must be 1-3 rows of at most 3 symbols",
                ));
            }
            let mut key = BTreeMap::new();
            if let Some(map) = body.get("key").and_then(Value::as_object) {
                for (symbol, value) in map {
                    let mut chars = symbol.chars();
                    let (Some(c), None) = (chars.next(), chars.next()) else {
                        return Err(error(
                            origin,
                            format!("key symbol {symbol:?} is not one character"),
                        ));
                    };
                    key.insert(c, ingredient(origin, value)?);
                }
            }
            if let Some(missing) = pattern
                .iter()
                .flat_map(|row| row.chars())
                .find(|c| *c != ' ' && !key.contains_key(c))
            {
                return Err(error(
                    origin,
                    format!("pattern symbol {missing:?} has no key"),
                ));
            }
            RecipeKind::Shaped {
                pattern,
                key,
                results: results(origin, body.get("result"))?,
                assume_symmetry: body
                    .get("assume_symmetry")
                    .map(|v| {
                        v.as_bool()
                            .ok_or_else(|| error(origin, "invalid assume_symmetry"))
                    })
                    .transpose()?
                    .unwrap_or(true),
            }
        }
        "minecraft:recipe_shapeless" => RecipeKind::Shapeless {
            ingredients: body
                .get("ingredients")
                .and_then(Value::as_array)
                .ok_or_else(|| error(origin, "missing ingredients"))?
                .iter()
                .map(|v| ingredient(origin, v))
                .collect::<Result<_, _>>()?,
            results: results(origin, body.get("result"))?,
        },
        "minecraft:recipe_furnace" => RecipeKind::Furnace {
            input: ingredient(
                origin,
                body.get("input")
                    .ok_or_else(|| error(origin, "missing input"))?,
            )?,
            output: result_item(
                origin,
                body.get("output")
                    .ok_or_else(|| error(origin, "missing output"))?,
            )?,
        },
        "minecraft:recipe_brewing_mix" => RecipeKind::BrewingMix {
            input: string(origin, body, "input")?,
            reagent: string(origin, body, "reagent")?,
            output: string(origin, body, "output")?,
        },
        "minecraft:recipe_brewing_container" => RecipeKind::BrewingContainer {
            input: string(origin, body, "input")?,
            reagent: string(origin, body, "reagent")?,
            output: string(origin, body, "output")?,
        },
        "minecraft:recipe_smithing_transform" => {
            let part = |key: &str| {
                ingredient(
                    origin,
                    body.get(key)
                        .ok_or_else(|| error(origin, format!("missing {key}")))?,
                )
            };
            RecipeKind::SmithingTransform {
                template: part("template")?,
                base: part("base")?,
                addition: part("addition")?,
                result: result_item(
                    origin,
                    body.get("result")
                        .ok_or_else(|| error(origin, "missing result"))?,
                )?,
            }
        }
        "minecraft:recipe_smithing_trim" => {
            let part = |key: &str| {
                ingredient(
                    origin,
                    body.get(key)
                        .ok_or_else(|| error(origin, format!("missing {key}")))?,
                )
            };
            RecipeKind::SmithingTrim {
                template: part("template")?,
                base: part("base")?,
                addition: part("addition")?,
            }
        }
        other => return Err(error(origin, format!("unknown recipe type {other}"))),
    };
    // Brewing documents have no unlock in any vanilla version; BDS sends their
    // mixes without unlocking requirements.
    let unlock = match kind {
        RecipeKind::BrewingMix { .. }
        | RecipeKind::BrewingContainer { .. }
        | RecipeKind::SmithingTrim { .. }
        | RecipeKind::SmithingTransform { .. } => Unlock::Always,
        _ => {
            let deprecated = tags.iter().all(|tag| tag == "deprecated");
            unlock(origin, format_version, deprecated, body.get("unlock"))?
        }
    };
    Ok(PackRecipe {
        origin: origin.to_string(),
        identifier,
        format_version,
        tags,
        priority,
        group,
        unlock,
        kind,
    })
}

/// Resolve documents (in path order) to one recipe per identifier: the
/// newest format version wins, the later document among equal versions.
pub fn resolve(recipes: Vec<PackRecipe>) -> Vec<PackRecipe> {
    let mut by_id: BTreeMap<String, PackRecipe> = BTreeMap::new();
    for recipe in recipes {
        if by_id
            .get(&recipe.identifier)
            .is_some_and(|previous| previous.format_version > recipe.format_version)
        {
            continue;
        }
        by_id.insert(recipe.identifier.clone(), recipe);
    }
    by_id.into_values().collect()
}

/// Decode and resolve the embedded vanilla recipes, sorted by identifier.
pub fn decode_vanilla() -> Result<Vec<PackRecipe>, RecipeError> {
    let recipes = VANILLA_RECIPE_FILES
        .iter()
        .map(|(origin, json)| {
            let document: Value =
                serde_json::from_str(json).map_err(|e| error(origin, e.to_string()))?;
            parse_recipe_document(origin, &document)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(resolve(recipes))
}

/// Frozen recipe definitions and the furnace lookup used by smelting and loot.
///
/// Tag ingredients and legacy item names are resolved once at construction.
/// The hot lookup only borrows its arguments and result; it never parses pack
/// data, walks tags, or allocates.
#[derive(Debug)]
pub struct RecipeRegistry {
    recipes: Vec<PackRecipe>,
    by_identifier: HashMap<String, usize>,
    furnace: HashMap<String, FurnaceInputs>,
}

#[derive(Debug, Default)]
struct FurnaceInputs {
    wildcard: Option<usize>,
    exact: HashMap<i16, usize>,
}

#[derive(serde::Deserialize)]
struct LegacyIdentity {
    name: String,
    data: Option<i16>,
}

#[derive(serde::Deserialize)]
struct LegacyIdentities {
    inputs: HashMap<String, LegacyIdentity>,
    outputs: HashMap<String, LegacyIdentity>,
}

impl RecipeRegistry {
    /// Load the embedded vanilla recipe and native item families at startup.
    pub fn vanilla() -> Result<Self, RecipeError> {
        Self::from_recipes(decode_vanilla()?)
    }

    /// Compile recipes against the native vanilla item/tag registry.
    pub fn from_recipes(recipes: Vec<PackRecipe>) -> Result<Self, RecipeError> {
        Self::with_items(recipes, crate::items::vanilla())
    }

    /// Compile an addon-capable item and recipe snapshot before registry freeze.
    ///
    /// Unknown named items remain valid identities (the caller validates its
    /// full item registry). Tag ingredients expand against this snapshot.
    pub fn with_items(
        recipes: Vec<PackRecipe>,
        items: &[crate::items::ItemData],
    ) -> Result<Self, RecipeError> {
        let mut recipes = resolve(recipes);
        let legacy: LegacyIdentities = serde_json::from_str(include_str!(
            "../data/measured/recipe_legacy_identities.json"
        ))
        .map_err(|e| error("recipe_legacy_identities.json", e.to_string()))?;
        for recipe in &mut recipes {
            if let RecipeKind::Furnace { input, output } = &mut recipe.kind {
                if let Ingredient::Item { name, data, .. } = input
                    && let Some(identity) = legacy.inputs.get(name.as_str())
                {
                    name.clone_from(&identity.name);
                    if data.is_none() {
                        *data = identity.data;
                    }
                }
                if let Some(identity) = legacy.outputs.get(output.name.as_str()) {
                    output.name.clone_from(&identity.name);
                    output.data = identity.data.ok_or_else(|| {
                        error(&recipe.origin, "legacy result identity has no aux value")
                    })?;
                }
            }
        }
        let by_identifier = recipes
            .iter()
            .enumerate()
            .map(|(index, recipe)| (recipe.identifier.clone(), index))
            .collect();
        let mut furnace: HashMap<String, FurnaceInputs> = HashMap::new();
        // Lower numeric priority wins, then identifier order. Inserting the
        // first candidate preserves a deterministic tie without per-tick sorting.
        let mut order: Vec<_> = (0..recipes.len()).collect();
        order.sort_by_key(|&index| (recipes[index].priority, index));
        for index in order {
            let recipe = &recipes[index];
            if !recipe.tags.iter().any(|tag| tag == "furnace") {
                continue;
            }
            let RecipeKind::Furnace { input, .. } = &recipe.kind else {
                continue;
            };
            let mut insert = |name: &str, data: Option<i16>| {
                let slots = furnace.entry(name.to_string()).or_default();
                match data {
                    Some(data) => {
                        slots.exact.entry(data).or_insert(index);
                    }
                    None => {
                        slots.wildcard.get_or_insert(index);
                    }
                }
            };
            match input {
                Ingredient::Item { name, data, .. } => insert(name, *data),
                Ingredient::Tag { tag, .. } => {
                    let mut matched = false;
                    for item in items.iter().filter(|item| item.has_tag(tag)) {
                        insert(&item.identifier, None);
                        matched = true;
                    }
                    if !matched {
                        return Err(error(
                            &recipe.origin,
                            format!("unresolved furnace tag {tag}"),
                        ));
                    }
                }
            }
        }
        Ok(Self {
            recipes,
            by_identifier,
            furnace,
        })
    }

    pub fn recipes(&self) -> &[PackRecipe] {
        &self.recipes
    }

    pub fn get(&self, identifier: &str) -> Option<&PackRecipe> {
        self.by_identifier
            .get(identifier)
            .map(|&index| &self.recipes[index])
    }

    /// Look up a furnace result by canonical input identifier and aux value.
    ///
    /// `None` means no furnace recipe, so loot `furnace_smelt` leaves that item
    /// unchanged. Consumers preserve the source loot count, not this recipe's
    /// yield. Exact and wildcard candidates share the same priority ordering.
    pub fn furnace_result(&self, identifier: &str, data: i16) -> Option<&ResultItem> {
        let slots = self.furnace.get(identifier)?;
        let exact = slots.exact.get(&data).copied();
        let index = match (exact, slots.wildcard) {
            (Some(a), Some(b)) => {
                if (self.recipes[a].priority, a) <= (self.recipes[b].priority, b) {
                    a
                } else {
                    b
                }
            }
            (Some(index), None) | (None, Some(index)) => index,
            (None, None) => return None,
        };
        match &self.recipes[index].kind {
            RecipeKind::Furnace { output, .. } => Some(output),
            _ => unreachable!("furnace index only contains furnace recipes"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn shaped(version: &str, extra: Value) -> Value {
        let mut body = json!({
            "description": {"identifier": "minecraft:test"},
            "tags": ["crafting_table"],
            "pattern": ["#", "#"],
            "key": {"#": {"item": "minecraft:planks", "data": 2}},
            "result": {"item": "minecraft:stick", "count": 4}
        });
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        json!({"format_version": version, "minecraft:recipe_shaped": body})
    }

    #[test]
    fn unlock_is_required_from_1_20() {
        let old = parse_recipe_document("old.json", &shaped("1.12", json!({}))).unwrap();
        assert_eq!(old.unlock, Unlock::Always);
        let RecipeKind::Shaped {
            assume_symmetry,
            key,
            results,
            ..
        } = old.kind
        else {
            panic!("shaped")
        };
        assert!(assume_symmetry);
        assert_eq!(
            key[&'#'],
            Ingredient::Item {
                name: "minecraft:planks".into(),
                data: Some(2),
                count: 1
            }
        );
        assert_eq!(results[0].count, 4);
        assert!(parse_recipe_document("new.json", &shaped("1.20.10", json!({}))).is_err());
        let new = parse_recipe_document(
            "new.json",
            &shaped(
                "1.20.10",
                json!({"unlock": {"context": "PlayerInWater"}, "assume_symmetry": false}),
            ),
        )
        .unwrap();
        assert_eq!(new.unlock, Unlock::Context("PlayerInWater".into()));
        assert!(
            parse_recipe_document("tagless.json", &shaped("1.12", json!({"tags": []}))).is_err()
        );
        let deprecated = parse_recipe_document(
            "dep.json",
            &shaped("1.20.10", json!({"tags": ["deprecated"]})),
        )
        .unwrap();
        assert_eq!(deprecated.unlock, Unlock::Always);
        assert!(
            parse_recipe_document("key.json", &shaped("1.12", json!({"pattern": ["#X"]}))).is_err()
        );
    }

    #[test]
    fn the_newest_format_version_then_the_later_file_wins() {
        let a = parse_recipe_document("a.json", &shaped("1.12", json!({}))).unwrap();
        let b = parse_recipe_document(
            "b.json",
            &shaped("1.20.10", json!({"unlock": [{"item": "minecraft:planks"}]})),
        )
        .unwrap();
        assert_eq!(resolve(vec![b.clone(), a.clone()]), vec![b.clone()]);
        assert_eq!(resolve(vec![a.clone(), b.clone()]), vec![b]);
        let mut later = a.clone();
        later.tags = vec!["deprecated".into()];
        assert_eq!(resolve(vec![a, later.clone()]), vec![later]);
    }

    #[test]
    fn furnace_lookup_uses_pack_outputs_and_native_tag_expansion() {
        let registry = RecipeRegistry::vanilla().unwrap();
        for (input, output) in [
            ("minecraft:beef", "minecraft:cooked_beef"),
            ("minecraft:chicken", "minecraft:cooked_chicken"),
            ("minecraft:cod", "minecraft:cooked_cod"),
            ("minecraft:mutton", "minecraft:cooked_mutton"),
            ("minecraft:oak_log", "minecraft:charcoal"),
        ] {
            assert_eq!(
                registry.furnace_result(input, 0),
                Some(&ResultItem {
                    name: output.into(),
                    data: 0,
                    count: 1
                }),
                "{input}",
            );
        }
        assert!(registry.furnace_result("minecraft:diamond", 0).is_none());
        assert!(registry.furnace_result("minecraft:sand", 1).is_none());
    }

    fn furnace(id: &str, input: Value, output: &str, priority: i32) -> PackRecipe {
        parse_recipe_document(
            id,
            &json!({
                "format_version": "1.12",
                "minecraft:recipe_furnace": {
                    "description": {"identifier": id},
                    "tags": ["furnace"],
                    "input": input,
                    "output": output,
                    "priority": priority
                }
            }),
        )
        .unwrap()
    }

    #[test]
    fn furnace_exact_and_wildcard_candidates_follow_priority_not_insertion_order() {
        let wildcard = furnace("addon:any", json!("addon:input"), "addon:any_result", 5);
        let exact = furnace(
            "addon:exact",
            json!({"item": "addon:input", "data": 2}),
            "addon:exact_result",
            1,
        );
        for rows in [
            vec![wildcard.clone(), exact.clone()],
            vec![exact.clone(), wildcard.clone()],
        ] {
            let registry = RecipeRegistry::from_recipes(rows).unwrap();
            assert_eq!(
                registry.furnace_result("addon:input", 2).unwrap().name,
                "addon:exact_result"
            );
            assert_eq!(
                registry.furnace_result("addon:input", 3).unwrap().name,
                "addon:any_result"
            );
        }
        let mut higher = wildcard;
        higher.priority = -1;
        let registry = RecipeRegistry::from_recipes(vec![exact, higher]).unwrap();
        assert_eq!(
            registry.furnace_result("addon:input", 2).unwrap().name,
            "addon:any_result"
        );
        let missing_tag = furnace(
            "addon:bad",
            json!({"tag": "addon:missing"}),
            "addon:output",
            0,
        );
        assert!(RecipeRegistry::from_recipes(vec![missing_tag]).is_err());
    }
}
