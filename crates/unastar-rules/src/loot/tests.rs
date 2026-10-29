use super::*;
use rand::SeedableRng;
use rand::rngs::StdRng;
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
            enchantments: Vec::new(),
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
            enchantments: Vec::new(),
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
            enchantments: Vec::new(),
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
            enchantments: Vec::new(),
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

/// A THIS actor and level with the facts the mob loot conditions read.
#[derive(Clone, Copy)]
struct Mob {
    player: bool,
    pet: bool,
    baby: bool,
    vehicle: Option<&'static str>,
    difficulty: Option<Difficulty>,
    regional: Option<f32>,
    enchanting: Option<crate::enchanting::ItemEnchanting>,
}

impl Default for Mob {
    fn default() -> Self {
        Self {
            player: false,
            pet: false,
            baby: false,
            vehicle: None,
            difficulty: Some(Difficulty::Normal),
            regional: Some(0.0),
            enchanting: None,
        }
    }
}

impl LootContext for Mob {
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
        self.player
    }
    fn killed_by_pet(&self) -> bool {
        self.pet
    }
    fn looting(&self) -> u8 {
        0
    }
    fn is_baby(&self) -> bool {
        self.baby
    }
    fn vehicle_is(&self, entity_type: &str) -> bool {
        self.vehicle == Some(entity_type)
    }
    fn difficulty(&self) -> Option<Difficulty> {
        self.difficulty
    }
    fn clamped_regional_difficulty(&self) -> Option<f32> {
        self.regional
    }
    fn item_enchanting(&self, _: &str) -> Result<crate::enchanting::ItemEnchanting, LootError> {
        self.enchanting
            .ok_or(LootError::MissingContext("item enchanting"))
    }
}

fn pool_table(pool: Value) -> LootTable {
    decode_table("control", "authored control", &json!({"pools": [pool]})).unwrap()
}

fn guarded(conditions: Value) -> LootTable {
    pool_table(json!({"rolls": 1, "conditions": conditions,
        "entries": [{"type": "item", "name": "addon:prize"}]}))
}

fn rolls(table: &LootTable, mob: Mob, seeds: u64) -> Vec<Vec<LootDrop>> {
    (0..seeds)
        .map(|seed| {
            roll(
                &LootTables::default(),
                table,
                &mob,
                &mut StdRng::seed_from_u64(seed),
            )
            .unwrap()
        })
        .collect()
}

fn hits(table: &LootTable, mob: Mob) -> usize {
    rolls(table, mob, 200)
        .iter()
        .filter(|d| !d.is_empty())
        .count()
}

#[test]
fn pets_count_as_player_kills_only_for_the_or_pets_condition() {
    let or_pets = guarded(json!([{"condition": "killed_by_player_or_pets"}]));
    let player_only = guarded(json!([{"condition": "killed_by_player"}]));
    let pet = Mob {
        pet: true,
        ..Mob::default()
    };
    assert_eq!(hits(&or_pets, pet), 200);
    assert_eq!(hits(&player_only, pet), 0);
    assert_eq!(hits(&or_pets, Mob::default()), 0);
}

#[test]
fn baby_and_vehicle_conditions_read_the_this_actor() {
    let table = guarded(json!([
        {"condition": "passenger_of_entity", "entity_type": "chicken"},
        {"condition": "is_baby"}
    ]));
    let rider = Mob {
        baby: true,
        vehicle: Some("minecraft:chicken"),
        ..Mob::default()
    };
    assert_eq!(hits(&table, rider), 200, "short identifiers are namespaced");
    let adult = Mob {
        baby: false,
        ..rider
    };
    assert_eq!(hits(&table, adult), 0);
    let on_pig = Mob {
        vehicle: Some("minecraft:pig"),
        ..rider
    };
    assert_eq!(hits(&table, on_pig), 0);
    let walking = Mob {
        vehicle: None,
        ..rider
    };
    assert_eq!(hits(&table, walking), 0);
}

#[test]
fn difficulty_chance_uses_the_named_level_or_the_default() {
    let table = guarded(json!([{"condition": "random_difficulty_chance",
        "default_chance": 1.0, "peaceful": 0, "easy": 0}]));
    let at = |difficulty| Mob {
        difficulty: Some(difficulty),
        ..Mob::default()
    };
    assert_eq!(hits(&table, at(Difficulty::Easy)), 0);
    assert_eq!(hits(&table, at(Difficulty::Normal)), 200);
    assert_eq!(hits(&table, at(Difficulty::Hard)), 200);
    let half = guarded(json!([{"condition": "random_difficulty_chance",
        "default_chance": 0.5, "peaceful": 0, "hard": 0.6}]));
    let normal = hits(&half, at(Difficulty::Normal));
    assert!((70..=130).contains(&normal), "about half: {normal}");
    let no_level = Mob {
        difficulty: None,
        ..Mob::default()
    };
    assert_eq!(
        roll(
            &LootTables::default(),
            &table,
            &no_level,
            &mut StdRng::seed_from_u64(0)
        ),
        Err(LootError::MissingContext("difficulty"))
    );
}

#[test]
fn regional_chance_scales_max_chance_by_the_clamped_regional_difficulty() {
    let table = guarded(json!([{"condition": "random_regional_difficulty_chance",
        "max_chance": 0.5}]));
    let at = |regional| Mob {
        regional: Some(regional),
        ..Mob::default()
    };
    assert_eq!(hits(&table, at(0.0)), 0, "easy worlds clamp to 0");
    let full = hits(&table, at(1.0));
    assert!((70..=130).contains(&full), "max_chance 0.5: {full}");
    let quarter = hits(&table, at(0.5));
    assert!((25..=80).contains(&quarter), "0.25: {quarter}");
}

#[test]
fn tiers_pick_by_index_and_bonus_and_ignore_entry_conditions() {
    let entries: Vec<Value> = (0..6)
        .map(|i| {
            json!({"type": "item", "name": format!("addon:tier{i}"),
            "conditions": [{"condition": "killed_by_player"}]})
        })
        .collect();
    let table = pool_table(json!({
        "tiers": {"initial_range": 3, "bonus_rolls": 3, "bonus_chance": 0.1087},
        "entries": entries
    }));
    let mut seen = [0usize; 6];
    for drops in rolls(&table, Mob::default(), 4000) {
        assert_eq!(drops.len(), 1, "exactly one tier");
        let tier = drops[0].item_id["addon:tier".len()..]
            .parse::<usize>()
            .unwrap();
        seen[tier] += 1;
    }
    assert!(seen[..3].iter().all(|n| *n > 900), "{seen:?}");
    assert!(seen[3] > 100 && seen[4] > 10 && seen[5] > 0, "{seen:?}");
    assert!(seen[3] < seen[2] && seen[5] < seen[4], "{seen:?}");

    let short = pool_table(json!({
        "tiers": {"initial_range": 1, "bonus_rolls": 1, "bonus_chance": 1.0},
        "entries": [{"type": "item", "name": "addon:only"}]
    }));
    assert!(
        rolls(&short, Mob::default(), 20).iter().all(Vec::is_empty),
        "a tier past the last entry drops nothing"
    );
}

#[test]
fn nested_pools_roll_only_after_their_entry_created_an_item() {
    let chain = |count: u32| {
        pool_table(json!({"rolls": 1, "entries": [{
            "type": "item", "name": "addon:helmet",
            "functions": [{"function": "set_count", "count": count}],
            "pools": [{"rolls": 1, "entries": [{
                "type": "item", "name": "addon:chestplate",
                "pools": [{"rolls": 1, "entries": [{"type": "item", "name": "addon:boots"}]}]
            }]}]
        }]}))
    };
    let drops = &rolls(&chain(1), Mob::default(), 1)[0];
    let names: Vec<&str> = drops.iter().map(|d| d.item_id.as_str()).collect();
    assert_eq!(names, ["addon:helmet", "addon:chestplate", "addon:boots"]);
    assert!(rolls(&chain(0), Mob::default(), 1)[0].is_empty());
}

#[test]
fn enchant_random_gear_needs_regional_difficulty_and_item_facts() {
    use crate::enchanting::{ItemEnchanting, slot};
    let table = pool_table(json!({"rolls": 1, "entries": [{
        "type": "item", "name": "addon:sword",
        "functions": [{"function": "enchant_random_gear", "chance": 1.0}]
    }]}));
    let sword = ItemEnchanting {
        slots: slot::SWORD,
        value: 14,
    };
    let hard = Mob {
        regional: Some(1.0),
        enchanting: Some(sword),
        ..Mob::default()
    };
    for drops in rolls(&table, hard, 100) {
        assert!(
            !drops[0].enchantments.is_empty(),
            "chance 1 × 1 always enchants"
        );
    }
    let easy = Mob {
        regional: Some(0.0),
        ..hard
    };
    assert!(
        rolls(&table, easy, 100)
            .iter()
            .all(|d| d[0].enchantments.is_empty())
    );
    let unknown_item = Mob {
        enchanting: None,
        ..hard
    };
    assert_eq!(
        roll(
            &LootTables::default(),
            &table,
            &unknown_item,
            &mut StdRng::seed_from_u64(0)
        ),
        Err(LootError::MissingContext("item enchanting"))
    );
}
