//! Native recipe facts absent from pack JSON, projected from a pinned BDS packet.
//!
//! `tools/extract_recipe_identities.py` reproduces the data without retaining
//! transient network recipe ids or random per-run UUIDs. Captured block hashes
//! are translated to the native canonical state ordinal at extraction time.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, Deserialize)]
pub enum DescriptorKind {
    #[serde(rename = "")]
    Empty,
    #[serde(rename = "name")]
    Item,
    #[serde(rename = "item_tag")]
    Tag,
}

/// Descriptor kind/value, aux (32767 means any), and ingredient count.
#[derive(Debug, Deserialize)]
pub struct BuiltinIngredient(pub DescriptorKind, pub String, pub i16, pub u8);

/// Canonical item, aux, count, optional canonical block state and raw user data.
#[derive(Debug, Deserialize)]
pub struct BuiltinResult(
    pub String,
    pub i16,
    pub u8,
    pub Option<u32>,
    #[serde(deserialize_with = "decode_user_data")] pub Option<Vec<u8>>,
);

fn decode_user_data<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<Vec<u8>>, D::Error> {
    use base64::Engine;
    Option::<String>::deserialize(deserializer)?
        .map(|encoded| {
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .map_err(serde::de::Error::custom)
        })
        .transpose()
}

#[derive(Debug, Deserialize)]
pub struct BuiltinShapeless {
    pub identifier: String,
    /// Recipes sharing one native UUID share this stable group identity.
    pub group: String,
    pub ingredients: Vec<BuiltinIngredient>,
    pub results: Vec<BuiltinResult>,
    pub unlock: Vec<BuiltinIngredient>,
    pub priority: i32,
}

#[derive(Debug, Deserialize)]
pub struct BuiltinShaped {
    pub recipe: BuiltinShapeless,
    pub width: u8,
    pub height: u8,
    pub assume_symmetry: bool,
}

#[derive(Debug, Deserialize)]
pub struct BuiltinRecipes {
    pub source_sha256: String,
    pub shaped: Vec<BuiltinShaped>,
    pub shapeless: Vec<BuiltinShapeless>,
    pub user_data_shapeless: Vec<BuiltinShapeless>,
    /// Multi-recipe UUIDs are stable protocol identities, unlike normal recipes.
    pub multi: Vec<String>,
    /// Canonical `item:aux` result identity to its measured default block state.
    pub result_block_states: BTreeMap<String, Option<u32>>,
    /// Pack recipe results whose native item constructor contributes user data.
    pub result_user_data: BTreeMap<String, Vec<BuiltinResult>>,
}

#[derive(Debug, Deserialize)]
pub struct PotionTypes {
    pub source_sha256: String,
    /// Lowercase pack potion type identifier to its measured aux value.
    pub types: BTreeMap<String, i16>,
}

/// Decoded once while the server builds its CraftingData snapshot.
pub fn builtins() -> &'static BuiltinRecipes {
    static DATA: LazyLock<BuiltinRecipes> = LazyLock::new(|| {
        serde_json::from_str(include_str!("../../data/measured/builtin_recipes.json"))
            .expect("embedded measured native recipes decode")
    });
    &DATA
}

/// Aux identities uniquely determined by the captured, reagent-labelled graph.
pub fn potions() -> &'static PotionTypes {
    static DATA: LazyLock<PotionTypes> = LazyLock::new(|| {
        serde_json::from_str(include_str!("../../data/measured/potion_types.json"))
            .expect("embedded measured potion types decode")
    });
    &DATA
}
