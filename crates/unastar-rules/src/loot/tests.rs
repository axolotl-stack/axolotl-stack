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
