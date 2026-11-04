//! The client packets of each interaction step, ported from
//! `tools/client-diff/gt-client/session.go` (`interaction`, `packets`,
//! `stackRequest`): the same packets, fields and stack network ID tracking,
//! so a scenario sends the same requests to BDS and to unastar.

use anyhow::{Context, Result, bail, ensure};
use bytes::Bytes;
use jolyne::valentine::*;
use zuri_nbt::{NBTTag, encoding::LittleEndian, tag};

use super::session::{CraftRecipe, Item, Session, container, count, predict, stack_id};
use crate::scenario::{SlotRef, Step, StepKind};

type Action = ItemStackRequestPacketDataRequestDataActionsItem;
type InputFlag = EnumsPlayerAuthInputPacketPayloadInputData;

/// A stack network ID the server never assigned; `"stale": true` steps send
/// it as the source ID to capture the rejection.
const STALE_STACK_ID: i32 = 999_999;

/// Clicked positions within a block for faces 0-5 (down, up, north, south,
/// west, east).
const FACE_OFFSETS: [[f32; 3]; 6] = [
    [0.5, 0.0, 0.5],
    [0.5, 1.0, 0.5],
    [0.5, 0.5, 0.0],
    [0.5, 0.5, 1.0],
    [0.0, 0.5, 0.5],
    [1.0, 0.5, 0.5],
];

fn face_offset(face: i32) -> [f32; 3] {
    usize::try_from(face)
        .ok()
        .and_then(|face| FACE_OFFSETS.get(face).copied())
        .unwrap_or([0.5, 0.5, 0.5])
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn block_vec(pos: [i32; 3]) -> [f32; 3] {
    [pos[0] as f32, pos[1] as f32, pos[2] as f32]
}

fn v3(v: [f32; 3]) -> Vec3 {
    Vec3 {
        x: v[0],
        y: v[1],
        z: v[2],
    }
}

fn block_pos(pos: [i32; 3]) -> BlockPos {
    BlockPos {
        x: pos[0],
        y: pos[1],
        z: pos[2],
    }
}

fn input_flag(name: &str) -> InputFlag {
    use EnumsPlayerAuthInputPacketPayloadInputData::*;
    match name {
        "sneaking" => Sneaking,
        "sneak_down" => Sneakdown,
        "start_sneaking" => Startsneaking,
        "stop_sneaking" => Stopsneaking,
        "sneak_pressed_raw" => Sneakpressedraw,
        "sneak_current_raw" => Sneakcurrentraw,
        "jumping" => Jumping,
        "jump_down" => Jumpdown,
        "start_jumping" => Startjumping,
        "jump_pressed_raw" => Jumppressedraw,
        "jump_current_raw" => Jumpcurrentraw,
        "want_up" => Wantup,
        "start_gliding" => Startgliding,
        "stop_gliding" => Stopgliding,
        "sprinting" => Sprinting,
        "start_sprinting" => Startsprinting,
        other => unreachable!("input flag {other:?} passed validation"),
    }
}

/// Packets a tick sends after its PlayerAuthInput, built when the tick is
/// sent so they carry the state of that moment (held item, position).
#[derive(Clone, Debug)]
pub enum Deferred {
    UseOnBlock { pos: [i32; 3], face: i32 },
    UseAir,
    UseOnEntity { target: u64, attack: bool },
    Release { consume: bool },
}

/// One PlayerAuthInput tick requested by a step: turn the head to `look`
/// (if set), send the input, then the `after` packets in the same batch.
#[derive(Clone, Debug, Default)]
pub struct InputSpec {
    pub delta: [f32; 3],
    pub look: Option<[f32; 3]>,
    pub flags: Vec<InputFlag>,
    pub block_actions: Option<Vec<PlayerBlockActionData>>,
    pub after: Option<Deferred>,
}

fn slot_info(
    name: EnumsContainerEnumName,
    slot: i32,
    stack: i32,
) -> ItemStackRequestCerealSlotInfoData {
    ItemStackRequestCerealSlotInfoData {
        fullcontainername: FullContainerName {
            container_name: name,
            dynamic_id: None,
        },
        slot: slot as u8,
        net_id_variant: stack,
    }
}

fn inventory_slot(slot: i32, stack: i32) -> ItemStackRequestCerealSlotInfoData {
    let name = if slot < 9 {
        EnumsContainerEnumName::Hotbarcontainer
    } else {
        EnumsContainerEnumName::Inventorycontainer
    };
    slot_info(name, slot, stack)
}

fn cursor_slot(stack: i32) -> ItemStackRequestCerealSlotInfoData {
    slot_info(EnumsContainerEnumName::Cursorcontainer, 0, stack)
}

/// An armour or offhand slot, addressed with stack network ID 0: the bot
/// does not track those containers, the responses show the result.
fn other_slot(name: &str, slot: i32) -> ItemStackRequestCerealSlotInfoData {
    slot_info(container(name), slot, 0)
}

fn request(id: i32, actions: Vec<Action>) -> McpePacket {
    McpePacket::from(ItemStackRequestPacket {
        requests: vec![ItemStackRequestPacketDataRequestData {
            client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 { id },
            actions,
            ..Default::default()
        }],
    })
}

/// Converts a JSON value into NBT as the bot does: integral numbers become
/// TAG_Int (as item tags such as Damage, RepairCost and ench entries use),
/// other numbers TAG_Float, lists typed lists.
fn nbt_value(value: &serde_json::Value) -> Result<NBTTag> {
    use serde_json::Value;
    Ok(match value {
        Value::Number(n) => {
            let f = n.as_f64().context("number out of range")?;
            if f.trunc() == f {
                NBTTag::Int(tag::Int(f as i32))
            } else {
                NBTTag::Float(tag::Float(f as f32))
            }
        }
        Value::String(s) => NBTTag::String(tag::String(s.clone())),
        Value::Bool(b) => NBTTag::Byte(tag::Byte(u8::from(*b))),
        Value::Object(map) => NBTTag::Compound(nbt_compound(map)?),
        Value::Array(items) => NBTTag::List(tag::List(
            items.iter().map(nbt_value).collect::<Result<_>>()?,
        )),
        Value::Null => bail!("null has no NBT form"),
    })
}

fn nbt_compound(map: &serde_json::Map<String, serde_json::Value>) -> Result<tag::Compound> {
    Ok(tag::Compound(
        map.iter()
            .map(|(key, value)| Ok((key.clone(), nbt_value(value)?)))
            .collect::<Result<_>>()?,
    ))
}

/// Item user data: the NBT (marker -1, version 1, little-endian compound)
/// when there is any, no can-place-on/can-destroy entries, and the shield
/// blocking tick for shields.
fn user_data(
    nbt: Option<&serde_json::Map<String, serde_json::Value>>,
    shield: bool,
) -> Result<Bytes> {
    let mut out = Vec::new();
    match nbt.filter(|nbt| !nbt.is_empty()) {
        Some(nbt) => {
            out.extend_from_slice(&(-1i16).to_le_bytes());
            out.push(1);
            NBTTag::Compound(nbt_compound(nbt)?)
                .write(&mut out, LittleEndian)
                .map_err(|error| anyhow::anyhow!("encoding result NBT: {error:?}"))?;
        }
        None => out.extend_from_slice(&0i16.to_le_bytes()),
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    if shield {
        out.extend_from_slice(&0i64.to_le_bytes());
    }
    Ok(out.into())
}

/// The `map_uuid` tag of a held filled map.
fn map_uuid(item: &Item) -> Option<i64> {
    let data = &item.user_data_buffer;
    if data.get(..2)? != (-1i16).to_le_bytes() {
        return None;
    }
    let mut cursor = std::io::Cursor::new(data.get(3..)?);
    let NBTTag::Compound(compound) = NBTTag::read(&mut cursor, LittleEndian).ok()? else {
        return None;
    };
    match compound.0.get("map_uuid")? {
        NBTTag::Long(id) => Some(id.0),
        _ => None,
    }
}

impl Session {
    /// The client packets of an interaction step: packets sent at once and
    /// PlayerAuthInput ticks queued for the tick loop.
    pub fn interaction(&mut self, st: &Step) -> Result<(Vec<McpePacket>, Vec<InputSpec>)> {
        use StepKind::*;
        match st.kind {
            Move => {
                let ticks = st.ticks as f32;
                let delta = [st.dx / ticks, st.dy / ticks, st.dz / ticks];
                let flags: Vec<_> = st.flags.iter().map(|name| input_flag(name)).collect();
                let spec = InputSpec {
                    delta,
                    flags,
                    ..InputSpec::default()
                };
                Ok((Vec::new(), vec![spec; st.ticks as usize]))
            }
            Break => {
                // Without ticks the block breaks at once (creative, or a
                // block the held tool breaks instantly). With ticks the
                // client mines like a survival player: start, crack on each
                // following tick, predict the destruction, then stop.
                let pos = self.block(st.x, st.y, st.z);
                let target = add(block_vec(pos), face_offset(st.face));
                let actions = |kinds: &[EnumsPlayerActionType]| InputSpec {
                    flags: vec![InputFlag::Performblockactions],
                    block_actions: Some(
                        kinds
                            .iter()
                            .map(|kind| PlayerBlockActionData {
                                player_action_type: *kind,
                                position: block_pos(pos),
                                facing: st.face,
                            })
                            .collect(),
                    ),
                    ..InputSpec::default()
                };
                let look = InputSpec {
                    look: Some(target),
                    ..InputSpec::default()
                };
                use EnumsPlayerActionType as A;
                if st.ticks == 0 {
                    return Ok((
                        Vec::new(),
                        vec![
                            look,
                            actions(&[A::Startdestroyblock, A::Predictdestroyblock]),
                        ],
                    ));
                }
                let mut specs = vec![look, actions(&[A::Startdestroyblock])];
                specs.extend((0..st.ticks).map(|_| actions(&[A::Crackblock])));
                specs.push(actions(&[A::Predictdestroyblock]));
                specs.push(actions(&[A::Stopdestroyblock]));
                Ok((Vec::new(), specs))
            }
            Click => {
                let pos = self.block(st.x, st.y, st.z);
                let target = add(block_vec(pos), face_offset(st.face));
                Ok((
                    Vec::new(),
                    vec![
                        InputSpec {
                            look: Some(target),
                            ..InputSpec::default()
                        },
                        InputSpec {
                            after: Some(Deferred::UseOnBlock { pos, face: st.face }),
                            ..InputSpec::default()
                        },
                    ],
                ))
            }
            Attack | Interact => {
                let target = *self
                    .entities
                    .get(&st.entity)
                    .with_context(|| format!("no {} has been added", st.entity))?;
                let aim = add(self.positions[&target], [0.0, 1.0, 0.0]);
                Ok((
                    Vec::new(),
                    vec![
                        InputSpec {
                            look: Some(aim),
                            ..InputSpec::default()
                        },
                        InputSpec {
                            after: Some(Deferred::UseOnEntity {
                                target,
                                attack: st.kind == Attack,
                            }),
                            ..InputSpec::default()
                        },
                    ],
                ))
            }
            Use => {
                // The client sends the transaction while processing input and
                // flags the PlayerAuthInput of the following tick with
                // StartUsingItem.
                let mut specs = Vec::new();
                if st.x != 0 || st.y != 0 || st.z != 0 {
                    let target = add(block_vec(self.block(st.x, st.y, st.z)), [0.5; 3]);
                    specs.push(InputSpec {
                        look: Some(target),
                        ..InputSpec::default()
                    });
                }
                specs.push(InputSpec {
                    after: Some(Deferred::UseAir),
                    ..InputSpec::default()
                });
                specs.push(InputSpec {
                    flags: vec![InputFlag::Startusingitem],
                    ..InputSpec::default()
                });
                Ok((Vec::new(), specs))
            }
            Release | Consume => Ok((
                Vec::new(),
                vec![InputSpec {
                    after: Some(Deferred::Release {
                        consume: st.kind == Consume,
                    }),
                    ..InputSpec::default()
                }],
            )),
            _ => Ok((self.packets(st)?, Vec::new())),
        }
    }

    /// The per-tick movement packet followed by the packets the tick carries;
    /// vanilla clients send one every tick and BDS only simulates (e.g. picks
    /// up items for) players that do.
    pub fn tick(&mut self, spec: InputSpec) -> Vec<McpePacket> {
        if let Some(target) = spec.look {
            self.look_at(target);
        }
        self.pos = add(self.pos, spec.delta);
        self.tick += 1;
        let mut packet = PlayerAuthInputPacket {
            player_rotation: Vec2 {
                x: self.pitch,
                y: self.yaw,
            },
            position: v3(self.pos),
            player_head_rotation: self.yaw,
            input_data: spec.flags,
            input_mode: EnumsInputMode::Mouse,
            play_mode: EnumsClientPlayMode::Normal,
            new_interaction_model: EnumsNewInteractionModel::Crosshair,
            interact_rotation: Vec2 {
                x: self.pitch,
                y: self.yaw,
            },
            client_tick: PlayerInputTick {
                inputtick: self.tick,
            },
            pos_delta: v3(spec.delta),
            player_block_actions: spec.block_actions,
            ..Default::default()
        };
        if spec.delta[0] != 0.0 || spec.delta[2] != 0.0 {
            let forward = Vec2 { x: 0.0, y: 1.0 };
            packet.move_vector = forward.clone();
            packet.raw_move_vector = forward.clone();
            packet.analog_move_vector = forward;
            packet.input_data.push(InputFlag::Up);
        }
        let (pitch, yaw) = (
            f64::from(self.pitch).to_radians(),
            f64::from(self.yaw).to_radians(),
        );
        packet.camera_orientation = Vec3 {
            x: (-yaw.sin() * pitch.cos()) as f32,
            y: (-pitch.sin()) as f32,
            z: (yaw.cos() * pitch.cos()) as f32,
        };
        let mut batch = vec![McpePacket::from(packet)];
        if let Some(after) = spec.after {
            batch.push(self.deferred(after));
        }
        batch
    }

    fn deferred(&mut self, after: Deferred) -> McpePacket {
        let transaction = match after {
            Deferred::UseOnBlock { pos, face } => {
                // The clicked block's runtime ID is the one the server last
                // sent for that position.
                InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(Box::new(
                    ItemUseInventoryTransaction {
                        action_type: EnumsItemUseInventoryTransactionActionType::Place,
                        trigger_type: EnumsItemUseInventoryTransactionTriggerType::Playerinput,
                        position: block_pos(pos),
                        face: face as u8,
                        slot: self.selected,
                        hand: EnumsHandSlot::Mainhand,
                        item: self.held(),
                        from_position: v3(self.pos),
                        click_position: v3(face_offset(face)),
                        target_block_id: self.blocks.get(&pos).copied().unwrap_or(0),
                        client_interact_prediction:
                            EnumsItemUseInventoryTransactionPredictedResult::Success,
                        ..Default::default()
                    },
                ))
            }
            // Eating, drinking, drawing a bow, throwing: no block position,
            // face, click position or block runtime ID.
            Deferred::UseAir => InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(
                Box::new(ItemUseInventoryTransaction {
                    action_type: EnumsItemUseInventoryTransactionActionType::Use,
                    trigger_type: EnumsItemUseInventoryTransactionTriggerType::Playerinput,
                    slot: self.selected,
                    hand: EnumsHandSlot::Mainhand,
                    item: self.held(),
                    from_position: v3(self.pos),
                    client_interact_prediction:
                        EnumsItemUseInventoryTransactionPredictedResult::Success,
                    ..Default::default()
                }),
            ),
            Deferred::UseOnEntity { target, attack } => {
                InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(
                    ItemUseOnActorInventoryTransaction {
                        runtime_id: ActorRuntimeId {
                            actor_runtime_id: target,
                        },
                        action_type: if attack {
                            EnumsItemUseOnActorInventoryTransactionActionType::Attack
                        } else {
                            EnumsItemUseOnActorInventoryTransactionActionType::Interact
                        },
                        slot: self.selected,
                        item: self.held(),
                        from_position: v3(self.pos),
                        hit_position: Vec3 {
                            x: 0.0,
                            y: 1.0,
                            z: 0.0,
                        },
                        ..Default::default()
                    },
                )
            }
            Deferred::Release { consume } => {
                InventoryTransactionPacketTransaction::ItemReleaseInventoryTransaction(
                    ItemReleaseInventoryTransaction {
                        action_type: if consume {
                            EnumsItemReleaseInventoryTransactionActionType::Use
                        } else {
                            EnumsItemReleaseInventoryTransactionActionType::Release
                        },
                        slot: self.selected,
                        item: self.held(),
                        from_position: v3(self.pos),
                        ..Default::default()
                    },
                )
            }
        };
        McpePacket::from(InventoryTransactionPacket {
            transaction,
            ..Default::default()
        })
    }

    fn packets(&mut self, st: &Step) -> Result<Vec<McpePacket>> {
        use StepKind::*;
        match st.kind {
            Select => {
                self.selected = st.slot;
                Ok(vec![McpePacket::from(MobEquipmentPacket {
                    target_runtime_id: ActorRuntimeId {
                        actor_runtime_id: self.runtime_id,
                    },
                    item: self.held(),
                    slot: st.slot as u8,
                    selected_slot: st.slot as u8,
                    container_id: 0,
                })])
            }
            MoveItem | TakeCursor | PlaceCursor | Swap | Drop | DropCursor | Destroy => {
                self.stack_request(st).map(|packet| vec![packet])
            }
            // The vanilla client opens its own inventory with Interact
            // OpenInventory targeting itself.
            OpenInventory => Ok(vec![McpePacket::from(InteractPacket {
                action: EnumsInteractPacketPayloadAction::Openinventory,
                target_runtime_id: ActorRuntimeId {
                    actor_runtime_id: self.runtime_id,
                },
                position: None,
            })]),
            // Window 0 with type 0xff (none) as the vanilla client sends it.
            CloseInventory => Ok(vec![McpePacket::from(ContainerClosePacket {
                container_id: 0,
                container_type: 0xff,
                server_initiated_close: false,
            })]),
            Book => {
                let operation = match st.action.as_str() {
                    "replace" => BookEditPacketOperation::ReplacePage(BookEditActionReplacePage {
                        page_index: st.page,
                        page_text: st.text.clone().into(),
                        photo_name: "".into(),
                    }),
                    "add" => BookEditPacketOperation::AddPage(BookEditActionAddPage {
                        page_index: st.page,
                        page_text: st.text.clone().into(),
                        photo_name: "".into(),
                    }),
                    "delete" => BookEditPacketOperation::DeletePage(BookEditActionDeletePage {
                        page_index: st.page,
                    }),
                    "swap" => BookEditPacketOperation::SwapPages(BookEditActionSwapPages {
                        page_index: st.page,
                        swap_with_index: st.to,
                    }),
                    "sign" => BookEditPacketOperation::Finalize(BookEditActionFinalize {
                        title: st.title.clone().into(),
                        author: st.author.clone().into(),
                        xuid: "".into(),
                    }),
                    other => unreachable!("book action {other:?} passed validation"),
                };
                Ok(vec![McpePacket::from(BookEditPacket {
                    book_slot: st.slot,
                    operation,
                })])
            }
            ToggleCrafter => {
                let pos = self.block(st.x, st.y, st.z);
                Ok(vec![McpePacket::from(
                    PlayerToggleCrafterSlotRequestPacket {
                        pos_x: pos[0],
                        pos_y: pos[1],
                        pos_z: pos[2],
                        slot_index: st.slot as u8,
                        is_disabled: st.disabled,
                    },
                )])
            }
            MapInfo => {
                // The client asks for the data of the map it holds.
                let held = self.held();
                let id = map_uuid(&held).context("held item has no map_uuid")?;
                Ok(vec![McpePacket::from(MapInfoRequestPacket {
                    map_unique_id: ActorUniqueId {
                        actor_unique_id: id,
                    },
                    client_pixels_list: Vec::new(),
                })])
            }
            CloseContainer => {
                if self.window == 0 {
                    // The server refused to open it (e.g. an obstructed
                    // chest): the recording shows the refusal.
                    self.notes
                        .push("close_container: no block window is open".into());
                    return Ok(Vec::new());
                }
                let packet = ContainerClosePacket {
                    container_id: self.window,
                    container_type: self.window_type,
                    server_initiated_close: false,
                };
                self.window = 0;
                Ok(vec![McpePacket::from(packet)])
            }
            Craft => self.craft(st).map(|packet| vec![packet]),
            other => bail!("unsupported interaction {:?}", other.name()),
        }
    }

    /// A crafting grid or block window slot with the stack the server last
    /// reported there.
    fn container_slot(&self, slot: &SlotRef) -> (ItemStackRequestCerealSlotInfoData, Item) {
        if slot.container == "crafting" {
            let item = self.grid.get(&slot.slot).cloned().unwrap_or_default();
            return (
                slot_info(
                    EnumsContainerEnumName::Craftinginputcontainer,
                    slot.slot,
                    stack_id(&item),
                ),
                item,
            );
        }
        let item = self
            .window_items
            .get(&slot.slot)
            .cloned()
            .unwrap_or_default();
        (self.block_slot(&slot.container, slot.slot), item)
    }

    /// A slot of the open block window with the stack ID last reported there.
    fn block_slot(&self, name: &str, slot: i32) -> ItemStackRequestCerealSlotInfoData {
        let stack = self.window_items.get(&slot).map_or(0, stack_id);
        slot_info(container(name), slot, stack)
    }

    fn craft(&mut self, st: &Step) -> Result<McpePacket> {
        let mut recipe = CraftRecipe {
            network_id: st.recipe_network_id,
            output_count: 1,
            output: Item::default(),
        };
        if !st.recipe.is_empty() {
            recipe = self.recipes.get(&st.recipe).cloned().with_context(|| {
                format!("no recipe {:?} among {}", st.recipe, self.recipes.len())
            })?;
        }
        if st.action == "enchant" {
            recipe.network_id = *usize::try_from(st.option)
                .ok()
                .and_then(|option| self.enchant_options.get(option))
                .with_context(|| {
                    format!(
                        "enchanting option {} of {} offered",
                        st.option,
                        self.enchant_options.len()
                    )
                })?;
        }
        let mut consumes = Vec::new();
        let mut consumed = Vec::new();
        let first: Item;
        if st.consume.is_empty() {
            first = self
                .item(st.slot)
                .cloned()
                .with_context(|| format!("inventory slot {} is empty", st.slot))?;
            consumes.push(Action::ConsumeActionData(
                ItemStackRequestCerealConsumeActionData {
                    actiontype: EnumsItemStackRequestActionType::Consume,
                    amount: 1,
                    source: inventory_slot(st.slot, stack_id(&first)),
                },
            ));
        } else {
            let mut items = Vec::new();
            for slot in &st.consume {
                let (info, item) = self.container_slot(slot);
                ensure!(
                    count(&item) > 0,
                    "{} slot {} is empty",
                    slot.container,
                    slot.slot
                );
                let amount = slot.count.max(1);
                if amount >= i32::from(count(&item)) {
                    consumed.push((slot.container.clone(), slot.slot));
                }
                consumes.push(Action::ConsumeActionData(
                    ItemStackRequestCerealConsumeActionData {
                        actiontype: EnumsItemStackRequestActionType::Consume,
                        amount: amount as u8,
                        source: info,
                    },
                ));
                items.push(item);
            }
            first = items.swap_remove(0);
        }
        let request_id = self.next_request_id();
        let mut actions = Vec::new();
        let mut filter = Vec::new();
        let mut origin = EnumsTextProcessingEventOrigin::default();
        match st.action.as_str() {
            "optional" => {
                // The anvil sends the item name as the request's filter string.
                filter.push(st.text.clone().into());
                origin = EnumsTextProcessingEventOrigin::Anviltext;
                actions.push(Action::CraftRecipeOptionalActionData(
                    ItemStackRequestCerealCraftRecipeOptionalActionData {
                        actiontype: EnumsItemStackRequestActionType::Craftrecipeoptional,
                        recipe_net_id: TypedServerNetIdstructRecipeNetIdTag {
                            raw_id: recipe.network_id,
                        },
                        filtered_string_index: 0,
                    },
                ));
            }
            "grindstone" => {
                // The grindstone's "recipe" ID is the stack network ID of the
                // first input unless the step names one.
                let id = if recipe.network_id == 0 {
                    stack_id(&first)
                } else {
                    recipe.network_id as i32
                };
                actions.push(Action::CraftRepairAndDisenchantActionData(
                    ItemStackRequestCerealCraftRepairAndDisenchantActionData {
                        actiontype: EnumsItemStackRequestActionType::Craftrepairanddisenchant,
                        recipe_net_id: id,
                        numberofrequestedcrafts: 1,
                        repair_cost: st.cost,
                    },
                ));
            }
            "loom" => actions.push(Action::CraftLoomActionData(
                ItemStackRequestCerealCraftLoomActionData {
                    actiontype: EnumsItemStackRequestActionType::Craftloom,
                    pattern_name_id: st.pattern.clone().into(),
                    num_crafts: 1,
                },
            )),
            // Enchanting sends the chosen option's recipe network ID.
            _ => actions.push(Action::CraftRecipeActionData(
                ItemStackRequestCerealCraftRecipeActionData {
                    actiontype: EnumsItemStackRequestActionType::Craftrecipe,
                    recipe_net_id: TypedServerNetIdstructRecipeNetIdTag {
                        raw_id: recipe.network_id,
                    },
                    numberofrequestedcrafts: 1,
                },
            )),
        }
        if !st.result.is_empty() {
            actions.push(Action::CraftResultsActionData(
                ItemStackRequestCerealCraftResultsActionData {
                    actiontype: EnumsItemStackRequestActionType::Craftresults,
                    craft_results: vec![ItemStackRequestCerealNetworkItemInstanceDescriptorData {
                        item_descriptor:
                            ItemStackRequestCerealNetworkItemInstanceDescriptorDataItemDescriptor::ItemNameDescriptorData(
                                ItemStackRequestCerealItemNameDescriptorData {
                                    descriptor_type: EnumsItemStackRequestCerealItemDescriptorType::Itemname,
                                    full_name: st.result.clone().into(),
                                    aux_value: 0,
                                },
                            ),
                        stacksize: recipe.output_count,
                        block_runtime_id: 0,
                        user_data_buffer: user_data(
                            st.result_nbt.as_ref(),
                            st.result == "minecraft:shield",
                        )?,
                    }],
                    num_crafts: 1,
                },
            ));
        }
        actions.extend(consumes);
        // The crafted stack is addressed by the request ID until placed; it
        // may merge into a matching stack already in the destination.
        let source = slot_info(
            EnumsContainerEnumName::Createdoutputcontainer,
            50,
            request_id,
        );
        let mut output = recipe.output.clone();
        if output.id == 0 {
            // Enchanting, the anvil and the grindstone output the input.
            output = first;
        }
        let destination = if !st.to_container.is_empty() {
            let mut dest = self.window_items.get(&st.to).cloned().unwrap_or_default();
            let mut info = self.block_slot(&st.to_container, st.to);
            if consumed.contains(&(st.to_container.clone(), st.to)) {
                dest = Item::default();
                info.net_id_variant = 0;
            }
            predict(&mut dest, &output);
            self.window_items.insert(st.to, dest);
            info
        } else {
            let mut dest = self.inventory.get(&st.to).cloned().unwrap_or_default();
            let info = inventory_slot(st.to, stack_id(&dest));
            predict(&mut dest, &output);
            self.inventory.insert(st.to, dest);
            info
        };
        actions.push(Action::TakeActionData(
            ItemStackRequestCerealTakeActionData {
                actiontype: EnumsItemStackRequestActionType::Take,
                amount: recipe.output_count as u8,
                source,
                destination,
            },
        ));
        Ok(McpePacket::from(ItemStackRequestPacket {
            requests: vec![ItemStackRequestPacketDataRequestData {
                client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 {
                    id: request_id,
                },
                actions,
                strings_to_filter: filter,
                strings_to_filter_origin: origin,
            }],
        }))
    }

    /// The ItemStackRequest of an inventory step; predicts the destination
    /// item type. Slots 0-35 are the player inventory; the cursor is the
    /// PlayerUI container's slot 0.
    fn stack_request(&mut self, st: &Step) -> Result<McpePacket> {
        use StepKind::*;
        let mut source = self.inventory.get(&st.slot).cloned().unwrap_or_default();
        if matches!(st.kind, PlaceCursor | DropCursor) {
            source = self.cursor.clone();
        }
        if !st.from_container.is_empty() {
            source = self.window_items.get(&st.slot).cloned().unwrap_or_default();
        }
        ensure!(
            count(&source) > 0,
            "{}: source slot is empty",
            st.kind.name()
        );
        let source_id = if st.stale {
            STALE_STACK_ID
        } else {
            stack_id(&source)
        };
        let amount = st.count as u8;
        let action = match st.kind {
            MoveItem => {
                let mut from = inventory_slot(st.slot, source_id);
                if !st.from_container.is_empty() {
                    from = self.block_slot(&st.from_container, st.slot);
                    from.net_id_variant = source_id;
                }
                let destination = match st.to_container.as_str() {
                    "armor" | "offhand" => other_slot(&st.to_container, st.to),
                    "crafting" => {
                        let mut dest = self.grid.get(&st.to).cloned().unwrap_or_default();
                        let info = slot_info(
                            EnumsContainerEnumName::Craftinginputcontainer,
                            st.to,
                            stack_id(&dest),
                        );
                        predict(&mut dest, &source);
                        self.grid.insert(st.to, dest);
                        info
                    }
                    "" => {
                        let mut dest = self.inventory.get(&st.to).cloned().unwrap_or_default();
                        let info = inventory_slot(st.to, stack_id(&dest));
                        predict(&mut dest, &source);
                        self.inventory.insert(st.to, dest);
                        info
                    }
                    name => {
                        let info = self.block_slot(name, st.to);
                        let mut dest = self.window_items.get(&st.to).cloned().unwrap_or_default();
                        predict(&mut dest, &source);
                        self.window_items.insert(st.to, dest);
                        info
                    }
                };
                Action::TakeActionData(ItemStackRequestCerealTakeActionData {
                    actiontype: EnumsItemStackRequestActionType::Take,
                    amount,
                    source: from,
                    destination,
                })
            }
            TakeCursor => {
                let destination = cursor_slot(stack_id(&self.cursor));
                predict(&mut self.cursor, &source);
                Action::TakeActionData(ItemStackRequestCerealTakeActionData {
                    actiontype: EnumsItemStackRequestActionType::Take,
                    amount,
                    source: inventory_slot(st.slot, source_id),
                    destination,
                })
            }
            PlaceCursor => {
                let destination = if st.to_container.is_empty() {
                    let mut dest = self.inventory.get(&st.to).cloned().unwrap_or_default();
                    let info = inventory_slot(st.to, stack_id(&dest));
                    predict(&mut dest, &source);
                    self.inventory.insert(st.to, dest);
                    info
                } else {
                    other_slot(&st.to_container, st.to)
                };
                Action::PlaceActionData(ItemStackRequestCerealPlaceActionData {
                    actiontype: EnumsItemStackRequestActionType::Place,
                    amount,
                    source: cursor_slot(source_id),
                    destination,
                })
            }
            Swap => {
                let dest = self.inventory.get(&st.to).cloned().unwrap_or_default();
                let action = Action::SwapActionData(ItemStackRequestCerealSwapActionData {
                    actiontype: EnumsItemStackRequestActionType::Swap,
                    source: inventory_slot(st.slot, source_id),
                    destination: inventory_slot(st.to, stack_id(&dest)),
                });
                self.inventory.insert(st.slot, dest);
                self.inventory.insert(st.to, source);
                action
            }
            Drop => Action::DropActionData(ItemStackRequestCerealDropActionData {
                actiontype: EnumsItemStackRequestActionType::Drop,
                amount,
                source: inventory_slot(st.slot, source_id),
                randomly: false,
            }),
            DropCursor => Action::DropActionData(ItemStackRequestCerealDropActionData {
                actiontype: EnumsItemStackRequestActionType::Drop,
                amount,
                source: cursor_slot(source_id),
                randomly: false,
            }),
            Destroy => Action::DestroyActionData(ItemStackRequestCerealDestroyActionData {
                actiontype: EnumsItemStackRequestActionType::Destroy,
                amount,
                source: inventory_slot(st.slot, source_id),
            }),
            other => unreachable!("{other:?} is not an inventory step"),
        };
        let id = self.next_request_id();
        Ok(request(id, vec![action]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_with(items: &[(i32, i16, u16, i32)]) -> Session {
        let mut session = Session::new(1, &Vec3::default());
        for &(slot, id, stacksize, stack) in items {
            session.inventory.insert(
                slot,
                Item {
                    id,
                    stacksize,
                    net_id_variant: Some(stack),
                    ..Default::default()
                },
            );
        }
        session
    }

    #[test]
    fn move_item_addresses_tracked_stacks_and_numbers_requests_like_the_client() {
        let mut session = session_with(&[(0, 5, 4, 7)]);
        let step = Step {
            kind: StepKind::MoveItem,
            slot: 0,
            to: 12,
            count: 2,
            ..Step::default()
        };
        let packet = session.stack_request(&step).unwrap();
        let McpePacketData::ItemStackRequestPacket(request) = packet.data else {
            panic!("not a stack request")
        };
        let request = &request.requests[0];
        assert_eq!(request.client_request_id.id, -1);
        let Action::TakeActionData(take) = &request.actions[0] else {
            panic!("not a take")
        };
        assert_eq!(take.source.net_id_variant, 7);
        assert_eq!(
            take.destination.fullcontainername.container_name,
            EnumsContainerEnumName::Inventorycontainer
        );
        // The destination now holds the predicted type with no stack yet.
        assert_eq!(
            (session.inventory[&12].id, count(&session.inventory[&12])),
            (5, 0)
        );
        assert_eq!(session.next_request_id(), -3);
    }

    #[test]
    fn use_queues_the_transaction_then_start_using_item() {
        let mut session = session_with(&[(0, 9, 1, 3)]);
        let (packets, specs) = session
            .interaction(&Step {
                kind: StepKind::Use,
                ..Step::default()
            })
            .unwrap();
        assert!(packets.is_empty());
        assert_eq!(specs.len(), 2);
        let batch = session.tick(specs[0].clone());
        assert!(matches!(
            &batch[1].data,
            McpePacketData::InventoryTransactionPacket(p)
                if matches!(&p.transaction, InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(t)
                    if t.action_type == EnumsItemUseInventoryTransactionActionType::Use && t.item.net_id_variant == Some(3))
        ));
        let batch = session.tick(specs[1].clone());
        assert!(matches!(
            &batch[0].data,
            McpePacketData::PlayerAuthInputPacket(p)
                if p.input_data == [InputFlag::Startusingitem] && p.client_tick.inputtick == 2
        ));
    }

    #[test]
    fn result_nbt_is_little_endian_user_data() {
        let nbt = serde_json::json!({"Damage": 9})
            .as_object()
            .cloned()
            .unwrap();
        let data = user_data(Some(&nbt), false).unwrap();
        assert_eq!(&data[..3], &[0xff, 0xff, 1]);
        assert_eq!(&data[data.len() - 8..], &[0; 8]);
        assert_eq!(user_data(None, true).unwrap().len(), 2 + 8 + 8);
    }
}
