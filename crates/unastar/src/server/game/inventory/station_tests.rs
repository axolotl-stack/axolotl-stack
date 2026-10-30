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
}
