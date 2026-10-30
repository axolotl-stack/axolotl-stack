use super::*;
use rand::rngs::mock::StepRng;
use serde_json::{Value, json};

#[derive(Default)]
struct Context<'a> {
    fire: Option<bool>,
    furnace: Option<&'a dyn FurnaceSmelt>,
}

impl LootContext for Context<'_> {
    fn tool_item(&self) -> Option<&str> {
        None
    }
    fn enchantment_level(&self, _: i16) -> u8 {
        0
    }
    fn block_state_matches(&self, _: &str, _: &LootStateValue) -> bool {
        false
    }
    fn killed_by_player(&self) -> bool {
        false
    }
    fn looting(&self) -> u8 {
        0
    }
    fn on_fire(&self) -> Option<bool> {
        self.fire
    }
    fn furnace(&self) -> Option<&dyn FurnaceSmelt> {
        self.furnace
    }
}

struct Furnace {
    result: Option<SmeltedItem<'static>>,
}

impl FurnaceSmelt for Furnace {
    fn smelt(&self, identifier: &str, data: i16) -> Option<SmeltedItem<'_>> {
        if identifier == "addon:ore" && data == 7 {
            self.result
        } else {
            None
        }
    }
}

fn table(functions: Value) -> LootTable {
    decode_table(
        "control",
        "authored control",
        &json!({
            "pools": [{"rolls": 1, "entries": [{
                "type": "item", "name": "addon:ore", "functions": functions
            }]}]
        }),
    )
    .unwrap()
}

fn evaluate(table: &LootTable, context: &Context<'_>) -> Result<Vec<LootDrop>, LootError> {
    roll(
        &LootTables::default(),
        table,
        context,
        &mut StepRng::new(0, 0),
    )
}

#[test]
fn false_function_guard_does_not_overwrite_an_earlier_count() {
    let table = table(json!([
        {"function": "set_count", "count": 3},
        {"function": "set_count", "count": 99,
         "conditions": [{"condition": "killed_by_player"}]}
    ]));
    assert_eq!(
        evaluate(&table, &Context::default()).unwrap(),
        vec![LootDrop {
            item_id: "addon:ore".into(),
            count: 3,
            data: 0,
            nbt: None,
        }]
    );
}

/// A THIS actor of palette colour `0`, where `addon:ore` with data 14 names
/// the flattened `addon:red_ore`.
struct Colored(Option<u8>);

impl LootContext for Colored {
    fn tool_item(&self) -> Option<&str> {
        None
    }
    fn enchantment_level(&self, _: i16) -> u8 {
        0
    }
    fn block_state_matches(&self, _: &str, _: &LootStateValue) -> bool {
        false
    }
    fn killed_by_player(&self) -> bool {
        false
    }
    fn looting(&self) -> u8 {
        0
    }
    fn this_color_index(&self) -> Option<u8> {
        self.0
    }
    fn data_alias(&self, name: &str, data: i16) -> Option<&str> {
        (name == "addon:ore" && data == 14).then_some("addon:red_ore")
    }
}

#[test]
fn color_index_sets_data_or_selects_the_flattened_item() {
    let table = table(json!([{"function": "minecraft:set_data_from_color_index"}]));
    let drop = |color| {
        let drops = roll(
            &LootTables::default(),
            &table,
            &Colored(color),
            &mut StepRng::new(0, 0),
        )
        .unwrap();
        (drops[0].item_id.clone(), drops[0].data)
    };
    assert_eq!(drop(Some(14)), ("addon:red_ore".to_owned(), 0));
    assert_eq!(drop(Some(3)), ("addon:ore".to_owned(), 3));
    assert_eq!(drop(None), ("addon:ore".to_owned(), 0), "no THIS actor");
}

fn smelting_table() -> LootTable {
    table(json!([
        {"function": "set_count", "count": 3},
        {"function": "set_data", "data": 7},
        {"function": "furnace_smelt", "conditions": [{
            "condition": "entity_properties", "entity": "this", "properties": {"on_fire": true}
        }]},
        {"function": "set_count", "count": 2, "add": true}
    ]))
}

#[test]
fn burning_actor_uses_borrowed_recipe_identity_and_aux_without_replacing_count() {
    let furnace = Furnace {
        result: Some(SmeltedItem {
            identifier: "addon:refined",
            data: 2,
        }),
    };
    let context = Context {
        fire: Some(true),
        furnace: Some(&furnace),
    };
    assert_eq!(
        evaluate(&smelting_table(), &context).unwrap(),
        vec![LootDrop {
            item_id: "addon:refined".into(),
            count: 5,
            data: 2,
            nbt: None,
        }]
    );
    // A false guard must not require a recipe provider at all.
    assert_eq!(
        evaluate(
            &smelting_table(),
            &Context {
                fire: Some(false),
                furnace: None
            }
        )
        .unwrap(),
        vec![LootDrop {
            item_id: "addon:ore".into(),
            count: 5,
            data: 7,
            nbt: None,
        }]
    );
}

#[test]
fn no_this_actor_is_not_an_actor_whose_fire_state_is_false() {
    let table = table(json!([
        {"function": "set_count", "count": 3},
        {"function": "set_count", "count": 99, "conditions": [{
            "condition": "entity_properties", "entity": "this", "properties": {"on_fire": false}
        }]}
    ]));
    assert_eq!(evaluate(&table, &Context::default()).unwrap()[0].count, 3);
    assert_eq!(
        evaluate(
            &table,
            &Context {
                fire: Some(false),
                furnace: None
            }
        )
        .unwrap()[0]
            .count,
        99
    );
}

#[test]
fn missing_furnace_context_is_distinct_from_a_recipe_miss() {
    assert_eq!(
        evaluate(
            &smelting_table(),
            &Context {
                fire: Some(true),
                furnace: None
            }
        ),
        Err(LootError::MissingContext("furnace_smelt"))
    );
    let furnace = Furnace { result: None };
    assert_eq!(
        evaluate(
            &smelting_table(),
            &Context {
                fire: Some(true),
                furnace: Some(&furnace)
            }
        )
        .unwrap(),
        vec![LootDrop {
            item_id: "addon:ore".into(),
            count: 5,
            data: 7,
            nbt: None,
        }]
    );
}

#[test]
fn additional_entity_predicates_are_not_silently_discarded() {
    let table = table(json!([
        {"function": "set_count", "count": 99, "conditions": [{
            "condition": "entity_properties", "entity": "this",
            "properties": {"on_fire": false, "is_baby": true}
        }]}
    ]));
    assert_eq!(
        evaluate(
            &table,
            &Context {
                fire: Some(false),
                furnace: None
            }
        ),
        Err(LootError::Unsupported("entity_properties".into()))
    );
}

/// A lucky context whose item functions record what they were asked.
struct Lucky {
    luck: f32,
}

impl LootContext for Lucky {
    fn tool_item(&self) -> Option<&str> {
        None
    }
    fn enchantment_level(&self, _: i16) -> u8 {
        0
    }
    fn block_state_matches(&self, _: &str, _: &LootStateValue) -> bool {
        false
    }
    fn killed_by_player(&self) -> bool {
        false
    }
    fn looting(&self) -> u8 {
        0
    }
    fn luck(&self) -> f32 {
        self.luck
    }
    fn item_functions(&self) -> Option<&dyn ItemFunctions> {
        Some(self)
    }
}

impl ItemFunctions for Lucky {
    fn set_damage(&self, drop: &mut LootDrop, fraction: f32) -> Result<(), LootError> {
        drop.data = (fraction * 100.0) as i16;
        Ok(())
    }
    fn enchant_with_levels(
        &self,
        drop: &mut LootDrop,
        levels: i32,
        treasure: bool,
        _: &mut dyn rand::RngCore,
    ) -> Result<(), LootError> {
        drop.item_id = format!("addon:enchanted_{levels}_{treasure}");
        Ok(())
    }
}

#[test]
fn luck_moves_weight_by_quality() {
    // Weights 10 (quality -2) and 5 (quality 2) total 15 for luck 0, 3 and
    // 5 alike: 10/5, 4/11 and 0/15. Pick 9 of 0..15 is junk without luck and
    // treasure with luck 3.
    let table = decode_table(
        "lucky",
        "authored lucky",
        &json!({"pools": [{"rolls": 1, "entries": [
            {"type": "item", "name": "addon:junk", "weight": 10, "quality": -2},
            {"type": "item", "name": "addon:treasure", "weight": 5, "quality": 2}
        ]}]}),
    )
    .unwrap();
    let pick = |luck: f32, draw: u64| {
        // gen_range(0..15) takes the high word of the u32 draw times 15.
        let value = (draw << 32) / 15 + 1;
        roll(
            &LootTables::default(),
            &table,
            &Lucky { luck },
            &mut StepRng::new(value, 0),
        )
        .unwrap()[0]
            .item_id
            .clone()
    };
    assert_eq!(pick(0.0, 9), "addon:junk");
    assert_eq!(pick(3.0, 9), "addon:treasure");
    // Luck 5 drives the junk weight to 0: only treasure remains.
    assert_eq!(pick(5.0, 0), "addon:treasure");
}

#[test]
fn item_functions_get_the_sampled_damage_and_levels() {
    let table = table(json!([
        {"function": "set_damage", "damage": {"min": 0.5, "max": 0.5}},
        {"function": "enchant_with_levels", "levels": 30, "treasure": true}
    ]));
    let drops = roll(
        &LootTables::default(),
        &table,
        &Lucky { luck: 0.0 },
        &mut StepRng::new(0, 0),
    )
    .unwrap();
    assert_eq!(drops[0].data, 50);
    assert_eq!(drops[0].item_id, "addon:enchanted_30_true");
    assert_eq!(
        evaluate(&table, &Context::default()),
        Err(LootError::MissingContext("set_damage"))
    );
}
