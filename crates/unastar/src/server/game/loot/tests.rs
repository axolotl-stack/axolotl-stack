//! Loot adapter tests over the vanilla block and entity tables.
use super::*;
use crate::item::enchant::with_enchantments;
use crate::item::test_support::{VANILLA, stack};
use crate::registry::BlockRegistry;
use rand::SeedableRng;
use rand::rngs::StdRng;
use unastar_rules::loot::{LootDrop, LootError, LootTable};

fn roll(
    table: &LootTable,
    ctx: &LootContext<'_>,
    rng: &mut impl rand::Rng,
) -> Result<Vec<LootDrop>, LootError> {
    unastar_rules::loot::roll(unastar_data::loot::vanilla(), table, ctx, rng)
}

fn block_ctx<'a>(name: &str, tool: Option<&'a ItemStack>) -> LootContext<'a> {
    let block = BlockRegistry::vanilla().get_by_name(name).expect(name);
    LootContext {
        tool,
        block: Some((block, block.default_state_id)),
        ..LootContext::default()
    }
}

fn table(name: &str) -> &'static LootTable {
    unastar_data::loot::get(name).expect(name)
}

fn counts(name: &str, ctx: &LootContext<'_>, seeds: u64) -> Vec<Vec<LootDrop>> {
    (0..seeds)
        .map(|seed| roll(table(name), ctx, &mut StdRng::seed_from_u64(seed)).unwrap())
        .collect()
}

#[test]
fn stone_drops_cobblestone_unless_silk_touched() {
    let pick = stack("minecraft:iron_pickaxe", 1);
    let silk = with_enchantments(&pick, &[(Enchantment::SilkTouch, 1)]);
    let mut rng = StdRng::seed_from_u64(1);
    let plain = roll(
        table("blocks/stone"),
        &block_ctx("minecraft:stone", Some(&pick)),
        &mut rng,
    )
    .unwrap();
    assert_eq!(
        plain,
        vec![LootDrop {
            item_id: "minecraft:cobblestone".into(),
            count: 1,
            data: 0,
            nbt: None,
            enchantments: Vec::new(),
        }]
    );
    let silky = roll(
        table("blocks/stone"),
        &block_ctx("minecraft:stone", Some(&silk)),
        &mut rng,
    )
    .unwrap();
    assert_eq!(silky[0].item_id, "minecraft:stone");
}

#[test]
fn fortune_multiplies_ore_drops_within_vanilla_bounds() {
    let pick = stack("minecraft:iron_pickaxe", 1);
    let fortune = with_enchantments(&pick, &[(Enchantment::Fortune, 3)]);
    let plain = counts(
        "blocks/diamond_ore",
        &block_ctx("minecraft:diamond_ore", Some(&pick)),
        50,
    );
    assert!(plain.iter().all(|d| d
        == &vec![LootDrop {
            item_id: "minecraft:diamond".into(),
            count: 1,
            data: 0,
            nbt: None,
            enchantments: Vec::new(),
        }]));
    let lucky = counts(
        "blocks/diamond_ore",
        &block_ctx("minecraft:diamond_ore", Some(&fortune)),
        200,
    );
    let amounts: Vec<u32> = lucky.iter().map(|d| d[0].count).collect();
    assert!(amounts.iter().all(|c| (1..=4).contains(c)));
    assert!(amounts.contains(&4) && amounts.contains(&1));
    let lapis = counts(
        "blocks/lapis_ore",
        &block_ctx("minecraft:lapis_ore", Some(&pick)),
        100,
    );
    assert!(lapis.iter().all(|d| (4..=9).contains(&d[0].count)));
}

#[test]
fn gravel_sometimes_drops_flint_and_leaves_need_shears() {
    let gravel = counts("blocks/gravel", &block_ctx("minecraft:gravel", None), 300);
    let flint = gravel
        .iter()
        .filter(|d| d[0].item_id == "minecraft:flint")
        .count();
    assert!(
        (10..=60).contains(&flint),
        "about 10% flint, got {flint}/300"
    );
    let shears = stack("minecraft:shears", 1);
    let sheared = roll(
        table("blocks/oak_leaves"),
        &block_ctx("minecraft:oak_leaves", Some(&shears)),
        &mut StdRng::seed_from_u64(2),
    )
    .unwrap();
    assert_eq!(
        sheared,
        vec![LootDrop {
            item_id: "minecraft:oak_leaves".into(),
            count: 1,
            data: 0,
            nbt: None,
            enchantments: Vec::new(),
        }]
    );
    let by_hand = counts(
        "blocks/oak_leaves",
        &block_ctx("minecraft:oak_leaves", None),
        400,
    );
    assert!(
        by_hand
            .iter()
            .flatten()
            .all(|d| d.item_id != "minecraft:oak_leaves")
    );
    assert!(
        by_hand
            .iter()
            .flatten()
            .any(|d| d.item_id == "minecraft:oak_sapling")
    );
}

#[test]
fn crops_drop_produce_only_when_mature() {
    let block = BlockRegistry::vanilla()
        .get_by_name("minecraft:wheat")
        .unwrap();
    let young = LootContext {
        block: Some((block, block.default_state_id)),
        ..LootContext::default()
    };
    let mature_state = block
        .with_state_value(block.default_state_id, "growth", StateValue::Int(7))
        .unwrap();
    let mature = LootContext {
        block: Some((block, mature_state)),
        ..LootContext::default()
    };
    let mut rng = StdRng::seed_from_u64(5);
    assert_eq!(
        roll(table("blocks/wheat"), &young, &mut rng).unwrap(),
        vec![LootDrop {
            item_id: "minecraft:wheat_seeds".into(),
            count: 1,
            data: 0,
            nbt: None,
            enchantments: Vec::new(),
        }]
    );
    let ripe = roll(table("blocks/wheat"), &mature, &mut rng).unwrap();
    assert_eq!(
        ripe[0],
        LootDrop {
            item_id: "minecraft:wheat".into(),
            count: 1,
            data: 0,
            nbt: None,
            enchantments: Vec::new(),
        }
    );
    assert!(ripe[1].item_id == "minecraft:wheat_seeds" && (1..=4).contains(&ripe[1].count));
}

/// Every block table must evaluate without unsupported features, with and
/// without tools and enchantments. Vanilla also ships `blocks/*` tables
/// that no block shares a name with (e.g. `shelf_mushroom_growth_0`,
/// referenced from block JSON); those roll without a block.
#[test]
fn block_tables_only_use_supported_features() {
    let pick = stack("minecraft:diamond_pickaxe", 1);
    let silk = with_enchantments(&pick, &[(Enchantment::SilkTouch, 1)]);
    let fortune = with_enchantments(&pick, &[(Enchantment::Fortune, 3)]);
    let shears = stack("minecraft:shears", 1);
    let registry = BlockRegistry::vanilla();
    let mut items = crate::registry::ItemRegistry::new();
    items.load_vanilla();
    for table in unastar_data::loot::vanilla()
        .iter()
        .filter(|t| t.name.starts_with("blocks/"))
    {
        let states: Vec<_> = registry
            .get_by_name(&format!("minecraft:{}", &table.name["blocks/".len()..]))
            .map_or_else(
                || vec![None],
                |block| {
                    (block.min_state_id..=block.max_state_id)
                        .map(|state| Some((block, state)))
                        .collect()
                },
            );
        for tool in [
            None,
            Some(&pick),
            Some(&silk),
            Some(&fortune),
            Some(&shears),
        ] {
            for (seed, &block) in states.iter().enumerate() {
                let ctx = LootContext {
                    tool,
                    block,
                    ..LootContext::default()
                };
                let drops = roll(table, &ctx, &mut StdRng::seed_from_u64(seed as u64))
                    .unwrap_or_else(|e| panic!("{}: {e}", table.name));
                for drop in drops {
                    assert!(
                        items.get_by_name(&drop.item_id).is_some(),
                        "{} drops unknown item {}",
                        table.name,
                        drop.item_id
                    );
                }
            }
        }
    }
}

#[test]
fn vanilla_entity_tables_roll_with_looting() {
    let ctx = LootContext {
        killed_by_player: true,
        looting: 3,
        ..LootContext::default()
    };
    let rolls: Vec<Vec<LootDrop>> = (0..50)
        .map(|seed| {
            roll(
                table("entities/spider"),
                &ctx,
                &mut StdRng::seed_from_u64(seed),
            )
            .unwrap()
        })
        .collect();
    let strings: Vec<u32> = rolls
        .iter()
        .flatten()
        .filter(|d| d.item_id == "minecraft:string")
        .map(|d| d.count)
        .collect();
    assert!(
        strings.iter().all(|c| *c <= 5),
        "0-2 plus up to 3 from looting"
    );
    assert!(strings.iter().any(|c| *c > 2), "looting adds string");
}

/// Vanilla features no loot caller implements yet still fail the roll
/// instead of being skipped (`killed_by_entity` guards the creeper's
/// music discs).
#[test]
fn unsupported_vanilla_features_are_errors_not_skipped() {
    let ctx = LootContext {
        killed_by_player: true,
        ..LootContext::default()
    };
    assert_eq!(
        roll(
            table("entities/creeper"),
            &ctx,
            &mut StdRng::seed_from_u64(3)
        ),
        Err(LootError::Unsupported("killed_by_entity".into()))
    );
}

const RARE: [&str; 3] = [
    "minecraft:iron_ingot",
    "minecraft:carrot",
    "minecraft:potato",
];
const DISC: &str = "minecraft:music_disc_lava_chicken";

fn level(difficulty: Difficulty, clamped_regional_difficulty: f32) -> Option<LootLevel> {
    Some(LootLevel {
        difficulty,
        clamped_regional_difficulty,
    })
}

fn zombie_kills(ctx: &LootContext<'_>, seeds: u64) -> Vec<Vec<LootDrop>> {
    counts("entities/zombie", ctx, seeds)
}

fn count_of(drops: &[LootDrop], item: &str) -> u32 {
    drops
        .iter()
        .filter(|d| d.item_id == item)
        .map(|d| d.count)
        .sum()
}

fn rare_rolls(kills: &[Vec<LootDrop>]) -> usize {
    kills
        .iter()
        .filter(|d| RARE.iter().any(|item| count_of(d, item) > 0))
        .count()
}

#[test]
fn zombie_player_kill_drops_flesh_and_rare_produce_at_the_bds_rate() {
    let player = LootContext {
        killed_by_player: true,
        ..LootContext::default()
    };
    let kills = zombie_kills(&player, 4000);
    let flesh: Vec<u32> = kills
        .iter()
        .map(|d| count_of(d, "minecraft:rotten_flesh"))
        .collect();
    assert!(flesh.iter().all(|c| *c <= 2));
    assert!((0..=2).all(|n| flesh.contains(&n)), "0, 1 and 2 flesh");
    let rare = rare_rolls(&kills);
    assert!((65..=140).contains(&rare), "2.5% of 4000: {rare}");
    for item in RARE {
        assert!(kills.iter().any(|d| count_of(d, item) == 1), "{item}");
    }
    assert!(kills.iter().all(|d| count_of(d, DISC) == 0));

    let looting = LootContext {
        looting: 3,
        ..player
    };
    let kills = zombie_kills(&looting, 4000);
    let flesh: Vec<u32> = kills
        .iter()
        .map(|d| count_of(d, "minecraft:rotten_flesh"))
        .collect();
    assert!(flesh.iter().all(|c| *c <= 5), "0-2 plus up to 3");
    assert!(flesh.iter().any(|c| *c > 2), "looting adds flesh");
    let rare = rare_rolls(&kills);
    assert!((170..=280).contains(&rare), "2.5% + 3 × 1%: {rare}");
}

#[test]
fn zombie_killed_by_other_causes_drops_only_flesh() {
    let kills = zombie_kills(&LootContext::default(), 2000);
    assert!(
        kills
            .iter()
            .flatten()
            .all(|d| d.item_id == "minecraft:rotten_flesh")
    );
    let pet = LootContext {
        killed_by_pet: true,
        ..LootContext::default()
    };
    let rare = rare_rolls(&zombie_kills(&pet, 4000));
    assert!((65..=140).contains(&rare), "a pet kill counts: {rare}");
}

#[test]
fn baby_chicken_jockey_drops_the_lava_chicken_disc_only_to_players() {
    let jockey = LootContext {
        killed_by_player: true,
        is_baby: true,
        vehicle: Some(ActorType::Chicken),
        ..LootContext::default()
    };
    assert!(
        zombie_kills(&jockey, 50)
            .iter()
            .all(|d| count_of(d, DISC) == 1)
    );
    for ctx in [
        LootContext {
            killed_by_player: false,
            ..jockey
        },
        LootContext {
            is_baby: false,
            ..jockey
        },
        LootContext {
            vehicle: None,
            ..jockey
        },
        LootContext {
            vehicle: Some(ActorType::Pig),
            ..jockey
        },
    ] {
        assert!(
            zombie_kills(&ctx, 50)
                .iter()
                .all(|d| count_of(d, DISC) == 0)
        );
    }
}

const WEAPONS: [&str; 3] = [
    "minecraft:iron_spear",
    "minecraft:iron_sword",
    "minecraft:iron_shovel",
];

fn is_armour(item: &str) -> bool {
    ["_helmet", "_chestplate", "_leggings", "_boots"]
        .iter()
        .any(|piece| item.ends_with(piece))
}

fn material(item: &str) -> &str {
    &item[..item.rfind('_').expect("armour piece")]
}

fn equipment(level: Option<LootLevel>, seeds: u64) -> Vec<Vec<LootDrop>> {
    let ctx = LootContext {
        level,
        items: Some(&VANILLA),
        ..LootContext::default()
    };
    counts("entities/zombie_equipment", &ctx, seeds)
}

fn armoured(rolls: &[Vec<LootDrop>]) -> usize {
    rolls
        .iter()
        .filter(|d| d.iter().any(|d| is_armour(&d.item_id)))
        .count()
}

#[test]
fn zombie_equipment_needs_normal_or_hard_and_regional_difficulty() {
    // Easy clamps the regional difficulty to 0 and its armour chance is 0.
    assert!(
        equipment(level(Difficulty::Easy, 0.0), 2000)
            .iter()
            .all(Vec::is_empty)
    );
    assert_eq!(armoured(&equipment(level(Difficulty::Easy, 1.0), 2000)), 0);
    let hard = equipment(level(Difficulty::Hard, 1.0), 4000);
    let armour = armoured(&hard);
    assert!((500..=700).contains(&armour), "15% of 4000: {armour}");
    let weapons: Vec<&str> = hard
        .iter()
        .flatten()
        .map(|d| d.item_id.as_str())
        .filter(|item| WEAPONS.contains(item))
        .collect();
    assert!(
        (140..=260).contains(&weapons.len()),
        "5% of 4000: {}",
        weapons.len()
    );
    let spears = weapons.iter().filter(|w| w.ends_with("spear")).count();
    let shovels = weapons.iter().filter(|w| w.ends_with("shovel")).count();
    assert!(spears < shovels, "weights 1:2:3");
    let armour = armoured(&equipment(level(Difficulty::Normal, 0.5), 4000));
    assert!((230..=380).contains(&armour), "7.5% of 4000: {armour}");
    assert_eq!(
        roll(
            table("entities/zombie_equipment"),
            &LootContext::default(),
            &mut StdRng::seed_from_u64(0)
        ),
        Err(LootError::MissingContext("regional difficulty"))
    );
}

#[test]
fn zombie_armour_tiers_give_one_material_chained_from_the_helmet() {
    let mut materials = std::collections::BTreeSet::new();
    let (mut helmets, mut chests) = (0, 0);
    for drops in equipment(level(Difficulty::Hard, 1.0), 20_000) {
        let armour: Vec<&LootDrop> = drops.iter().filter(|d| is_armour(&d.item_id)).collect();
        let Some(first) = armour.first() else {
            continue;
        };
        let set = material(&first.item_id);
        assert!(
            armour.iter().all(|d| material(&d.item_id) == set),
            "{armour:?}"
        );
        let order = ["_helmet", "_chestplate", "_leggings", "_boots"];
        for (piece, suffix) in armour.iter().zip(order) {
            assert!(piece.item_id.ends_with(suffix), "chained order {armour:?}");
        }
        helmets += 1;
        chests += usize::from(armour.len() > 1);
        materials.insert(set.to_owned());
    }
    for set in [
        "minecraft:leather",
        "minecraft:copper",
        "minecraft:golden",
        "minecraft:chainmail",
        "minecraft:iron",
    ] {
        assert!(materials.contains(set), "{set} in {materials:?}");
    }
    // Hard: each further piece follows with 60%.
    let ratio = chests as f32 / helmets as f32;
    assert!((0.55..=0.65).contains(&ratio), "{ratio}");
}

#[test]
fn zombie_gear_is_randomly_enchanted_on_hard() {
    let gear: Vec<LootDrop> = equipment(level(Difficulty::Hard, 1.0), 4000)
        .into_iter()
        .flatten()
        .collect();
    let enchanted = gear.iter().filter(|d| !d.enchantments.is_empty()).count();
    let ratio = enchanted as f32 / gear.len() as f32;
    assert!((0.18..=0.32).contains(&ratio), "25% of pieces: {ratio}");
    for drop in &gear {
        let item = VANILLA.get_by_name(&drop.item_id).unwrap();
        let facts = crate::item::enchanting::item_enchanting(item).unwrap();
        let mut check = Vec::new();
        for e in &drop.enchantments {
            assert!(
                unastar_rules::enchanting::add_enchantment(&mut check, facts.slots, *e),
                "{} cannot carry {e:?}",
                drop.item_id
            );
        }
    }
    assert!(
        equipment(level(Difficulty::Normal, 0.0), 2000)
            .iter()
            .flatten()
            .all(|d| d.enchantments.is_empty())
    );
}

/// Every zombie table, and the tables they reference, evaluates for any
/// killer, difficulty and rider state.
#[test]
fn vanilla_zombie_tables_evaluate() {
    let names = [
        "entities/zombie",
        "entities/zombie_equipment",
        "entities/zombie_rider",
        "entities/zombie_rider_equipment",
    ];
    for name in names {
        for difficulty in [
            Difficulty::Peaceful,
            Difficulty::Easy,
            Difficulty::Normal,
            Difficulty::Hard,
        ] {
            for (player, baby) in [(false, false), (true, false), (true, true)] {
                let ctx = LootContext {
                    killed_by_player: player,
                    is_baby: baby,
                    vehicle: baby.then_some(ActorType::Chicken),
                    level: level(difficulty, 1.0),
                    items: Some(&VANILLA),
                    ..LootContext::default()
                };
                for seed in 0..200 {
                    roll(table(name), &ctx, &mut StdRng::seed_from_u64(seed))
                        .unwrap_or_else(|e| panic!("{name}: {e}"));
                }
            }
        }
    }
}
