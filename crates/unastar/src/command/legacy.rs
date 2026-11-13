//! Pre-flattening names commands still accept: BDS 1.26.51 keeps legacy
//! block names (`wool`) in the `Block` enum with their old states
//! (`wool ["color"="red"]` places `minecraft:red_wool`), and `/give` reads
//! data values of flattened item families (`/give @s wool 1 14` gives red
//! wool, `/give @s log 1 2` birch log) — both measured.
//!
//! Only the families below are mapped; other legacy names keep answering a
//! literal "not implemented" failure.

use super::parser::StateLiteral;

/// The sixteen colours in data-value order; the old `color` state spells
/// light grey `silver`.
const COLORS: [&str; 16] = [
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

/// Legacy names flattened by colour: the legacy name and the suffix of the
/// current identifier (`<color>_<suffix>`).
const COLOR_FAMILIES: [(&str, &str); 9] = [
    ("wool", "wool"),
    ("carpet", "carpet"),
    ("concrete", "concrete"),
    ("concrete_powder", "concrete_powder"),
    ("concretepowder", "concrete_powder"),
    ("stained_glass", "stained_glass"),
    ("stained_glass_pane", "stained_glass_pane"),
    ("stained_hardened_clay", "terracotta"),
    ("shulker_box", "shulker_box"),
];

/// Item families flattened by data value: legacy name and the current
/// identifiers by data value.
const DATA_FAMILIES: [(&str, &[&str]); 3] = [
    ("log", &["oak_log", "spruce_log", "birch_log", "jungle_log"]),
    ("log2", &["acacia_log", "dark_oak_log"]),
    (
        "planks",
        &[
            "oak_planks",
            "spruce_planks",
            "birch_planks",
            "jungle_planks",
            "acacia_planks",
            "dark_oak_planks",
        ],
    ),
];

fn short(name: &str) -> &str {
    name.strip_prefix("minecraft:").unwrap_or(name)
}

fn color_suffix(name: &str) -> Option<&'static str> {
    COLOR_FAMILIES
        .iter()
        .find(|(legacy, _)| *legacy == short(name))
        .map(|(_, suffix)| *suffix)
}

/// The current block a legacy block name and its old states stand for, and
/// the states left for the current block. `None`: not a mapped legacy name,
/// or an old state value the family does not have.
pub fn block(
    name: &str,
    states: &[(String, StateLiteral)],
) -> Option<(String, Vec<(String, StateLiteral)>)> {
    let suffix = color_suffix(name)?;
    let mut color = "white";
    let mut rest = Vec::new();
    for (state, value) in states {
        match (state.as_str(), value) {
            ("color", StateLiteral::String(value)) => {
                color = match value.as_str() {
                    "silver" => "light_gray",
                    other => COLORS.iter().find(|c| **c == other)?,
                };
            }
            _ => rest.push((state.clone(), value.clone())),
        }
    }
    Some((format!("minecraft:{color}_{suffix}"), rest))
}

/// The current item a legacy item name and data value stand for.
pub fn item(name: &str, data: i32) -> Option<String> {
    let index = usize::try_from(data).ok()?;
    if let Some(suffix) = color_suffix(name) {
        return COLORS
            .get(index)
            .map(|color| format!("minecraft:{color}_{suffix}"));
    }
    DATA_FAMILIES
        .iter()
        .find(|(legacy, _)| *legacy == short(name))
        .and_then(|(_, items)| items.get(index))
        .map(|item| format!("minecraft:{item}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colour_families_follow_the_data_order() {
        assert_eq!(
            block(
                "wool",
                &[("color".into(), StateLiteral::String("red".into()))]
            ),
            Some(("minecraft:red_wool".into(), vec![]))
        );
        assert_eq!(
            block(
                "minecraft:stained_hardened_clay",
                &[("color".into(), StateLiteral::String("silver".into()))]
            ),
            Some(("minecraft:light_gray_terracotta".into(), vec![]))
        );
        assert_eq!(
            block("wool", &[]),
            Some(("minecraft:white_wool".into(), vec![]))
        );
        assert_eq!(
            block(
                "wool",
                &[("color".into(), StateLiteral::String("teal".into()))]
            ),
            None
        );
        assert_eq!(item("wool", 14).as_deref(), Some("minecraft:red_wool"));
        assert_eq!(item("log", 2).as_deref(), Some("minecraft:birch_log"));
        assert_eq!(item("log2", 2), None);
        assert_eq!(item("stone", 1), None);
    }
}
