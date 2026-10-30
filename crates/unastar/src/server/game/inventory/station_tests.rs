//! Anvil, smithing table and grindstone: BDS 1.26.51 capture replays
//! through the request engine, engine-level rules, and the world side
//! (window opening, XP, anvil wear) against a real `GameServer` world.

use std::time::Instant;

use glam::IVec3;
use jolyne::valentine::{
    DataItemEntryPayload, EnumsContainerEnumName as Name, EnumsItemStackNetResult as Status,
    FullContainerName, ItemStackRequestCerealConsumeActionData,
    ItemStackRequestCerealCraftRecipeOptionalActionData,
    ItemStackRequestCerealCraftRepairAndDisenchantActionData, ItemStackRequestCerealSlotInfoData,
    ItemStackRequestCerealTakeActionData, ItemStackRequestPacketDataRequestData,
    ItemStackRequestPacketDataRequestDataActionsItem as Action, McpePacketData,
    TypedClientNetIdstructItemStackRequestIdTagint32T0, TypedServerNetIdstructRecipeNetIdTag,
};

use super::capture::decode;
use super::engine::{Context, Outcome, RequestIdAssignments, StationUse, Stores, process};
use super::replay::{IdMap, assert_response, set_slot, set_window, translate};
use super::stations::{ANVIL_BREAK_EVENT, ANVIL_USE_EVENT, grindstone_xp};
use crate::entity::components::{
    ArmourInventory, Inventory, PlayerUi, ScreenKind, StationKind, StationScreen,
};
use crate::item::ItemStack;
use crate::item::enchant::item_tag;
use crate::item::test_support::{VANILLA, stack};

const ANVIL: &str = include_str!("testdata/bds-anvil.jsonl");
const ANVIL_WEAR: &str = include_str!("testdata/bds-anvil-wear.jsonl");
const SMITHING: &str = include_str!("testdata/bds-smithing-table.jsonl");
const GRINDSTONE: &str = include_str!("testdata/bds-grindstone.jsonl");
const GRINDSTONE_PROBE: &str = include_str!("testdata/bds-grindstone-probe.jsonl");

/// BDS's own recipe network IDs in the smithing capture (its CraftingData).
const BDS_SMITHING_RECIPES: &[(u32, &str)] = &[
    (1567, "minecraft:smithing_netherite_sword"),
    (1558, "minecraft:smithing_armor_trim"),
];

/// Our network ID of the advertised smithing recipe `identifier`.
fn smithing_recipe_id(identifier: &str) -> u32 {
    let packet = super::test_support::CRAFTING.0.packet();
    packet
        .smithing_transform_recipes
        .iter()
        .find(|recipe| &*recipe.recipe_id == identifier)
        .map(|recipe| recipe.net_id.raw_id)
        .or_else(|| {
            packet
                .smithing_trim_recipes
                .iter()
                .find(|recipe| &*recipe.recipe_id == identifier)
                .map(|recipe| recipe.net_id.raw_id)
        })
        .unwrap_or_else(|| panic!("{identifier} is advertised"))
}

/// What a replay saw.
#[derive(Debug, Default)]
struct Replayed {
    responses: usize,
    uses: Vec<StationUse>,
    /// Anvil `LevelEvent`s, in order.
    anvil_events: Vec<i32>,
    /// Inventory stacks whose NBT we compared with BDS's resend.
    compared_stacks: usize,
    /// Total XP of the orbs BDS spawned.
    orb_xp: u32,
}

fn level_of(packet: &jolyne::valentine::UpdateAttributesPacket) -> Option<i32> {
    packet
        .attribute_list
        .iter()
        .find(|attribute| &*attribute.name == "minecraft:player.level")
        .map(|attribute| attribute.current_value as i32)
}

/// Replay one station capture: every request through the engine, every
/// response compared with BDS's, every anvil level charge compared with
/// BDS's level update, and every inventory stack BDS resends unchanged
/// compared with ours (item, count and NBT).
fn replay(capture: &str) -> Replayed {
    let mut main = Inventory::new(36);
    let mut armor = ArmourInventory::default();
    let mut offhand = ItemStack::empty();
    let mut ui = PlayerUi::default().0;
    let mut stores = Stores {
        main: &mut main,
        armor: &mut armor,
        offhand: &mut offhand,
        ui: &mut ui,
        block: None,
    };
    let mut cache = RequestIdAssignments::default();
    let mut ids = IdMap::default();
    let mut screen = ScreenKind::None;
    let mut levels = 0;
    let mut expected_level = None;
    let mut pending: Option<(i32, Outcome)> = None;
    let mut seen = Replayed::default();
    let no_creative = |_: u32| None;

    for line in capture.lines() {
        let entry: serde_json::Value = serde_json::from_str(line).unwrap();
        let seq = entry["seq"].as_u64().unwrap();
        let s2c = entry["dir"] == "s2c";
        match decode(entry["hex"].as_str().unwrap()) {
            McpePacketData::ContainerOpenPacket(open) => {
                let kind = match open.container_type as i8 {
                    5 => StationKind::Anvil,
                    33 => StationKind::SmithingTable,
                    26 => StationKind::Grindstone,
                    other => panic!("seq {seq}: window type {other}"),
                };
                let at = IVec3::new(open.position.x, open.position.y, open.position.z);
                screen = ScreenKind::Station(StationScreen { kind, at });
            }
            McpePacketData::ContainerClosePacket(_) if !s2c => screen = ScreenKind::None,
            McpePacketData::InventoryContentPacket(content) => {
                if content.container_id == 0 {
                    for (slot, wire) in content.slots.iter().enumerate() {
                        let ours = stores.main.item(slot).unwrap();
                        let same = wire
                            .net_id_variant
                            .and_then(|bds| ids.known(bds))
                            .is_some_and(|id| !ours.is_empty() && id == ours.net_id);
                        if !same {
                            continue;
                        }
                        let theirs = super::replay::stack_of(wire, &mut ids);
                        let context = format!("seq {seq} slot {slot}");
                        assert_eq!(ours.item_id(), theirs.item_id(), "{context}");
                        assert_eq!(ours.count, theirs.count, "{context}");
                        assert_eq!(item_tag(ours), item_tag(&theirs), "{context}: NBT");
                        seen.compared_stacks += 1;
                    }
                }
                set_window(content.container_id, &content.slots, &mut stores, &mut ids);
            }
            McpePacketData::InventorySlotPacket(slot) => set_slot(
                u32::from(slot.container_id),
                slot.slot as usize,
                &slot.item,
                &mut stores,
                &mut ids,
            ),
            McpePacketData::UpdateAttributesPacket(update)
                if update.target_runtime_id.actor_runtime_id == 1 =>
            {
                if let Some(level) = level_of(&update) {
                    if let Some(expected) = expected_level.take() {
                        assert_eq!(level, expected, "seq {seq}: level after the anvil");
                    }
                    levels = level;
                }
            }
            McpePacketData::LevelEventPacket(event)
                if matches!(event.event_id, ANVIL_USE_EVENT | ANVIL_BREAK_EVENT) =>
            {
                // Counted only: the capture's request/event interleaving is
                // checked against our own order by the world tests.
                seen.anvil_events.push(event.event_id);
            }
            McpePacketData::AddActorPacket(actor) if &*actor.actor_type == "minecraft:xp_orb" => {
                seen.orb_xp += actor
                    .actor_data
                    .data
                    .iter()
                    .find(|entry| entry.id == 15)
                    .map(|entry| match &entry.payload {
                        DataItemEntryPayload::DataItemIntPayload(value) => value.value as u32,
                        other => panic!("orb value {other:?}"),
                    })
                    .expect("orb value");
            }
            McpePacketData::ItemStackRequestPacket(packet) => {
                assert_eq!(packet.requests.len(), 1);
                let mut request = packet.requests[0].clone();
                for action in &mut request.actions {
                    if let Action::CraftRecipeActionData(craft) = action {
                        let (_, identifier) = BDS_SMITHING_RECIPES
                            .iter()
                            .find(|(bds, _)| *bds == craft.recipe_net_id.raw_id)
                            .expect("captured smithing recipe");
                        craft.recipe_net_id.raw_id = smithing_recipe_id(identifier);
                    }
                }
                translate(&mut request, &ids);
                let ctx = Context {
                    screen,
                    creative: false,
                    can_drop: true,
                    now: Instant::now(),
                    creative_item: &no_creative,
                    crafting: &super::test_support::CRAFTING.1,
                    items: &VANILLA,
                    levels,
                    enchanting: None,
                };
                let outcome = process(&request, &mut stores, &mut cache, &ctx);
                pending = Some((request.client_request_id.id, outcome));
            }
            McpePacketData::ItemStackResponsePacket(packet) => {
                let (id, outcome) = pending.take().expect("response of a request");
                let bds = &packet.responses[0];
                assert_eq!(bds.client_request_id.id, id);
                let context = format!("seq {seq} request {id}");
                let ours = outcome.response.expect("negative odd IDs are answered");
                assert_response(bds, ours, &mut ids, &context);
                if let Some(used) = outcome.station {
                    if let StationUse::Anvil { cost } = used {
                        expected_level = Some(levels - cost);
                    }
                    seen.uses.push(used);
                }
                seen.responses += 1;
            }
            _ => {}
        }
    }
    assert!(
        expected_level.is_none(),
        "every anvil use updates the level"
    );
    seen
}

fn anvil_costs(seen: &Replayed) -> Vec<i32> {
    seen.uses
        .iter()
        .map(|used| match used {
            StationUse::Anvil { cost } => *cost,
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn anvil_answers_like_bds() {
    let seen = replay(ANVIL);
    // Renames, unit repairs, a combination, enchantment merges, a refused
    // conflicting sacrifice and the doubling prior-work penalty.
    assert_eq!(
        anvil_costs(&seen),
        [1, 1, 1, 3, 2, 1, 5, 1, 3, 7, 11, 17, 35]
    );
    assert_eq!(seen.anvil_events.len(), seen.uses.len());
    assert!(seen.responses > 40 && seen.compared_stacks > 0, "{seen:?}");
}

#[test]
fn anvil_wear_capture_charges_every_use_and_breaks_once() {
    let seen = replay(ANVIL_WEAR);
    assert_eq!(seen.uses.len(), 60);
    assert!(anvil_costs(&seen).iter().all(|cost| *cost == 1));
    // The break event replaces the use event once; after it the window
    // keeps working without events.
    let breaks = seen
        .anvil_events
        .iter()
        .filter(|event| **event == ANVIL_BREAK_EVENT)
        .count();
    assert_eq!(breaks, 1);
    assert!(seen.anvil_events.len() < seen.uses.len());
}

#[test]
fn smithing_table_answers_like_bds() {
    let seen = replay(SMITHING);
    assert_eq!(
        seen.uses,
        [
            StationUse::Anvil { cost: 1 },
            StationUse::Smithing,
            StationUse::Smithing
        ]
    );
    assert!(seen.compared_stacks > 0, "{seen:?}");
}

#[test]
fn grindstone_answers_like_bds() {
    let probe = replay(GRINDSTONE_PROBE);
    assert!(probe.uses.is_empty() && probe.responses > 4, "{probe:?}");
    let seen = replay(GRINDSTONE);
    let [StationUse::Grindstone { enchantment_value }] = seen.uses[..] else {
        panic!("{seen:?}");
    };
    // BDS spawned 16 XP for Sharpness III; our draw covers it.
    assert_eq!(seen.orb_xp, 16);
    let low = grindstone_xp(enchantment_value, |_| 0);
    let high = grindstone_xp(enchantment_value, |half| half - 1);
    assert!((low..=high).contains(&16), "{low}..={high}");
}

// ── Engine rules ────────────────────────────────────────────────────────────

struct Bench {
    main: Inventory,
    armor: ArmourInventory,
    offhand: ItemStack,
    ui: Inventory,
    cache: RequestIdAssignments,
    screen: ScreenKind,
    creative: bool,
    levels: i32,
    next: i32,
    enchanting: Option<super::engine::EnchantingContext>,
}

impl Bench {
    fn new(kind: StationKind) -> Self {
        Self {
            main: Inventory::new(36),
            armor: ArmourInventory::default(),
            offhand: ItemStack::empty(),
            ui: PlayerUi::default().0,
            cache: RequestIdAssignments::default(),
            screen: ScreenKind::Station(StationScreen {
                kind,
                at: IVec3::ZERO,
            }),
            creative: false,
            levels: 30,
            next: -1,
            enchanting: None,
        }
    }

    fn put(&mut self, ui_slot: usize, stack: ItemStack) -> i32 {
        self.ui.set_item(ui_slot, stack.into_slot()).unwrap();
        self.ui.item(ui_slot).unwrap().net_id
    }

    fn run(&mut self, actions: Vec<Action>, strings: &[&str]) -> Outcome {
        let id = self.next;
        self.next -= 2;
        let mut stores = Stores {
            main: &mut self.main,
            armor: &mut self.armor,
            offhand: &mut self.offhand,
            ui: &mut self.ui,
            block: None,
        };
        process(
            &ItemStackRequestPacketDataRequestData {
                client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 { id },
                actions,
                strings_to_filter: strings.iter().map(|s| (*s).into()).collect(),
                ..Default::default()
            },
            &mut stores,
            &mut self.cache,
            &Context {
                screen: self.screen,
                creative: self.creative,
                can_drop: true,
                now: Instant::now(),
                creative_item: &|_| None,
                crafting: &super::test_support::CRAFTING.1,
                items: &VANILLA,
                levels: self.levels,
                enchanting: self.enchanting,
            },
        )
    }
}

fn slot(name: Name, slot: u8, net_id: i32) -> ItemStackRequestCerealSlotInfoData {
    ItemStackRequestCerealSlotInfoData {
        fullcontainername: FullContainerName {
            container_name: name,
            dynamic_id: None,
        },
        slot,
        net_id_variant: net_id,
    }
}

fn consume(name: Name, ui_slot: u8, net_id: i32, amount: u8) -> Action {
    Action::ConsumeActionData(ItemStackRequestCerealConsumeActionData {
        amount,
        source: slot(name, ui_slot, net_id),
        ..Default::default()
    })
}

fn take_output(request: i32, to: u8) -> Action {
    Action::TakeActionData(ItemStackRequestCerealTakeActionData {
        amount: 1,
        source: slot(Name::Createdoutputcontainer, 50, request),
        destination: slot(Name::Hotbarcontainer, to, 0),
        ..Default::default()
    })
}

fn rename(bench: &mut Bench, net_id: i32, name: &str) -> Outcome {
    let request = bench.next;
    bench.run(
        vec![
            Action::CraftRecipeOptionalActionData(
                ItemStackRequestCerealCraftRecipeOptionalActionData {
                    recipe_net_id: TypedServerNetIdstructRecipeNetIdTag { raw_id: 0 },
                    filtered_string_index: 0,
                    ..Default::default()
                },
            ),
            consume(Name::Anvilinputcontainer, 1, net_id, 1),
            take_output(request, 0),
        ],
        &[name],
    )
}

fn status(outcome: &Outcome) -> Status {
    outcome.response.as_ref().unwrap().result
}

#[test]
fn survival_anvil_refuses_unaffordable_and_too_expensive_work() {
    let mut bench = Bench::new(StationKind::Anvil);
    let sword = crate::item::enchant::with_repair_cost(&stack("minecraft:iron_sword", 1), Some(39));
    let id = bench.put(1, sword);
    // Rename alone: 1 + 39 = 40 levels, "Too Expensive!".
    bench.levels = 1000;
    assert_eq!(
        status(&rename(&mut bench, id, "X")),
        Status::Invalidcraftresult
    );
    // Creative is exempt and charges nothing here (the world commit skips it).
    bench.creative = true;
    let outcome = rename(&mut bench, id, "X");
    assert_eq!(status(&outcome), Status::Success);
    assert_eq!(outcome.station, Some(StationUse::Anvil { cost: 40 }));

    let mut bench = Bench::new(StationKind::Anvil);
    let id = bench.put(1, stack("minecraft:stick", 1));
    bench.levels = 0;
    let outcome = rename(&mut bench, id, "S");
    assert_eq!(status(&outcome), Status::Invalidcraftresult);
    assert_eq!(outcome.station, None);
    bench.levels = 1;
    assert_eq!(status(&rename(&mut bench, id, "S")), Status::Success);
    assert_eq!(
        crate::item::enchant::custom_name(bench.main.item(0).unwrap()).as_deref(),
        Some("S")
    );
}

#[test]
fn station_actions_need_their_own_window() {
    // An anvil craft on a grindstone or the inventory screen is refused.
    let mut bench = Bench::new(StationKind::Grindstone);
    let id = bench.put(1, stack("minecraft:stick", 1));
    assert_eq!(
        status(&rename(&mut bench, id, "S")),
        Status::Invalidrequestcraftactiontype
    );
    // The grindstone names its input stack.
    let mut bench = Bench::new(StationKind::Grindstone);
    let sword =
        crate::item::enchant::with_enchantment_list(&stack("minecraft:iron_sword", 1), &[(9, 1)]);
    let id = bench.put(16, sword);
    let grind = |named: i32| {
        Action::CraftRepairAndDisenchantActionData(
            ItemStackRequestCerealCraftRepairAndDisenchantActionData {
                recipe_net_id: named,
                numberofrequestedcrafts: 1,
                ..Default::default()
            },
        )
    };
    let request = bench.next;
    let wrong = bench.run(
        vec![
            grind(0),
            consume(Name::Grindstoneinputcontainer, 16, id, 1),
            take_output(request, 0),
        ],
        &[],
    );
    assert_eq!(status(&wrong), Status::Invalidcraftresult);
    let request = bench.next;
    let right = bench.run(
        vec![
            grind(id),
            consume(Name::Grindstoneinputcontainer, 16, id, 1),
            take_output(request, 0),
        ],
        &[],
    );
    assert_eq!(status(&right), Status::Success);
    assert_eq!(
        right.station,
        Some(StationUse::Grindstone {
            enchantment_value: 1
        })
    );
    assert!(crate::item::enchant::enchantments(bench.main.item(0).unwrap()).is_empty());
}

#[test]
fn station_screens_address_their_ui_slots_only() {
    let anvil = ScreenKind::Station(StationScreen {
        kind: StationKind::Anvil,
        at: IVec3::ZERO,
    });
    let rule = anvil.rule(Name::Anvilinputcontainer).unwrap();
    assert_eq!((rule.index(1), rule.index(2)), (Some(Some(1)), None));
    assert_eq!(
        anvil.rule(Name::Anvilmaterialcontainer).unwrap().index(2),
        Some(Some(2))
    );
    assert!(anvil.rule(Name::Smithingtableinputcontainer).is_none());
    assert!(anvil.rule(Name::Craftinginputcontainer).is_none());
    assert!(anvil.rule(Name::Hotbarcontainer).is_some());
    assert!(!anvil.allows_destroy());
    let smithing = ScreenKind::Station(StationScreen {
        kind: StationKind::SmithingTable,
        at: IVec3::ZERO,
    });
    for (name, slot) in [
        (Name::Smithingtableinputcontainer, 51),
        (Name::Smithingtablematerialcontainer, 52),
        (Name::Smithingtabletemplatecontainer, 53),
    ] {
        assert_eq!(
            smithing.rule(name).unwrap().index(slot),
            Some(Some(usize::from(slot)))
        );
    }
    let grindstone = ScreenKind::Station(StationScreen {
        kind: StationKind::Grindstone,
        at: IVec3::ZERO,
    });
    assert_eq!(
        grindstone
            .rule(Name::Grindstoneadditionalcontainer)
            .unwrap()
            .index(17),
        Some(Some(17))
    );
}

// ── Enchanting table, stonecutter, loom, cartography table ─────────────────

const OPTION_BASE: u32 = 90_000;

fn craft(net_id: u32, count: u8) -> Action {
    Action::CraftRecipeActionData(
        jolyne::valentine::ItemStackRequestCerealCraftRecipeActionData {
            recipe_net_id: TypedServerNetIdstructRecipeNetIdTag { raw_id: net_id },
            numberofrequestedcrafts: count,
            ..Default::default()
        },
    )
}

fn enchanting_bench(seed: u32, item: ItemStack, lapis: u8) -> (Bench, i32, i32) {
    let mut bench = Bench::new(StationKind::EnchantingTable);
    bench.enchanting = Some(super::engine::EnchantingContext {
        seed,
        bookshelves: 15,
        first_option_id: OPTION_BASE,
    });
    let item = bench.put(14, item);
    let lapis = bench.put(15, stack("minecraft:lapis_lazuli", lapis));
    (bench, item, lapis)
}

fn enchant(bench: &mut Bench, option: u8, item: i32, lapis: i32, lapis_used: u8) -> Outcome {
    let request = bench.next;
    bench.run(
        vec![
            craft(OPTION_BASE + u32::from(option), 1),
            consume(Name::Enchantinginputcontainer, 14, item, 1),
            consume(Name::Enchantingmaterialcontainer, 15, lapis, lapis_used),
            take_output(request, 0),
        ],
        &[],
    )
}

#[test]
fn enchanting_applies_the_offer_and_takes_lapis_and_levels() {
    let sword = stack("minecraft:iron_sword", 1);
    let offers = crate::item::enchanting::offers(&sword, 15, 77).unwrap();
    let (mut bench, item, lapis) = enchanting_bench(77, sword, 5);
    let outcome = enchant(&mut bench, 2, item, lapis, 3);
    assert_eq!(status(&outcome), Status::Success);
    assert_eq!(outcome.station, Some(StationUse::Enchant { levels: 3 }));
    let result = bench.main.item(0).unwrap();
    assert_eq!(result.item_id(), "minecraft:iron_sword");
    assert_eq!(
        crate::item::enchant::enchantments(result),
        offers[2].enchantments
    );
    assert!(bench.ui.item(14).unwrap().is_empty());
    // Measured: `{Damage: 0, ench: [...]}`.
    assert_eq!(crate::item::enchant::durability_damage(result), 0);
    assert!(item_tag(result).unwrap().0.contains_key("Damage"));
    assert_eq!(bench.ui.item(15).unwrap().count, 2);
}

#[test]
fn enchanting_refuses_bad_ids_missing_levels_and_short_lapis() {
    let sword = || stack("minecraft:diamond_sword", 1);
    // Unknown option IDs, and no table context at all.
    let (mut bench, item, lapis) = enchanting_bench(5, sword(), 3);
    let request = bench.next;
    let outcome = bench.run(
        vec![
            craft(OPTION_BASE + 3, 1),
            consume(Name::Enchantinginputcontainer, 14, item, 1),
            take_output(request, 0),
        ],
        &[],
    );
    assert_eq!(status(&outcome), Status::Failedtofindrecipebynetid);
    bench.enchanting = None;
    assert_eq!(
        status(&enchant(&mut bench, 0, item, lapis, 1)),
        Status::Failedtofindrecipebynetid
    );
    // The bottom offer costs 30 levels with 15 bookshelves (measured
    // status for a level 2 player).
    let (mut bench, item, lapis) = enchanting_bench(5, sword(), 3);
    bench.levels = 29;
    assert_eq!(
        status(&enchant(&mut bench, 2, item, lapis, 3)),
        Status::Insufficientplayerleveltoenchant
    );
    bench.creative = true;
    assert_eq!(
        status(&enchant(&mut bench, 2, item, lapis, 3)),
        Status::Success
    );
    // Two lapis cannot pay for the bottom offer.
    let (mut bench, item, lapis) = enchanting_bench(5, sword(), 2);
    assert_ne!(
        status(&enchant(&mut bench, 2, item, lapis, 2)),
        Status::Success
    );
    assert_ne!(
        status(&enchant(&mut bench, 2, item, lapis, 3)),
        Status::Success
    );
    assert_eq!(
        bench.ui.item(14).unwrap().item_id(),
        "minecraft:diamond_sword"
    );
    // Already enchanted items get no offers.
    let enchanted = crate::item::enchant::with_enchantment_list(&sword(), &[(9, 1)]);
    let (mut bench, item, lapis) = enchanting_bench(5, enchanted, 3);
    assert_eq!(
        status(&enchant(&mut bench, 0, item, lapis, 1)),
        Status::Failedtoenchant
    );
}

#[test]
fn enchanting_a_book_makes_an_enchanted_book() {
    let book = stack("minecraft:book", 1);
    let seed = (0..100)
        .find(|seed| {
            !crate::item::enchanting::offers(&book, 15, *seed).unwrap()[0]
                .enchantments
                .is_empty()
        })
        .unwrap();
    let offer = crate::item::enchanting::offers(&book, 15, seed).unwrap()[0].clone();
    let (mut bench, item, lapis) = enchanting_bench(seed, book, 1);
    assert_eq!(
        status(&enchant(&mut bench, 0, item, lapis, 1)),
        Status::Success
    );
    let result = bench.main.item(0).unwrap();
    assert_eq!(result.item_id(), "minecraft:enchanted_book");
    assert_eq!(
        crate::item::enchant::enchantments(result),
        offer.enchantments
    );
    assert!(bench.ui.item(15).unwrap().is_empty());
}

#[test]
fn enchanting_slots_take_one_item_and_lapis_only() {
    let table = ScreenKind::Station(StationScreen {
        kind: StationKind::EnchantingTable,
        at: IVec3::ZERO,
    });
    let input = table.rule(Name::Enchantinginputcontainer).unwrap();
    assert_eq!(input.index(14), Some(Some(14)));
    assert_eq!(input.set_count(&stack("minecraft:book", 64)), 1);
    let lapis = table.rule(Name::Enchantingmaterialcontainer).unwrap();
    assert!(lapis.accepts(15, &stack("minecraft:lapis_lazuli", 1)));
    assert!(!lapis.accepts(15, &stack("minecraft:stick", 1)));
    assert!(table.rule(Name::Anvilinputcontainer).is_none());
}

/// Our network ID of the advertised recipe `identifier` with station `tag`.
fn tagged_recipe(identifier: &str, tag: &str) -> u32 {
    super::test_support::CRAFTING
        .0
        .packet()
        .shapeless_recipes
        .iter()
        .find(|recipe| &*recipe.recipe_id == identifier && &*recipe.tag == tag)
        .map(|recipe| recipe.net_id.raw_id)
        .unwrap_or_else(|| panic!("{identifier} ({tag}) is advertised"))
}

#[test]
fn stonecutter_crafts_its_recipes_only() {
    let mut bench = Bench::new(StationKind::Stonecutter);
    let stone = bench.put(3, stack("minecraft:stone", 5));
    let slab = tagged_recipe("minecraft:stonecutter_double_stone_slab", "stonecutter");
    let request = bench.next;
    let outcome = bench.run(
        vec![
            craft(slab, 2),
            consume(Name::Stonecutterinputcontainer, 3, stone, 2),
            Action::TakeActionData(ItemStackRequestCerealTakeActionData {
                amount: 4,
                source: slot(Name::Createdoutputcontainer, 50, request),
                destination: slot(Name::Hotbarcontainer, 0, 0),
                ..Default::default()
            }),
        ],
        &[],
    );
    assert_eq!(status(&outcome), Status::Success);
    assert_eq!(outcome.station, Some(StationUse::Crafted));
    assert_eq!(bench.main.item(0).unwrap().count, 4);
    assert!(bench.main.item(0).unwrap().item_id().contains("slab"));
    assert_eq!(bench.ui.item(3).unwrap().count, 3);
    // A crafting-table recipe is not a stonecutter recipe.
    let planks = super::test_support::recipe_id("minecraft:oak_planks");
    let request = bench.next;
    let outcome = bench.run(
        vec![
            craft(planks, 1),
            consume(Name::Stonecutterinputcontainer, 3, stone, 1),
            take_output(request, 1),
        ],
        &[],
    );
    assert_eq!(status(&outcome), Status::Mismatchedrecipeforinputgriditems);
    // Nor does a stonecutter recipe accept another input.
    let mut bench = Bench::new(StationKind::Stonecutter);
    let dirt = bench.put(3, stack("minecraft:dirt", 1));
    let brick = tagged_recipe("minecraft:stonecutter_stonebrick", "stonecutter");
    let request = bench.next;
    let outcome = bench.run(
        vec![
            craft(brick, 1),
            consume(Name::Stonecutterinputcontainer, 3, dirt, 1),
            take_output(request, 0),
        ],
        &[],
    );
    assert_eq!(status(&outcome), Status::Mismatchedrecipeforinputgriditems);
}

#[test]
fn cartography_table_makes_empty_and_locator_maps() {
    let mut bench = Bench::new(StationKind::CartographyTable);
    let paper = bench.put(12, stack("minecraft:paper", 2));
    let map = tagged_recipe("minecraft:cartography_table_map", "cartography_table");
    let request = bench.next;
    let outcome = bench.run(
        vec![
            craft(map, 1),
            consume(Name::Cartographyinputcontainer, 12, paper, 1),
            take_output(request, 0),
        ],
        &[],
    );
    assert_eq!(status(&outcome), Status::Success);
    assert_eq!(bench.main.item(0).unwrap().item_id(), "minecraft:empty_map");
    assert_eq!(bench.main.item(0).unwrap().damage, 0);
    let compass = bench.put(13, stack("minecraft:compass", 1));
    let locator = tagged_recipe(
        "minecraft:cartography_table_locator_map",
        "cartography_table",
    );
    let request = bench.next;
    let outcome = bench.run(
        vec![
            craft(locator, 1),
            consume(Name::Cartographyinputcontainer, 12, paper, 1),
            consume(Name::Cartographyadditionalcontainer, 13, compass, 1),
            take_output(request, 1),
        ],
        &[],
    );
    assert_eq!(status(&outcome), Status::Success);
    assert_eq!(bench.main.item(1).unwrap().item_id(), "minecraft:empty_map");
    assert_eq!(bench.main.item(1).unwrap().damage, 2);
    assert!(bench.ui.item(12).unwrap().is_empty() && bench.ui.item(13).unwrap().is_empty());
}

fn loom_action(pattern: &str, count: u8) -> Action {
    Action::CraftLoomActionData(
        jolyne::valentine::ItemStackRequestCerealCraftLoomActionData {
            pattern_name_id: pattern.into(),
            num_crafts: count,
            ..Default::default()
        },
    )
}

fn patterns(stack: &ItemStack) -> Vec<(i32, String)> {
    use zuri_nbt::NBTTag;
    let Some(NBTTag::List(list)) = item_tag(stack).and_then(|tag| tag.0.get("Patterns").cloned())
    else {
        return Vec::new();
    };
    list.0
        .iter()
        .map(|entry| {
            let NBTTag::Compound(entry) = entry else {
                panic!("{entry:?}")
            };
            let (Some(NBTTag::Int(colour)), Some(NBTTag::String(pattern))) =
                (entry.0.get("Color"), entry.0.get("Pattern"))
            else {
                panic!("{entry:?}")
            };
            (colour.0, pattern.0.clone())
        })
        .collect()
}

fn weave(bench: &mut Bench, pattern: &str, banner: i32, dye: i32, to: u8) -> Outcome {
    let request = bench.next;
    bench.run(
        vec![
            loom_action(pattern, 1),
            consume(Name::Loominputcontainer, 9, banner, 1),
            consume(Name::Loomdyecontainer, 10, dye, 1),
            take_output(request, to),
        ],
        &[],
    )
}

#[test]
fn loom_adds_a_dyed_pattern_to_a_banner() {
    let mut bench = Bench::new(StationKind::Loom);
    let white_banner = stack("minecraft:banner", 1).with_damage(15);
    let banner = bench.put(9, white_banner);
    let dye = bench.put(10, stack("minecraft:red_dye", 8));
    let outcome = weave(&mut bench, "bs", banner, dye, 0);
    assert_eq!(status(&outcome), Status::Success);
    assert_eq!(outcome.station, Some(StationUse::Crafted));
    let result = bench.main.item(0).unwrap().clone();
    assert_eq!((result.item_id(), result.damage), ("minecraft:banner", 15));
    assert_eq!(patterns(&result), vec![(1, "bs".to_string())]);
    assert!(bench.ui.item(9).unwrap().is_empty());
    assert_eq!(bench.ui.item(10).unwrap().count, 7);

    // The flower needs its banner pattern item, which stays.
    let banner = bench.put(9, result);
    assert_eq!(
        status(&weave(&mut bench, "flo", banner, dye, 1)),
        Status::Invalidcraftresult
    );
    bench.put(11, stack("minecraft:flower_banner_pattern", 1));
    assert_eq!(
        status(&weave(&mut bench, "flo", banner, dye, 1)),
        Status::Success
    );
    assert_eq!(
        patterns(bench.main.item(1).unwrap()),
        vec![(1, "bs".to_string()), (1, "flo".to_string())]
    );
    assert_eq!(
        bench.ui.item(11).unwrap().item_id(),
        "minecraft:flower_banner_pattern"
    );
    // Unknown patterns, non-dyes and a seventh layer make nothing.
    let mut banner_stack = bench.main.item(1).unwrap().clone();
    for _ in 0..4 {
        banner_stack = super::stations::loom(
            &banner_stack,
            &stack("minecraft:blue_dye", 1),
            &ItemStack::empty(),
            "ts",
        )
        .unwrap();
    }
    assert_eq!(patterns(&banner_stack).len(), 6);
    let banner = bench.put(9, banner_stack);
    assert_eq!(
        status(&weave(&mut bench, "ts", banner, dye, 2)),
        Status::Invalidcraftresult
    );
    let fresh = bench.put(9, stack("minecraft:banner", 1));
    assert_eq!(
        status(&weave(&mut bench, "nope", fresh, dye, 2)),
        Status::Invalidcraftresult
    );
    let stick = bench.put(10, stack("minecraft:stick", 1));
    assert_eq!(
        status(&weave(&mut bench, "ts", fresh, stick, 2)),
        Status::Invalidcraftresult
    );
}

#[test]
fn bookshelves_count_around_the_table_unless_blocked() {
    use super::stations::count_bookshelves;
    use std::collections::HashSet;
    // A full ring two blocks out on both levels: 32 shelves, capped at 15.
    let ring: HashSet<IVec3> = (-2..=2)
        .flat_map(|x| (-2..=2).map(move |z| (x, z)))
        .filter(|(x, z): &(i32, i32)| x.abs() == 2 || z.abs() == 2)
        .flat_map(|(x, z)| [IVec3::new(x, 0, z), IVec3::new(x, 1, z)])
        .collect();
    assert_eq!(count_bookshelves(|p| ring.contains(&p), |_| true), 15);
    // One shelf straight out, one diagonal, one offset.
    let shelves: HashSet<IVec3> = [
        IVec3::new(2, 0, 0),
        IVec3::new(-2, 1, -2),
        IVec3::new(1, 0, 2),
        // Too close and too far count nothing.
        IVec3::new(1, 0, 0),
        IVec3::new(3, 0, 0),
        IVec3::new(2, 2, 0),
    ]
    .into();
    assert_eq!(count_bookshelves(|p| shelves.contains(&p), |_| true), 3);
    // A block between the table and a shelf, at the table's level or
    // above, cuts it off: (1, 0, 0) blocks the shelf at (2, 0, 0); (-1, 1,
    // -1) the diagonal one; (1, 1, 1) the offset one.
    for blocker in [
        IVec3::new(1, 0, 0),
        IVec3::new(-1, 1, -1),
        IVec3::new(1, 1, 1),
    ] {
        assert_eq!(
            count_bookshelves(|p| shelves.contains(&p) && p != blocker, |p| p != blocker),
            2,
            "{blocker}"
        );
    }
}

// ── World side ──────────────────────────────────────────────────────────────

mod world {
    use bevy_ecs::prelude::*;
    use bevy_ecs::schedule::Schedule;
    use glam::{DVec3, IVec3};
    use jolyne::valentine::{
        EnumsContainerEnumName as Name, EnumsItemStackNetResult as Status,
        ItemStackRequestCerealCraftRepairAndDisenchantActionData, ItemStackRequestPacket,
        ItemStackRequestPacketDataRequestData,
        ItemStackRequestPacketDataRequestDataActionsItem as Action, McpePacket, McpePacketData,
        TypedClientNetIdstructItemStackRequestIdTagint32T0,
    };
    use rand::SeedableRng;
    use tokio::sync::mpsc::Receiver;
    use unastar_data::blocks::StateValue;

    use super::{consume, take_output};
    use crate::entity::EntityRandom;
    use crate::entity::components::{
        ContainerWindows, Experience, GameMode, MainInventory, PlayerUi, Position, ScreenKind,
        StationKind,
    };
    use crate::item::test_support::stack;
    use crate::registry::BlockRegistry;
    use crate::server::game::inventory::stations::{self, ANVIL_BREAK_EVENT, ANVIL_USE_EVENT};
    use crate::server::game::packet_queues::{InventoryAction, InventoryPacketQueue};
    use crate::world::chunk::Chunk;
    use crate::world::ecs::{
        ChunkData, ChunkManager, ChunkPosition, ChunkStateFlags, ChunkViewers,
    };

    struct Harness {
        server: crate::GameServer,
        player: Entity,
        rx: Receiver<McpePacket>,
        next_request: i32,
    }

    /// Run `test` on a server thread with one player at (0.5, 64, 0.5)
    /// viewing a stone-floored test chunk at (0, 0).
    fn with_world(mode: GameMode, test: impl FnOnce(&mut Harness) + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let _guard = runtime.enter();
                let mut config = crate::world::WorldConfig::void_spawn_platform(1);
                config.generation_threads = 1;
                let mut server = crate::GameServer::with_config(crate::ServerConfig {
                    default_gamemode: mode,
                    world: config,
                    ..Default::default()
                });
                let (tx, mut rx) = tokio::sync::mpsc::channel(8192);
                let player = server
                    .spawn_player(crate::PlayerSpawnData {
                        session_id: 9,
                        display_name: "stations".into(),
                        xuid: None,
                        uuid: Some(uuid::Uuid::from_u128(9).to_string()),
                        runtime_id: 99,
                        position: DVec3::new(0.5, 64.0 + crate::entity::PLAYER_EYE_HEIGHT, 0.5),
                        saved_data: None,
                        outbound_tx: tx,
                        chunk_radius: 0,
                    })
                    .unwrap();
                let world = server.ecs.world_mut();
                let mut viewers = ChunkViewers::default();
                viewers.insert(player);
                let mut chunk = Chunk::new(0, 0);
                for x in 0..16 {
                    for z in 0..16 {
                        chunk.set_block(x, 63, z, *crate::world::chunk::blocks::STONE);
                    }
                }
                let chunk = world
                    .spawn((ChunkData::new(chunk), ChunkStateFlags::default(), viewers))
                    .id();
                world
                    .resource_mut::<ChunkManager>()
                    .insert(ChunkPosition::new(0, 0), chunk);
                world.insert_resource(EntityRandom(rand::rngs::StdRng::seed_from_u64(26)));
                world.get_mut::<Experience>(player).unwrap().level = 1000;
                while rx.try_recv().is_ok() {}
                test(&mut Harness {
                    server,
                    player,
                    rx,
                    next_request: -1,
                });
            })
            .unwrap()
            .join()
            .unwrap();
    }

    impl Harness {
        fn world(&mut self) -> &mut World {
            self.server.ecs.world_mut()
        }

        fn packets(&mut self) -> Vec<McpePacketData> {
            let mut out = Vec::new();
            while let Ok(packet) = self.rx.try_recv() {
                out.push(packet.data);
            }
            out
        }

        fn place(&mut self, at: IVec3, name: &str, facing: &'static str) {
            let entry = BlockRegistry::vanilla().get_by_name(name).unwrap();
            let state = entry
                .with_state_value(
                    entry.default_state_id,
                    "minecraft:cardinal_direction",
                    StateValue::Str(facing),
                )
                .unwrap_or(entry.default_state_id);
            crate::server::game::blocks::set_block(self.world(), at, state, false);
        }

        fn block(&mut self, at: IVec3) -> (String, Option<StateValue>) {
            let state =
                crate::server::game::blocks::block_at(self.world(), (at.x, at.y, at.z)).unwrap();
            let entry = BlockRegistry::vanilla().get_by_runtime_id(state).unwrap();
            (
                entry.string_id.clone(),
                entry.state_value(state, "minecraft:cardinal_direction"),
            )
        }

        fn put(&mut self, ui_slot: usize, item: crate::item::ItemStack) -> i32 {
            let player = self.player;
            let mut ui = self.world().get_mut::<PlayerUi>(player).unwrap();
            ui.0.set_item(ui_slot, item.into_slot()).unwrap();
            ui.0.item(ui_slot).unwrap().net_id
        }

        fn run_inventory(&mut self, action: InventoryAction) {
            let player = self.player;
            let world = self.world();
            world
                .resource_mut::<InventoryPacketQueue>()
                .0
                .push((player, action));
            let mut schedule = Schedule::default();
            schedule.add_systems(crate::server::game::inventory::apply_inventory);
            schedule.run(world);
        }

        fn request(&mut self, actions: impl FnOnce(i32) -> Vec<Action>, strings: &[&str]) {
            let id = self.next_request;
            self.next_request -= 2;
            self.run_inventory(InventoryAction::ItemStackRequest(ItemStackRequestPacket {
                requests: vec![ItemStackRequestPacketDataRequestData {
                    client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 { id },
                    actions: actions(id),
                    strings_to_filter: strings.iter().map(|s| (*s).into()).collect(),
                    ..Default::default()
                }],
            }));
        }

        /// Empty hotbar slot 0, where each craft's output is taken.
        fn clear_hotbar(&mut self) {
            let player = self.player;
            let mut main = self.world().get_mut::<MainInventory>(player).unwrap();
            main.0.set_item(0, crate::item::ItemStack::empty()).unwrap();
        }

        fn level(&mut self) -> i32 {
            let player = self.player;
            self.world().get::<Experience>(player).unwrap().level
        }
    }

    fn rename_actions(net_id: i32) -> impl FnOnce(i32) -> Vec<Action> {
        move |request| {
            vec![
                Action::CraftRecipeOptionalActionData(Default::default()),
                consume(Name::Anvilinputcontainer, 1, net_id, 1),
                take_output(request, 0),
            ]
        }
    }

    /// Packet kinds in order, for sequence checks.
    fn kinds(packets: &[McpePacketData]) -> Vec<String> {
        packets
            .iter()
            .filter_map(|packet| match packet {
                McpePacketData::LevelEventPacket(event) => {
                    Some(format!("event {}", event.event_id))
                }
                McpePacketData::ItemStackResponsePacket(response) => {
                    Some(format!("response {:?}", response.responses[0].result))
                }
                McpePacketData::UpdateAttributesPacket(update) => Some(format!(
                    "attributes {:?}",
                    update
                        .attribute_list
                        .iter()
                        .map(|a| (a.name.to_string(), a.current_value))
                        .collect::<Vec<_>>()
                )),
                McpePacketData::UpdateBlockPacket(_) => Some("block".into()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn anvil_opens_charges_levels_and_wears_down_to_nothing() {
        with_world(GameMode::Survival, |h| {
            let at = IVec3::new(2, 64, 0);
            h.place(at, "minecraft:anvil", "east");
            h.packets();
            let player = h.player;
            stations::open(h.world(), player, at, StationKind::Anvil);
            let open = h.packets();
            let [
                McpePacketData::ContainerOpenPacket(window),
                McpePacketData::InventoryContentPacket(content),
            ] = &open[..]
            else {
                panic!("{open:?}");
            };
            assert_eq!(
                (
                    window.container_type,
                    window.position.x,
                    window.target_actor_id.actor_unique_id
                ),
                (5, 2, -1)
            );
            assert_eq!(
                (content.container_id, content.slots.len()),
                (u32::from(window.container_id), 3)
            );
            let mut stages = vec![h.block(at)];
            let mut events = Vec::new();
            for use_index in 0..400 {
                let level = h.level();
                h.clear_hotbar();
                let id = h.put(1, stack("minecraft:stick", 1));
                h.request(rename_actions(id), &["S"]);
                let packets = h.packets();
                let sequence = kinds(&packets);
                let broken = stages.last().unwrap().0 == "minecraft:air";
                let response = sequence
                    .iter()
                    .position(|kind| kind == "response Success")
                    .unwrap_or_else(|| panic!("use {use_index}: {sequence:?}"));
                // Only the level attribute follows the response.
                assert!(
                    sequence[response + 1..].iter().any(|kind| {
                        kind == &format!(
                            "attributes [(\"minecraft:player.level\", {}.0)]",
                            level - 1
                        )
                    }),
                    "{sequence:?}"
                );
                assert_eq!(h.level(), level - 1);
                if broken {
                    // The window outlives its anvil and works silently.
                    assert!(!sequence.iter().any(|kind| kind.starts_with("event")));
                    break;
                }
                let event = &sequence[..response];
                assert_eq!(event.len(), 1, "{sequence:?}");
                let block = h.block(at);
                let changed = block != *stages.last().unwrap();
                if changed {
                    assert!(sequence[response..].contains(&"block".to_string()));
                    stages.push(block);
                }
                events.push((event[0].clone(), changed));
            }
            let names: Vec<&str> = stages.iter().map(|(name, _)| name.as_str()).collect();
            assert_eq!(
                names,
                [
                    "minecraft:anvil",
                    "minecraft:chipped_anvil",
                    "minecraft:damaged_anvil",
                    "minecraft:air"
                ]
            );
            for (_, facing) in &stages[..3] {
                assert_eq!(*facing, Some(StateValue::Str("east")));
            }
            let break_event = format!("event {ANVIL_BREAK_EVENT}");
            let use_event = format!("event {ANVIL_USE_EVENT}");
            assert_eq!(events.last().unwrap(), &(break_event.clone(), true));
            assert!(
                events[..events.len() - 1]
                    .iter()
                    .all(|(event, _)| event == &use_event)
            );
            assert!(matches!(
                h.world().get::<ContainerWindows>(player).unwrap().screen(),
                ScreenKind::Station(_)
            ));
        });
    }

    #[test]
    fn creative_anvil_neither_charges_nor_wears() {
        with_world(GameMode::Creative, |h| {
            let at = IVec3::new(2, 64, 0);
            h.place(at, "minecraft:anvil", "north");
            let player = h.player;
            stations::open(h.world(), player, at, StationKind::Anvil);
            for _ in 0..50 {
                h.clear_hotbar();
                let id = h.put(1, stack("minecraft:stick", 1));
                h.request(rename_actions(id), &["S"]);
            }
            let sequence = kinds(&h.packets());
            assert_eq!(
                sequence
                    .iter()
                    .filter(|kind| *kind == "response Success")
                    .count(),
                50
            );
            assert!(!sequence.iter().any(|kind| kind.starts_with("attributes")));
            assert_eq!(h.block(at).0, "minecraft:anvil");
            assert_eq!(h.level(), 1000);
        });
    }

    #[test]
    fn grindstone_spawns_its_xp_as_orbs_at_the_player() {
        with_world(GameMode::Survival, |h| {
            let at = IVec3::new(2, 64, 0);
            h.place(at, "minecraft:grindstone", "north");
            let player = h.player;
            stations::open(h.world(), player, at, StationKind::Grindstone);
            let sword = crate::item::enchant::with_enchantment_list(
                &stack("minecraft:iron_sword", 1),
                &[(9, 5)],
            );
            let id = h.put(16, sword);
            h.request(
                |request| {
                    vec![
                        Action::CraftRepairAndDisenchantActionData(
                            ItemStackRequestCerealCraftRepairAndDisenchantActionData {
                                recipe_net_id: id,
                                numberofrequestedcrafts: 1,
                                ..Default::default()
                            },
                        ),
                        consume(Name::Grindstoneinputcontainer, 16, id, 1),
                        take_output(request, 0),
                    ]
                },
                &[],
            );
            assert!(kinds(&h.packets()).contains(&format!("response {:?}", Status::Success)));
            let eye = h.world().get::<Position>(player).unwrap().0;
            let world = h.world();
            let mut orbs = world.query::<(&crate::entity::xp_orb::XpOrb, &Position)>();
            let mut total = 0;
            for (orb, position) in orbs.iter(world) {
                assert_eq!(position.0, eye);
                total += orb.value;
            }
            // Sharpness V: minimum enchanting level 45, so 23..=45 XP.
            assert!((23..=45).contains(&total), "{total}");
            let main = world.get::<MainInventory>(player).unwrap();
            assert!(crate::item::enchant::enchantments(main.0.item(0).unwrap()).is_empty());
        });
    }

    #[test]
    fn closing_a_station_returns_its_inputs() {
        with_world(GameMode::Survival, |h| {
            let at = IVec3::new(2, 64, 0);
            h.place(at, "minecraft:smithing_table", "north");
            let player = h.player;
            stations::open(h.world(), player, at, StationKind::SmithingTable);
            h.put(51, stack("minecraft:iron_chestplate", 1));
            h.put(53, stack("minecraft:coast_armor_trim_smithing_template", 1));
            h.run_inventory(InventoryAction::ContainerClose(
                jolyne::valentine::ContainerClosePacket {
                    container_id: 2,
                    container_type: 33,
                    server_initiated_close: false,
                },
            ));
            let world = h.world();
            let main = &world.get::<MainInventory>(player).unwrap().0;
            let items: Vec<&str> = (0..2)
                .map(|slot| main.item(slot).unwrap().item_id())
                .collect();
            assert_eq!(
                items,
                [
                    "minecraft:iron_chestplate",
                    "minecraft:coast_armor_trim_smithing_template"
                ]
            );
            let ui = &world.get::<PlayerUi>(player).unwrap().0;
            assert!(ui.item(51).unwrap().is_empty() && ui.item(53).unwrap().is_empty());
            assert_eq!(
                world.get::<ContainerWindows>(player).unwrap().screen(),
                ScreenKind::None
            );
        });
    }

    #[test]
    fn enchanting_table_offers_enchants_and_reoffers() {
        with_world(GameMode::Survival, |h| {
            let at = IVec3::new(5, 64, 5);
            h.place(at, "minecraft:enchanting_table", "north");
            for z in 3..=7 {
                for y in 64..=65 {
                    h.place(IVec3::new(7, y, z), "minecraft:bookshelf", "north");
                }
            }
            // Blocks the columns at (7, 3) and (7, 4): 10 - 4 shelves.
            h.place(IVec3::new(6, 65, 4), "minecraft:stone", "north");
            assert_eq!(stations::bookshelves(h.world(), at), 6);
            h.packets();
            let player = h.player;
            stations::open(h.world(), player, at, StationKind::EnchantingTable);
            let open = h.packets();
            let [
                McpePacketData::ContainerOpenPacket(window),
                McpePacketData::InventoryContentPacket(content),
            ] = &open[..]
            else {
                panic!("{open:?}");
            };
            assert_eq!((window.container_type, content.slots.len()), (3, 2));

            let sword = stack("minecraft:iron_sword", 1);
            let seed = h
                .world()
                .get::<Experience>(player)
                .unwrap()
                .enchantment_seed;
            let offers = crate::item::enchanting::offers(&sword, 6, seed).unwrap();
            let put_sword = |h: &mut Harness, hotbar: usize| {
                let player = h.player;
                let sword_id = {
                    let mut main = h.world().get_mut::<MainInventory>(player).unwrap();
                    main.0
                        .set_item(hotbar, stack("minecraft:iron_sword", 1).into_slot())
                        .unwrap();
                    main.0.item(hotbar).unwrap().net_id
                };
                h.request(
                    |_| {
                        vec![Action::PlaceActionData(
                            jolyne::valentine::ItemStackRequestCerealPlaceActionData {
                                amount: 1,
                                source: super::slot(Name::Hotbarcontainer, hotbar as u8, sword_id),
                                destination: super::slot(Name::Enchantinginputcontainer, 14, 0),
                                ..Default::default()
                            },
                        )]
                    },
                    &[],
                );
                h.packets()
            };
            let lapis = h.put(15, stack("minecraft:lapis_lazuli", 3));
            let packets = put_sword(h, 0);
            // Measured: the options precede the response.
            let [
                McpePacketData::PlayerEnchantOptionsPacket(options),
                McpePacketData::ItemStackResponsePacket(response),
            ] = &packets[..]
            else {
                panic!("{packets:?}");
            };
            assert_eq!(response.responses[0].result, Status::Success);
            let first = h
                .world()
                .resource::<crate::server::game::types::CraftingExecutorResource>()
                .0
                .next_free_net_id();
            let expected: Vec<(u32, u8, i32)> = offers
                .iter()
                .enumerate()
                .map(|(slot, offer)| (first + slot as u32, offer.cost as u8, 16))
                .collect();
            let sent: Vec<(u32, u8, i32)> = options
                .options
                .iter()
                .map(|o| (o.enchant_net_id.raw_id, o.cost, o.enchants.slot))
                .collect();
            assert_eq!(sent, expected);
            let top = options.options[0].clone();
            assert!(top.enchant_name.ends_with(' '));
            let listed: usize = top.enchants.item_enchants.iter().map(Vec::len).sum();
            assert_eq!(listed, offers[0].enchantments.len());

            let item = h
                .world()
                .get::<PlayerUi>(player)
                .unwrap()
                .0
                .item(14)
                .unwrap()
                .net_id;
            h.request(
                |request| {
                    vec![
                        super::craft(top.enchant_net_id.raw_id, 1),
                        consume(Name::Enchantinginputcontainer, 14, item, 1),
                        consume(Name::Enchantingmaterialcontainer, 15, lapis, 1),
                        take_output(request, 1),
                    ]
                },
                &[],
            );
            let packets = h.packets();
            // Measured: only the level attribute follows; no new offers.
            assert_eq!(
                kinds(&packets),
                [
                    format!("response {:?}", Status::Success),
                    "attributes [(\"minecraft:player.level\", 999.0)]".to_string()
                ]
            );
            assert!(
                !packets
                    .iter()
                    .any(|p| matches!(p, McpePacketData::PlayerEnchantOptionsPacket(_)))
            );
            let world = h.world();
            assert_ne!(
                world.get::<Experience>(player).unwrap().enchantment_seed,
                seed
            );
            assert_eq!(world.get::<Experience>(player).unwrap().level, 999);
            let main = &world.get::<MainInventory>(player).unwrap().0;
            assert_eq!(
                crate::item::enchant::enchantments(main.item(1).unwrap()),
                offers[0].enchantments
            );
            assert_eq!(
                world
                    .get::<PlayerUi>(player)
                    .unwrap()
                    .0
                    .item(15)
                    .unwrap()
                    .count,
                2
            );
            // The next options packet takes the next three IDs.
            let packets = put_sword(h, 2);
            let Some(McpePacketData::PlayerEnchantOptionsPacket(again)) = packets.first() else {
                panic!("{packets:?}");
            };
            assert_eq!(again.options[0].enchant_net_id.raw_id, first + 3);
        });
    }
}
