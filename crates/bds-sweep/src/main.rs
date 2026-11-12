//! Scripted player for the BDS sweep harness (`tools/bds-sweep`).
//!
//! Joins an offline BDS over loopback and behaves like an idle vanilla
//! client so the server simulates it: `PlayerAuthInput` every 50 ms at the
//! position and rotation the server last set, latency probes answered,
//! respawns and dimension changes acknowledged. The Endstone sweep plugin
//! drives experiments server-side through this player (held items, armour
//! slots, `/execute as`) and sends it orders as chat messages:
//!
//! * `bds-sweep:mine X Y Z FACE PREDICT`: start destroying the block (block
//!   action `StartDestroyBlock`), then `ContinueDestroyBlock` every tick; when
//!   `PREDICT` > 0, send `PredictDestroyBlock` that many ticks after the start
//!   and stop;
//! * `bds-sweep:stop`: abort the current mining (`AbortDestroyBlock`);
//! * `bds-sweep:pause TICKS`: send no `PlayerAuthInput` for that many ticks
//!   (a frozen client), then carry on where it stopped;
//! * `bds-sweep:attack RUNTIME_ID`: attack that actor with the held item
//!   (`InventoryTransaction` use-item-on-actor, hotbar slot 0 as last sent
//!   by the server).
//! * `bds-sweep:drop hotbar SLOT COUNT`: closed-screen Q (`COUNT=1`) or
//!   ctrl-Q (`COUNT=stack size`) from hotbar index 0..8;
//! * `bds-sweep:drop inventory SLOT COUNT`: open the native inventory, wait
//!   for `ContainerOpen`, then drop from main-inventory index 0..35;
//! * `bds-sweep:close-inventory`: close the inventory opened by this client.
//! * `bds-sweep:walk DX DY DZ DYAW TICKS FLAGS [DPITCH]`: for the next
//!   `TICKS` inputs, move the reported position by (`DX`, `DY`, `DZ`)
//!   thousandths of a block and turn the yaw (and head yaw) by `DYAW` and the
//!   pitch by `DPITCH` thousandths of a degree each input; `FLAGS` bit 0 also
//!   reports forward movement (`Up`, move vector (0, 1)), bit 1 reports
//!   `VerticalCollision` (standing on the ground);
//! * `bds-sweep:press FLAG...`: add these `PlayerAuthInput` input flags
//!   (lower-case names, `startsneaking`) to the next input only;
//! * `bds-sweep:hold FLAG...` / `bds-sweep:release FLAG...`: add them to
//!   every input from the next one on, until released;
//! * `bds-sweep:use`: use the held item in the air (`InventoryTransaction`
//!   item use, hotbar slot as last synchronized); BDS rejects it for armour
//!   and resends the whole inventory, which refreshes stack net IDs;
//! * `bds-sweep:quit`: close the connection and exit (BDS keeps the player
//!   until the connection times out, about 10 s later);
//! * `bds-sweep:equip-head SLOT`: open the inventory (as an inventory drop
//!   does) and move one item from hotbar index SLOT to the head armour slot
//!   (`ItemStackRequest` place).
//! * `bds-sweep:use X Y Z FACE CX CY CZ TARGET`: use the held stack on the
//!   block at X Y Z (`InventoryTransaction` item use `Place`, the block
//!   click a vanilla client sends to place a block), clicking face FACE at
//!   (CX, CY, CZ)/1000 within the block; TARGET is the clicked block's
//!   network id.
//!
//! `--script FILE` drives the client without the plugin, against any
//! server: one `TICK ORDER` line per step, run when the client's input tick
//! reaches `TICK`; `ORDER` is an order above without the `bds-sweep:` prefix,
//! a slash command (sent as `CommandRequest`, so the player must be an
//! operator), or `exit` to leave. Blank lines and `#` comments are skipped.
//!
//! Drop orders require server-synchronized nonempty slots with stack net IDs.
//! Inventory drops leave the screen open; hotbar drops close it first.
//! `--capture-dir DIR` records the entire session, including login and both
//! directions, in `DIR/packets.jsonl` using the client-diff wire recorder.
//! Packets are flushed as recorded and again on session completion/error.
//!
//! Exits successfully when the server closes the session.

use anyhow::{Context, Result, ensure};
use clap::Parser;
use client_diff::recording::{self, Recorder};
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::{
    BedrockStream, Client, ClientConfig, GameData, JolyneError, Play,
    stream::{Handshake, transport::Transport},
    valentine::{
        ActorRuntimeId, BlockPos, CerealizerNetworkItemStackDescriptorSerializedData,
        CommandOriginDatajson, CommandRequestPacket, ContainerClosePacket, EnumsClientPlayMode,
        EnumsContainerEnumName, EnumsInputMode, EnumsInteractPacketPayloadAction,
        EnumsItemStackRequestActionType, EnumsItemUseInventoryTransactionActionType,
        EnumsItemUseInventoryTransactionClientCooldownState,
        EnumsItemUseInventoryTransactionPredictedResult,
        EnumsItemUseInventoryTransactionTriggerType,
        EnumsItemUseOnActorInventoryTransactionActionType, EnumsNewInteractionModel,
        EnumsPlayerActionType, EnumsPlayerAuthInputPacketPayloadInputData, EnumsPlayerRespawnState,
        FullContainerName, InteractPacket, InventoryTransactionPacket,
        InventoryTransactionPacketTransaction, ItemStackRequestCerealDropActionData,
        ItemStackRequestCerealPlaceActionData, ItemStackRequestCerealSlotInfoData,
        ItemStackRequestPacket, ItemStackRequestPacketDataRequestData,
        ItemStackRequestPacketDataRequestDataActionsItem, ItemUseInventoryTransaction,
        ItemUseOnActorInventoryTransaction, McpePacket, McpePacketData, McpePacketName,
        MobEquipmentPacket, PlayerActionPacket, PlayerAuthInputPacket, PlayerBlockActionData,
        PlayerInputTick, RespawnPacket, TypedClientNetIdstructItemStackRequestIdTagint32T0, Vec2,
        Vec3,
    },
};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::info;

#[derive(Parser, Debug)]
#[command(about = "Idle scripted player for BDS sweeps")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:19132")]
    addr: SocketAddr,
    #[arg(long, default_value = "BDSSweep")]
    name: String,
    /// Login timeout in seconds
    #[arg(long, default_value = "120")]
    timeout: u64,
    /// Record both packet directions (login included) to DIR/packets.jsonl.
    #[arg(long)]
    capture_dir: Option<PathBuf>,
    /// Log in with this identity UUID (a random one otherwise), so a
    /// rejoining player keeps its saved data.
    #[arg(long)]
    uuid: Option<uuid::Uuid>,
    /// Run the timed orders and commands of this file (see the crate docs).
    #[arg(long)]
    script: Option<PathBuf>,
}

/// A mining order in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Mining {
    pos: [i32; 3],
    face: i32,
    /// Ticks after the start at which to predict the break; 0 = never.
    predict_after: u32,
    elapsed: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DropSource {
    Hotbar,
    Inventory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DropOrder {
    source: DropSource,
    slot: u8,
    count: u8,
}

/// A block click with the held stack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UseOrder {
    pos: [i32; 3],
    face: u8,
    /// Click position within the block, in thousandths.
    click: [i32; 3],
    /// Network id of the clicked block.
    target: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Order {
    Mine(Mining),
    Stop,
    /// Release the button: `StopDestroyBlock` (the default position, as a
    /// client sends it) and no more mining.
    Release,
    /// Send no input for this many ticks.
    Pause(u32),
    /// Send this many more inputs right after this tick's input, at once (a
    /// client running faster than real time).
    Burst(u32),
    /// Attack the actor with this runtime id with the held item.
    Attack(u64),
    Drop(DropOrder),
    CloseInventory,
    Walk(Walk),
    /// Input flags for the next `PlayerAuthInput` only.
    Press(Vec<EnumsPlayerAuthInputPacketPayloadInputData>),
    /// Input flags for every `PlayerAuthInput` until released.
    Hold(Vec<EnumsPlayerAuthInputPacketPayloadInputData>),
    ReleaseFlags(Vec<EnumsPlayerAuthInputPacketPayloadInputData>),
    /// Use the held item in the air.
    UseItem,
    /// Leave the server.
    Quit,
    /// Move the item in this hotbar slot to the head armour slot.
    EquipHead(u8),
    /// Use the held stack on a block face.
    Use(UseOrder),
}

/// A scripted walk: per-input position and yaw steps, in thousandths of a
/// block and of a degree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Walk {
    step: [i64; 3],
    turn: i64,
    tilt: i64,
    remaining: u32,
    forward: bool,
    grounded: bool,
}

/// The input flags the `press`, `hold` and `release` orders can name.
fn input_flag(name: &str) -> Option<EnumsPlayerAuthInputPacketPayloadInputData> {
    use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
    Some(match name {
        "jumping" => Flag::Jumping,
        "startjumping" => Flag::Startjumping,
        "sneaking" => Flag::Sneaking,
        "sneakdown" => Flag::Sneakdown,
        "startsneaking" => Flag::Startsneaking,
        "stopsneaking" => Flag::Stopsneaking,
        "sprinting" => Flag::Sprinting,
        "sprintdown" => Flag::Sprintdown,
        "startsprinting" => Flag::Startsprinting,
        "stopsprinting" => Flag::Stopsprinting,
        "startswimming" => Flag::Startswimming,
        "stopswimming" => Flag::Stopswimming,
        "startcrawling" => Flag::Startcrawling,
        "stopcrawling" => Flag::Stopcrawling,
        "startgliding" => Flag::Startgliding,
        "stopgliding" => Flag::Stopgliding,
        "startflying" => Flag::Startflying,
        "stopflying" => Flag::Stopflying,
        "up" => Flag::Up,
        _ => return None,
    })
}

/// The input flags named by the rest of an order: each word's leading
/// letters (the order may arrive inside escaped JSON text), at least one.
fn input_flags<'a>(
    words: impl Iterator<Item = &'a str>,
) -> Option<Vec<EnumsPlayerAuthInputPacketPayloadInputData>> {
    let flags: Vec<_> = words
        .map(|word| {
            let name: String = word
                .chars()
                .take_while(char::is_ascii_alphanumeric)
                .collect();
            input_flag(&name)
        })
        .take_while(Option::is_some)
        .flatten()
        .collect();
    (!flags.is_empty()).then_some(flags)
}

/// The first `n` numbers of an order. The order arrives inside the Debug
/// form of the text body, so the last number is followed by the closing
/// quote.
fn numbers<'a>(words: impl Iterator<Item = &'a str>, n: usize) -> Option<Vec<i64>> {
    let numbers: Vec<i64> = words
        .take(n)
        .map(|word| word.trim_end_matches(|c: char| !c.is_ascii_digit()).parse())
        .collect::<Result<_, _>>()
        .ok()?;
    (numbers.len() == n).then_some(numbers)
}

/// Parses a plugin order from a chat message (anywhere in the text).
fn parse_order(text: &str) -> Option<Order> {
    let rest = &text[text.find("bds-sweep:")? + "bds-sweep:".len()..];
    let mut words = rest.split_whitespace();
    match words.next()? {
        "stop" | "stop\"" => Some(Order::Stop),
        // Bare: release the mining button; with flags: stop holding them.
        word if word.starts_with("release") && !word[7..].starts_with(char::is_alphanumeric) => {
            Some(match input_flags(words) {
                Some(flags) => Order::ReleaseFlags(flags),
                None => Order::Release,
            })
        }
        "pause" => Some(Order::Pause(u32::try_from(numbers(words, 1)?[0]).ok()?)),
        "burst" => Some(Order::Burst(u32::try_from(numbers(words, 1)?[0]).ok()?)),
        "mine" => {
            let numbers = numbers(words, 5)?;
            let [x, y, z, face, predict_after] = numbers[..] else {
                return None;
            };
            Some(Order::Mine(Mining {
                pos: [x as i32, y as i32, z as i32],
                face: face as i32,
                predict_after: u32::try_from(predict_after).ok()?,
                elapsed: 0,
            }))
        }
        "attack" => Some(Order::Attack(u64::try_from(numbers(words, 1)?[0]).ok()?)),
        "drop" => {
            let source = match words.next()? {
                "hotbar" => DropSource::Hotbar,
                "inventory" => DropSource::Inventory,
                _ => return None,
            };
            let values = numbers(words, 2)?;
            Some(Order::Drop(DropOrder {
                source,
                slot: u8::try_from(values[0]).ok()?,
                count: u8::try_from(values[1]).ok()?,
            }))
        }
        "close-inventory" | "close-inventory\"" => Some(Order::CloseInventory),
        "walk" => {
            let values = numbers(words.by_ref(), 6)?;
            // The optional pitch step; whatever follows the order otherwise.
            let tilt = words
                .next()
                .and_then(|word| numbers(std::iter::once(word), 1))
                .map_or(0, |tilt| tilt[0]);
            Some(Order::Walk(Walk {
                step: [values[0], values[1], values[2]],
                turn: values[3],
                tilt,
                remaining: u32::try_from(values[4]).ok()?,
                forward: values[5] & 1 != 0,
                grounded: values[5] & 2 != 0,
            }))
        }
        "press" => Some(Order::Press(input_flags(words)?)),
        "hold" => Some(Order::Hold(input_flags(words)?)),
        "use" => {
            let mut words = words.peekable();
            if words.peek().is_none() {
                return Some(Order::UseItem);
            }
            let numbers = numbers(words, 8)?;
            let [x, y, z, face, cx, cy, cz, target] = numbers[..] else {
                return None;
            };
            Some(Order::Use(UseOrder {
                pos: [x as i32, y as i32, z as i32],
                face: u8::try_from(face).ok()?,
                click: [cx as i32, cy as i32, cz as i32],
                target: u32::try_from(target).ok()?,
            }))
        }
        word if word.starts_with("use") && !word[3..].starts_with(char::is_alphanumeric) => {
            Some(Order::UseItem)
        }
        word if word.starts_with("quit") && !word[4..].starts_with(char::is_alphanumeric) => {
            Some(Order::Quit)
        }
        "equip-head" => Some(Order::EquipHead(u8::try_from(numbers(words, 1)?[0]).ok()?)),
        _ => None,
    }
}

/// One step of a `--script` file.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    Order(Order),
    /// A slash command, sent as `CommandRequest`.
    Command(String),
    Exit,
}

/// `TICK ORDER` lines, sorted by tick (stable for equal ticks).
fn parse_script(text: &str) -> Result<Vec<(u64, Step)>> {
    let mut steps = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let context = || format!("script line {}: {line}", index + 1);
        let (tick, rest) = line.split_once(char::is_whitespace).with_context(context)?;
        let tick: u64 = tick.parse().with_context(context)?;
        let rest = rest.trim();
        let step = if rest == "exit" {
            Step::Exit
        } else if rest.starts_with('/') {
            Step::Command(rest.to_owned())
        } else {
            Step::Order(parse_order(&format!("bds-sweep:{rest}")).with_context(context)?)
        };
        steps.push((tick, step));
    }
    steps.sort_by_key(|(tick, _)| *tick);
    Ok(steps)
}

/// A slash command from the player, as bds-extractor sends them.
fn command_request(command: &str) -> McpePacket {
    McpePacket::from(CommandRequestPacket {
        command: command.into(),
        origin: CommandOriginDatajson {
            type_: "player".into(),
            ..Default::default()
        },
        is_internal: false,
        version: "latest".into(),
    })
}

/// The own player's state needed to tick like a vanilla client.
struct Player {
    runtime_id: u64,
    position: Vec3,
    /// Pitch (x) and yaw (y), as the server last set them.
    rotation: Vec2,
    tick: u64,
    /// Ticks left without input (`Order::Pause`).
    paused: u32,
    /// Extra inputs to send right after the next one (`Order::Burst`).
    burst: u32,
    mining: Option<Mining>,
    /// A block action to send with the next input (abort or release).
    pending: Option<PlayerBlockActionData>,
    /// Authoritative native main inventory, including hotbar indices 0..8.
    inventory: [Option<CerealizerNetworkItemStackDescriptorSerializedData>; 36],
    selected_slot: u8,
    /// Server-confirmed inventory window (id, type), never a guessed window.
    inventory_window: Option<(u8, u8)>,
    pending_drop: Option<DropOrder>,
    next_request_id: i32,
    walk: Option<Walk>,
    /// Input flags sent with every input (`hold`) and with the next one
    /// only (`press`).
    held: Vec<EnumsPlayerAuthInputPacketPayloadInputData>,
    pressed: Vec<EnumsPlayerAuthInputPacketPayloadInputData>,
    /// A `quit` order arrived: close the session.
    quit: bool,
    /// Hotbar slot to equip on the head once the inventory opens.
    pending_equip: Option<u8>,
}

fn block_action(action: EnumsPlayerActionType, mining: &Mining) -> PlayerBlockActionData {
    PlayerBlockActionData {
        player_action_type: action,
        position: BlockPos {
            x: mining.pos[0],
            y: mining.pos[1],
            z: mining.pos[2],
        },
        facing: mining.face,
    }
}

impl Player {
    /// The block actions of this tick's input, advancing the mining order.
    fn block_actions(&mut self) -> Vec<PlayerBlockActionData> {
        let mut actions: Vec<_> = self.pending.take().into_iter().collect();
        if let Some(mining) = &mut self.mining {
            let action = if mining.elapsed == 0 {
                EnumsPlayerActionType::Startdestroyblock
            } else if mining.predict_after != 0 && mining.elapsed >= mining.predict_after {
                EnumsPlayerActionType::Predictdestroyblock
            } else {
                EnumsPlayerActionType::Continuedestroyblock
            };
            actions.push(block_action(action, mining));
            mining.elapsed += 1;
            if action == EnumsPlayerActionType::Predictdestroyblock {
                self.mining = None;
            }
        }
        actions
    }

    /// Applies an order; returns packets to send right away.
    fn order(&mut self, order: Order) -> Result<Vec<McpePacket>> {
        match order {
            Order::Mine(mining) => self.mining = Some(mining),
            Order::Stop => {
                if let Some(mining) = self.mining.take() {
                    self.pending = Some(block_action(
                        EnumsPlayerActionType::Abortdestroyblock,
                        &mining,
                    ));
                }
            }
            Order::Release => {
                if self.mining.take().is_some() {
                    self.pending = Some(PlayerBlockActionData {
                        player_action_type: EnumsPlayerActionType::Stopdestroyblock,
                        position: BlockPos::default(),
                        facing: 0,
                    });
                }
            }
            Order::Pause(ticks) => self.paused = ticks,
            Order::Burst(inputs) => self.burst = inputs,
            Order::Attack(target) => return Ok(vec![self.attack(target)]),
            Order::Use(order) => return Ok(vec![self.use_on_block(order)]),
            Order::Drop(drop) => return self.drop_order(drop),
            Order::Walk(walk) => self.walk = Some(walk),
            Order::CloseInventory => {
                ensure!(
                    self.pending_drop.is_none(),
                    "inventory open still pending for a drop"
                );
                return Ok(self.close_inventory().into_iter().collect());
            }
            Order::Press(flags) => self.pressed.extend(flags),
            Order::Hold(flags) => {
                for flag in flags {
                    if !self.held.contains(&flag) {
                        self.held.push(flag);
                    }
                }
            }
            Order::ReleaseFlags(flags) => self.held.retain(|flag| !flags.contains(flag)),
            Order::UseItem => return Ok(vec![self.use_item()]),
            Order::Quit => self.quit = true,
            Order::EquipHead(slot) => {
                if self.inventory_window.is_some() {
                    return Ok(vec![self.equip_request(slot)?]);
                }
                self.pending_equip = Some(slot);
                return Ok(vec![McpePacket::from(InteractPacket {
                    action: EnumsInteractPacketPayloadAction::Openinventory,
                    target_runtime_id: ActorRuntimeId {
                        actor_runtime_id: self.runtime_id,
                    },
                    ..Default::default()
                })]);
            }
        }
        Ok(Vec::new())
    }

    /// `InventoryTransaction` item use in the air with the held slot.
    fn use_item(&self) -> McpePacket {
        McpePacket::from(InventoryTransactionPacket {
            transaction: InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(
                Box::new(ItemUseInventoryTransaction {
                    action_type: EnumsItemUseInventoryTransactionActionType::Use,
                    trigger_type: EnumsItemUseInventoryTransactionTriggerType::Playerinput,
                    face: 255,
                    slot: i32::from(self.selected_slot),
                    item: self.inventory[usize::from(self.selected_slot)]
                        .clone()
                        .unwrap_or_default(),
                    from_position: self.position.clone(),
                    client_interact_prediction:
                        EnumsItemUseInventoryTransactionPredictedResult::Success,
                    ..Default::default()
                }),
            ),
            ..Default::default()
        })
    }

    /// `InventoryTransaction` use-item-on-actor `Attack` with the held slot.
    fn attack(&self, target: u64) -> McpePacket {
        McpePacket::from(InventoryTransactionPacket {
            transaction: InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(
                ItemUseOnActorInventoryTransaction {
                    runtime_id: ActorRuntimeId {
                        actor_runtime_id: target,
                    },
                    action_type: EnumsItemUseOnActorInventoryTransactionActionType::Attack,
                    slot: 0,
                    item: self.inventory[0].clone().unwrap_or_default(),
                    from_position: self.position.clone(),
                    ..Default::default()
                },
            ),
            ..Default::default()
        })
    }

    /// `InventoryTransaction` item use `Place` with the selected hotbar
    /// stack, as a vanilla client sends when it places a block.
    fn use_on_block(&self, order: UseOrder) -> McpePacket {
        let [cx, cy, cz] = order.click.map(|v| v as f32 / 1000.0);
        McpePacket::from(InventoryTransactionPacket {
            transaction: InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(
                Box::new(ItemUseInventoryTransaction {
                    action_type: EnumsItemUseInventoryTransactionActionType::Place,
                    trigger_type: EnumsItemUseInventoryTransactionTriggerType::Playerinput,
                    position: BlockPos {
                        x: order.pos[0],
                        y: order.pos[1],
                        z: order.pos[2],
                    },
                    face: order.face,
                    slot: i32::from(self.selected_slot),
                    hand: jolyne::valentine::EnumsHandSlot::Mainhand,
                    item: self.inventory[usize::from(self.selected_slot)]
                        .clone()
                        .unwrap_or_default(),
                    from_position: self.position.clone(),
                    click_position: Vec3 {
                        x: cx,
                        y: cy,
                        z: cz,
                    },
                    target_block_id: order.target,
                    client_interact_prediction:
                        EnumsItemUseInventoryTransactionPredictedResult::Success,
                    client_cooldown_state: EnumsItemUseInventoryTransactionClientCooldownState::Off,
                    ..Default::default()
                }),
            ),
            ..Default::default()
        })
    }

    fn drop_stack(
        &self,
        drop: DropOrder,
    ) -> Result<&CerealizerNetworkItemStackDescriptorSerializedData> {
        let limit = match drop.source {
            DropSource::Hotbar => 9,
            DropSource::Inventory => 36,
        };
        ensure!(
            drop.slot < limit,
            "drop {:?} slot {} out of range",
            drop.source,
            drop.slot
        );
        let stack = self.inventory[usize::from(drop.slot)]
            .as_ref()
            .with_context(|| format!("drop slot {} has not been synchronized", drop.slot))?;
        ensure!(
            stack.id != 0 && stack.stacksize > 0,
            "drop slot {} is empty",
            drop.slot
        );
        ensure!(
            drop.count > 0 && u16::from(drop.count) <= stack.stacksize,
            "drop count {} exceeds slot {} stack size {} or is zero",
            drop.count,
            drop.slot,
            stack.stacksize
        );
        ensure!(
            stack.net_id_variant.is_some_and(|id| id > 0),
            "drop slot {} has no authoritative positive stack net ID",
            drop.slot
        );
        Ok(stack)
    }

    fn close_inventory(&mut self) -> Option<McpePacket> {
        self.inventory_window
            .take()
            .map(|(container_id, container_type)| {
                McpePacket::from(ContainerClosePacket {
                    container_id,
                    container_type,
                    server_initiated_close: false,
                })
            })
    }

    fn drop_order(&mut self, drop: DropOrder) -> Result<Vec<McpePacket>> {
        ensure!(
            self.pending_drop.is_none(),
            "another drop is waiting for ContainerOpen"
        );
        self.drop_stack(drop)?;
        let mut packets = Vec::new();
        match drop.source {
            DropSource::Hotbar => {
                packets.extend(self.close_inventory());
                if self.selected_slot != drop.slot {
                    packets.push(McpePacket::from(MobEquipmentPacket {
                        target_runtime_id: ActorRuntimeId {
                            actor_runtime_id: self.runtime_id,
                        },
                        item: self.drop_stack(drop)?.clone(),
                        slot: drop.slot,
                        selected_slot: drop.slot,
                        container_id: 0,
                    }));
                    self.selected_slot = drop.slot;
                }
                packets.push(self.drop_request(drop)?);
            }
            DropSource::Inventory if self.inventory_window.is_some() => {
                packets.push(self.drop_request(drop)?);
            }
            DropSource::Inventory => {
                self.pending_drop = Some(drop);
                packets.push(McpePacket::from(InteractPacket {
                    action: EnumsInteractPacketPayloadAction::Openinventory,
                    target_runtime_id: ActorRuntimeId {
                        actor_runtime_id: self.runtime_id,
                    },
                    ..Default::default()
                }));
            }
        }
        Ok(packets)
    }

    fn drop_request(&mut self, drop: DropOrder) -> Result<McpePacket> {
        let net_id_variant = self
            .drop_stack(drop)?
            .net_id_variant
            .context("missing stack net ID")?;
        let request_id = self.next_request_id;
        self.next_request_id = request_id
            .checked_sub(2)
            .context("stack request ID exhausted")?;
        info!(
            ?drop,
            request_id, net_id_variant, "sending directional player drop"
        );
        Ok(McpePacket::from(ItemStackRequestPacket {
            requests: vec![ItemStackRequestPacketDataRequestData {
                client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 {
                    id: request_id,
                },
                actions: vec![
                    ItemStackRequestPacketDataRequestDataActionsItem::DropActionData(
                        ItemStackRequestCerealDropActionData {
                            actiontype: EnumsItemStackRequestActionType::Drop,
                            amount: drop.count,
                            source: ItemStackRequestCerealSlotInfoData {
                                fullcontainername: FullContainerName {
                                    container_name: match drop.source {
                                        DropSource::Hotbar => {
                                            EnumsContainerEnumName::Hotbarcontainer
                                        }
                                        DropSource::Inventory => {
                                            EnumsContainerEnumName::Inventorycontainer
                                        }
                                    },
                                    dynamic_id: None,
                                },
                                slot: drop.slot,
                                net_id_variant,
                            },
                            randomly: false,
                        },
                    ),
                ],
                ..Default::default()
            }],
        }))
    }

    /// `ItemStackRequest`: place one item of hotbar `slot` in the head
    /// armour slot.
    fn equip_request(&mut self, slot: u8) -> Result<McpePacket> {
        let net_id_variant = self
            .inventory
            .get(usize::from(slot))
            .and_then(|stack| stack.as_ref())
            .and_then(|stack| stack.net_id_variant)
            .with_context(|| format!("hotbar slot {slot} has no synchronized stack"))?;
        let request_id = self.next_request_id;
        self.next_request_id = request_id
            .checked_sub(2)
            .context("stack request ID exhausted")?;
        info!(slot, request_id, net_id_variant, "equipping the head slot");
        Ok(McpePacket::from(ItemStackRequestPacket {
            requests: vec![ItemStackRequestPacketDataRequestData {
                client_request_id: TypedClientNetIdstructItemStackRequestIdTagint32T0 {
                    id: request_id,
                },
                actions: vec![
                    ItemStackRequestPacketDataRequestDataActionsItem::PlaceActionData(
                        ItemStackRequestCerealPlaceActionData {
                            actiontype: EnumsItemStackRequestActionType::Place,
                            amount: 1,
                            source: ItemStackRequestCerealSlotInfoData {
                                fullcontainername: FullContainerName {
                                    container_name: EnumsContainerEnumName::Hotbarcontainer,
                                    dynamic_id: None,
                                },
                                slot,
                                net_id_variant,
                            },
                            destination: ItemStackRequestCerealSlotInfoData {
                                fullcontainername: FullContainerName {
                                    container_name: EnumsContainerEnumName::Armorcontainer,
                                    dynamic_id: None,
                                },
                                slot: 0,
                                net_id_variant: 0,
                            },
                        },
                    ),
                ],
                ..Default::default()
            }],
        }))
    }

    fn auth_input(&mut self) -> McpePacket {
        self.tick += 1;
        let actions = self.block_actions();
        let mut input_data = self.held.clone();
        for flag in self.pressed.drain(..) {
            if !input_data.contains(&flag) {
                input_data.push(flag);
            }
        }
        let mut move_vector = Vec2::default();
        if let Some(walk) = &mut self.walk {
            let thousandths = |value: i64| value as f32 / 1000.0;
            self.position.x += thousandths(walk.step[0]);
            self.position.y += thousandths(walk.step[1]);
            self.position.z += thousandths(walk.step[2]);
            self.rotation.y += thousandths(walk.turn);
            self.rotation.x += thousandths(walk.tilt);
            if walk.forward {
                move_vector = Vec2 { x: 0.0, y: 1.0 };
            }
            let walk_flags = [
                (walk.forward, EnumsPlayerAuthInputPacketPayloadInputData::Up),
                (walk.grounded, EnumsPlayerAuthInputPacketPayloadInputData::Verticalcollision),
            ];
            for (_, flag) in walk_flags.into_iter().filter(|(on, _)| *on) {
                if !input_data.contains(&flag) {
                    input_data.push(flag);
                }
            }
            walk.remaining = walk.remaining.saturating_sub(1);
            if walk.remaining == 0 {
                self.walk = None;
            }
        }
        let player_block_actions = if actions.is_empty() {
            None
        } else {
            input_data.push(EnumsPlayerAuthInputPacketPayloadInputData::Performblockactions);
            Some(actions)
        };
        McpePacket::from(PlayerAuthInputPacket {
            player_rotation: self.rotation.clone(),
            player_head_rotation: self.rotation.y,
            interact_rotation: self.rotation.clone(),
            position: self.position.clone(),
            input_data,
            input_mode: EnumsInputMode::Mouse,
            play_mode: EnumsClientPlayMode::Normal,
            new_interaction_model: EnumsNewInteractionModel::Crosshair,
            client_tick: PlayerInputTick {
                inputtick: self.tick,
            },
            player_block_actions,
            move_vector: move_vector.clone(),
            analog_move_vector: move_vector.clone(),
            raw_move_vector: move_vector,
            ..Default::default()
        })
    }

    fn action(&self, action: EnumsPlayerActionType) -> McpePacket {
        McpePacket::from(PlayerActionPacket {
            player_runtime_id: ActorRuntimeId {
                actor_runtime_id: self.runtime_id,
            },
            action,
            face: -1,
            ..Default::default()
        })
    }

    /// Follows server-set positions and returns the acknowledgements a
    /// vanilla client sends.
    fn handle(&mut self, packet: McpePacketData) -> Result<Vec<McpePacket>> {
        match packet {
            McpePacketData::MovePlayerPacket(packet)
                if packet.player_runtime_id.actor_runtime_id == self.runtime_id =>
            {
                self.position = packet.position;
                self.rotation = packet.rotation;
            }
            McpePacketData::TextPacket(text) => {
                if let Some(order) = parse_order(&format!("{:?}", text.body)) {
                    info!("order {order:?}");
                    return self.order(order);
                }
            }
            McpePacketData::InventoryContentPacket(content) if content.container_id == 0 => {
                self.inventory = std::array::from_fn(|_| None);
                for (slot, item) in content.slots.into_iter().take(36).enumerate() {
                    self.inventory[slot] = Some(item);
                }
            }
            McpePacketData::InventorySlotPacket(slot)
                if slot.container_id == 0 && slot.slot < 36 =>
            {
                self.inventory[slot.slot as usize] = Some(slot.item);
            }
            McpePacketData::ContainerOpenPacket(open) => {
                self.inventory_window = Some((open.container_id, open.container_type));
                if let Some(drop) = self.pending_drop.take() {
                    return Ok(vec![self.drop_request(drop)?]);
                }
                if let Some(slot) = self.pending_equip.take() {
                    return Ok(vec![self.equip_request(slot)?]);
                }
            }
            McpePacketData::ContainerClosePacket(close)
                if self
                    .inventory_window
                    .is_some_and(|(id, _)| id == close.container_id) =>
            {
                self.inventory_window = None;
                if close.server_initiated_close {
                    return Ok(vec![McpePacket::from(ContainerClosePacket {
                        server_initiated_close: false,
                        ..close
                    })]);
                }
            }
            McpePacketData::CorrectPlayerMovePredictionPacket(packet) => {
                self.position = packet.pos;
            }
            McpePacketData::ChangeDimensionPacket(packet) => {
                self.position = packet.position;
                return Ok(vec![self.action(EnumsPlayerActionType::Changedimensionack)]);
            }
            McpePacketData::RespawnPacket(packet) => match packet.state {
                EnumsPlayerRespawnState::Searchingforspawn => {
                    return Ok(vec![
                        McpePacket::from(RespawnPacket {
                            position: packet.position,
                            state: EnumsPlayerRespawnState::Clientreadytospawn,
                            player_runtime_id: ActorRuntimeId {
                                actor_runtime_id: self.runtime_id,
                            },
                        }),
                        self.action(EnumsPlayerActionType::Respawn),
                    ]);
                }
                EnumsPlayerRespawnState::Readytospawn => self.position = packet.position,
                _ => {}
            },
            McpePacketData::NetworkStackLatencyPacket(mut latency) if latency.is_from_server => {
                latency.is_from_server = false;
                return Ok(vec![McpePacket::from(latency)]);
            }
            _ => {}
        }
        Ok(Vec::new())
    }
}

// Match bds-extractor's stack allowance for the large generated packet codecs
// on Windows debug builds, including the initial login/definition exchange.
const STACK_SIZE: usize = 64 * 1024 * 1024;

fn main() -> Result<()> {
    let args = Args::parse();
    std::thread::Builder::new()
        .name("bds-sweep-client".into())
        .stack_size(STACK_SIZE)
        .spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(STACK_SIZE)
                .build()
                .context("building the sweep runtime")?
                .block_on(run(args))
        })
        .context("spawning the sweep client thread")?
        .join()
        .map_err(|_| anyhow::anyhow!("sweep client thread panicked"))?
}

async fn run(args: Args) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".parse().expect("static filter")),
        )
        .init();
    let script = match &args.script {
        Some(path) => parse_script(
            &std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?,
        )?,
        None => Vec::new(),
    };
    let mut config = ClientHandshakeConfig::random(args.addr, &args.name);
    if let Some(uuid) = args.uuid {
        config.uuid = uuid;
    }
    let recorder = args
        .capture_dir
        .as_ref()
        .map(|dir| {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
            Recorder::create(&dir.join("packets.jsonl"), config.identity_key.clone())
                .map(|recorder| Arc::new(Mutex::new(recorder)))
        })
        .transpose()?;
    let session = async {
        if let Some(recorder) = &recorder {
            let (play, game) = tokio::time::timeout(Duration::from_secs(args.timeout), async {
                let transport = recording::connect(args.addr, recorder.clone()).await?;
                let stream = BedrockStream::<Handshake, Client, _>::from_transport(
                    transport,
                    ClientConfig::default(),
                )
                .await?;
                Ok::<_, anyhow::Error>(stream.join(config).await?)
            })
            .await
            .context("login timed out")?
            .context("login")?;
            info!("spawned as {}", args.name);
            run_session(play, game, script).await
        } else {
            let (play, game) = tokio::time::timeout(Duration::from_secs(args.timeout), async {
                BedrockStream::connect(args.addr).await?.join(config).await
            })
            .await
            .context("login timed out")?
            .context("login")?;
            info!("spawned as {}", args.name);
            run_session(play, game, script).await
        }
    }
    .await;
    if let Some(recorder) = recorder {
        recorder
            .lock()
            .map_err(|_| anyhow::anyhow!("capture lock poisoned"))?
            .flush()?;
        info!(
            "capture complete: {}/packets.jsonl",
            args.capture_dir
                .as_ref()
                .expect("capture directory")
                .display()
        );
    }
    session
}

async fn run_session<T: Transport>(
    mut play: BedrockStream<Play, Client, T>,
    game: GameData,
    script: Vec<(u64, Step)>,
) -> Result<()> {
    let mut player = Player {
        runtime_id: game.start_game.runtime_id.actor_runtime_id,
        position: game.start_game.position.clone(),
        rotation: game.start_game.rotation.clone(),
        tick: 0,
        paused: 0,
        burst: 0,
        mining: None,
        pending: None,
        inventory: std::array::from_fn(|_| None),
        selected_slot: 0,
        inventory_window: None,
        pending_drop: None,
        next_request_id: -1,
        walk: None,
        held: Vec::new(),
        pressed: Vec::new(),
        quit: false,
        pending_equip: None,
    };
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    let mut steps = script.into_iter().peekable();
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if player.paused > 0 {
                    player.paused -= 1;
                    continue;
                }
                while let Some((_, step)) = steps.next_if(|(tick, _)| *tick <= player.tick + 1) {
                    match step {
                        Step::Exit => {
                            info!("script finished");
                            return Ok(());
                        }
                        Step::Command(command) => play.send_packet(command_request(&command)).await?,
                        Step::Order(order) => {
                            info!("script order {order:?}");
                            for packet in player.order(order)? {
                                play.send_packet(packet).await?;
                            }
                        }
                    }
                }
                play.send_packet(player.auth_input()).await?;
                for _ in 0..std::mem::take(&mut player.burst) {
                    play.send_packet(player.auth_input()).await?;
                }
            }
            packet = play.recv_packet_raw() => {
                let packet = match packet {
                    Ok(packet) => packet,
                    Err(JolyneError::Disconnected(disconnect)) => {
                        info!("server closed the session: {:?}", disconnect.packet.body());
                        return Ok(());
                    }
                    Err(error) => return Err(error.into()),
                };
                if matches!(
                    packet.name(),
                    Some(
                        McpePacketName::MovePlayerPacket
                            | McpePacketName::CorrectPlayerMovePredictionPacket
                            | McpePacketName::ChangeDimensionPacket
                            | McpePacketName::RespawnPacket
                            | McpePacketName::NetworkStackLatencyPacket
                            | McpePacketName::TextPacket
                            | McpePacketName::InventoryContentPacket
                            | McpePacketName::InventorySlotPacket
                            | McpePacketName::ContainerOpenPacket
                            | McpePacketName::ContainerClosePacket
                    )
                ) {
                    for reply in player.handle(packet.decode()?.data)? {
                        play.send_packet(reply).await?;
                    }
                    if player.quit {
                        info!("quitting");
                        play.close().await?;
                        return Ok(());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jolyne::valentine::{
        ContainerOpenPacket, InventoryContentPacket, InventorySlotPacket, McpePacketArgs,
    };

    fn player() -> Player {
        Player {
            runtime_id: 1,
            position: Vec3::default(),
            rotation: Vec2::default(),
            tick: 0,
            paused: 0,
            burst: 0,
            mining: None,
            pending: None,
            inventory: std::array::from_fn(|_| None),
            selected_slot: 0,
            inventory_window: None,
            pending_drop: None,
            next_request_id: -1,
            walk: None,
            held: Vec::new(),
            pressed: Vec::new(),
            quit: false,
            pending_equip: None,
        }
    }

    #[test]
    fn walk_orders_step_position_and_yaw_each_input() {
        let order = parse_order(r#"{ message: "bds-sweep:walk 200 0 -100 10000 2 3" }"#).unwrap();
        let mut player = player();
        player.order(order).unwrap();
        for _ in 0..3 {
            player.auth_input();
        }
        assert!((player.position.x - 0.4).abs() < 1e-6);
        assert!((player.position.z + 0.2).abs() < 1e-6);
        assert!((player.rotation.y - 20.0).abs() < 1e-4);
        assert!(player.walk.is_none(), "the walk ends after its inputs");
        let tilted = parse_order("bds-sweep:walk 0 0 0 0 2 2 -1500").unwrap();
        player.order(tilted).unwrap();
        player.auth_input();
        player.auth_input();
        assert!((player.rotation.x + 3.0).abs() < 1e-4);
    }

    #[test]
    fn use_orders_parse_and_send_a_block_click() {
        let order =
            parse_order(r#"{ message: "bds-sweep:use 3 -60 0 1 500 1000 250 12345" }"#).unwrap();
        assert_eq!(
            order,
            Order::Use(UseOrder {
                pos: [3, -60, 0],
                face: 1,
                click: [500, 1000, 250],
                target: 12345,
            })
        );
        let mut player = player();
        let packets = player.order(order).unwrap();
        let McpePacketData::InventoryTransactionPacket(packet) = &packets[0].data else {
            panic!("{packets:?}");
        };
        let InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(click) =
            &packet.transaction
        else {
            panic!("{packet:?}");
        };
        assert_eq!(
            click.action_type,
            EnumsItemUseInventoryTransactionActionType::Place
        );
        assert_eq!(
            (click.position.x, click.position.y, click.position.z),
            (3, -60, 0)
        );
        assert_eq!(click.face, 1);
        assert_eq!(click.click_position.z, 0.25);
        assert_eq!(click.target_block_id, 12345);
    }

    #[test]
    fn orders_parse_from_chat_text() {
        assert_eq!(
            parse_order(
                r#"MessageOnly(TextPacketPayloadMessageOnly { message: "bds-sweep:mine 1 -59 0 4 0" })"#
            ),
            Some(Order::Mine(Mining {
                pos: [1, -59, 0],
                face: 4,
                predict_after: 0,
                elapsed: 0
            }))
        );
        assert_eq!(parse_order("bds-sweep:stop"), Some(Order::Stop));
        assert_eq!(parse_order("bds-sweep:mine 1 2"), None);
        assert_eq!(parse_order("hello"), None);
        assert_eq!(
            parse_order(r#"{ message: "bds-sweep:attack 4294967297" }"#),
            Some(Order::Attack(4294967297))
        );
        assert_eq!(parse_order("bds-sweep:pause 10"), Some(Order::Pause(10)));
    }

    #[test]
    fn scripts_run_orders_and_commands_by_input_tick() {
        let steps = parse_script(
            "# arena
40 mine 1 -59 0 4 0

20 /setblock 1 -59 0 dirt
45 pause 3
50 release
51 burst 14
60 exit
",
        )
        .unwrap();
        assert_eq!(
            steps,
            [
                (20, Step::Command("/setblock 1 -59 0 dirt".into())),
                (
                    40,
                    Step::Order(Order::Mine(Mining {
                        pos: [1, -59, 0],
                        face: 4,
                        predict_after: 0,
                        elapsed: 0
                    }))
                ),
                (45, Step::Order(Order::Pause(3))),
                (50, Step::Order(Order::Release)),
                (51, Step::Order(Order::Burst(14))),
                (60, Step::Exit),
            ]
        );
        assert!(parse_script("x mine").is_err());
        assert!(parse_script("5 dance").is_err());
    }

    #[test]
    fn a_pause_holds_inputs_without_advancing_the_tick() {
        let mut p = player();
        p.order(Order::Pause(2)).unwrap();
        assert_eq!(p.paused, 2);
        let McpePacketData::PlayerAuthInputPacket(input) = p.auth_input().data else {
            panic!("auth input");
        };
        assert_eq!(
            input.client_tick.inputtick, 1,
            "the order lands on the next input"
        );
    }

    #[test]
    fn a_release_sends_stop_with_the_default_position_once() {
        let mut p = player();
        p.order(Order::Release).unwrap();
        assert_eq!(p.block_actions(), [], "nothing to release");
        p.order(Order::Mine(Mining {
            pos: [1, -59, 0],
            face: 4,
            predict_after: 0,
            elapsed: 0,
        }))
        .unwrap();
        assert_eq!(
            p.block_actions()[0].player_action_type,
            EnumsPlayerActionType::Startdestroyblock
        );
        p.order(Order::Release).unwrap();
        assert_eq!(
            p.block_actions(),
            [PlayerBlockActionData {
                player_action_type: EnumsPlayerActionType::Stopdestroyblock,
                position: BlockPos::default(),
                facing: 0,
            }]
        );
        assert_eq!(p.block_actions(), [], "the mining ended");
        p.order(Order::Burst(3)).unwrap();
        assert_eq!(p.burst, 3);
    }

    fn synchronize(player: &mut Player, slot: u32, count: u16, net_id: Option<i32>) {
        player
            .handle(McpePacketData::InventorySlotPacket(InventorySlotPacket {
                container_id: 0,
                slot,
                item: CerealizerNetworkItemStackDescriptorSerializedData {
                    id: 1,
                    stacksize: count,
                    net_id_variant: net_id,
                    ..Default::default()
                },
                ..Default::default()
            }))
            .unwrap();
    }

    fn open_inventory(player: &mut Player) -> Vec<McpePacket> {
        player
            .handle(McpePacketData::ContainerOpenPacket(ContainerOpenPacket {
                // Native own-inventory opening allocates a window (v5: 2),
                // distinct from the logical player inventory container 0.
                container_id: 2,
                container_type: 255,
                ..Default::default()
            }))
            .unwrap()
    }

    fn decoded_request(packet: &McpePacket) -> ItemStackRequestPacketDataRequestData {
        let mut wire = Vec::new();
        packet.data.encode_inner(&mut wire, 0, 0).unwrap();
        let mut input = wire.as_slice();
        let (_, data) = McpePacketData::decode_inner(&mut input, McpePacketArgs).unwrap();
        assert!(input.is_empty());
        let McpePacketData::ItemStackRequestPacket(mut request) = data else {
            panic!("expected a real ItemStackRequest packet");
        };
        assert_eq!(request.requests.len(), 1);
        request.requests.remove(0)
    }

    fn assert_drop(
        packet: &McpePacket,
        container: EnumsContainerEnumName,
        slot: u8,
        count: u8,
        net_id: i32,
    ) -> i32 {
        let request = decoded_request(packet);
        let [ItemStackRequestPacketDataRequestDataActionsItem::DropActionData(drop)] =
            request.actions.as_slice()
        else {
            panic!("expected one Drop action");
        };
        assert_eq!(drop.actiontype, EnumsItemStackRequestActionType::Drop);
        assert_eq!(drop.amount, count);
        assert_eq!(drop.source.fullcontainername.container_name, container);
        assert_eq!(drop.source.fullcontainername.dynamic_id, None);
        assert_eq!(drop.source.slot, slot);
        assert_eq!(drop.source.net_id_variant, net_id);
        assert!(
            !drop.randomly,
            "player Q is directional, not a random scatter"
        );
        assert!(
            request.client_request_id.id < 0 && request.client_request_id.id & 1 != 0,
            "BDS client ItemStackRequest IDs must be negative odd"
        );
        request.client_request_id.id
    }

    #[test]
    fn input_and_use_orders_parse_from_tellraw_text() {
        use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
        let tellraw = |order: &str| {
            format!(
                "{:?}",
                format!("{{\"rawtext\":[{{\"text\":\"bds-sweep:{order}\"}}]}}\n")
            )
        };
        assert_eq!(
            parse_order(&tellraw("press startsneaking sneaking")),
            Some(Order::Press(vec![Flag::Startsneaking, Flag::Sneaking]))
        );
        assert_eq!(
            parse_order(&tellraw("hold sprinting")),
            Some(Order::Hold(vec![Flag::Sprinting]))
        );
        assert_eq!(
            parse_order(&tellraw("release sprinting")),
            Some(Order::ReleaseFlags(vec![Flag::Sprinting]))
        );
        assert_eq!(parse_order(&tellraw("use")), Some(Order::UseItem));
        // Bare `use` and `release` keep their other meanings: the item use
        // (also in a script line, with nothing after it) and the mining
        // button's release.
        assert_eq!(parse_order("bds-sweep:use"), Some(Order::UseItem));
        assert_eq!(parse_order(&tellraw("release")), Some(Order::Release));
        assert_eq!(parse_order("bds-sweep:release"), Some(Order::Release));
        assert!(matches!(
            parse_order("bds-sweep:use 1 2 3 1 500 1000 500 7"),
            Some(Order::Use(_))
        ));
        assert_eq!(parse_order("bds-sweep:use 1 2 3"), None);
        assert_eq!(parse_order(&tellraw("quit")), Some(Order::Quit));
        assert_eq!(
            parse_order(&tellraw("equip-head 3")),
            Some(Order::EquipHead(3))
        );
        assert_eq!(parse_order(&tellraw("press nothing")), None);
        assert_eq!(parse_order(&tellraw("user")), None);
    }

    #[test]
    fn held_flags_repeat_until_released_and_pressed_flags_go_once() {
        use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
        let mut player = player();
        let flags = |player: &mut Player| match player.auth_input().data {
            McpePacketData::PlayerAuthInputPacket(input) => input.input_data,
            other => panic!("expected input, got {other:?}"),
        };
        player.order(Order::Hold(vec![Flag::Sneaking])).unwrap();
        player
            .order(Order::Press(vec![Flag::Startsneaking]))
            .unwrap();
        assert_eq!(flags(&mut player), [Flag::Sneaking, Flag::Startsneaking]);
        assert_eq!(flags(&mut player), [Flag::Sneaking]);
        player
            .order(Order::ReleaseFlags(vec![Flag::Sneaking]))
            .unwrap();
        player
            .order(Order::Press(vec![Flag::Stopsneaking]))
            .unwrap();
        assert_eq!(flags(&mut player), [Flag::Stopsneaking]);
        assert!(flags(&mut player).is_empty());
    }

    #[test]
    fn use_sends_an_item_use_in_the_air_with_the_held_stack() {
        let mut player = player();
        synchronize(&mut player, 0, 1, Some(5));
        let packets = player.order(Order::UseItem).unwrap();
        let [packet] = packets.as_slice() else {
            panic!("expected one transaction");
        };
        let McpePacketData::InventoryTransactionPacket(transaction) = &packet.data else {
            panic!("expected InventoryTransaction");
        };
        let InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(item_use) =
            &transaction.transaction
        else {
            panic!("expected an item use");
        };
        assert_eq!(
            item_use.action_type,
            EnumsItemUseInventoryTransactionActionType::Use
        );
        assert_eq!(item_use.slot, 0);
        assert_eq!(item_use.item.net_id_variant, Some(5));
    }

    #[test]
    fn drop_orders_parse_from_plugin_chat() {
        assert_eq!(
            parse_order(r#"{ message: "bds-sweep:drop hotbar 2 1" }"#),
            Some(Order::Drop(DropOrder {
                source: DropSource::Hotbar,
                slot: 2,
                count: 1
            })),
        );
        assert_eq!(
            parse_order("bds-sweep:drop inventory 9 64"),
            Some(Order::Drop(DropOrder {
                source: DropSource::Inventory,
                slot: 9,
                count: 64
            })),
        );
        assert_eq!(
            parse_order(r#"bds-sweep:close-inventory""#),
            Some(Order::CloseInventory)
        );
        assert_eq!(parse_order("bds-sweep:drop cursor 0 1"), None);
        assert_eq!(parse_order("bds-sweep:drop hotbar 0"), None);
        assert_eq!(parse_order("bds-sweep:drop hotbar -1 1"), None);
    }

    #[test]
    fn closed_hotbar_q_and_ctrl_q_use_authoritative_stack() {
        for count in [1, 64] {
            let mut player = player();
            synchronize(&mut player, 2, 64, Some(12345));
            let packets = player
                .order(Order::Drop(DropOrder {
                    source: DropSource::Hotbar,
                    slot: 2,
                    count,
                }))
                .unwrap();
            let [equipment, request] = packets.as_slice() else {
                panic!(
                    "closed hotbar drop should select its slot then request Drop, never open a screen"
                );
            };
            let McpePacketData::MobEquipmentPacket(equipment) = &equipment.data else {
                panic!("expected slot selection");
            };
            assert_eq!(equipment.slot, 2);
            assert_eq!(equipment.selected_slot, 2);
            assert_eq!(equipment.item.net_id_variant, Some(12345));
            assert_eq!(equipment.item.stacksize, 64);
            assert_eq!(
                assert_drop(
                    request,
                    EnumsContainerEnumName::Hotbarcontainer,
                    2,
                    count,
                    12345
                ),
                -1
            );
            assert!(player.inventory_window.is_none());
            assert!(player.pending_drop.is_none());
        }
    }

    #[test]
    fn inventory_q_and_ctrl_q_wait_for_open_and_use_latest_net_id() {
        for count in [1, 64] {
            let mut player = player();
            synchronize(&mut player, 9, 64, Some(41));
            let packets = player
                .order(Order::Drop(DropOrder {
                    source: DropSource::Inventory,
                    slot: 9,
                    count,
                }))
                .unwrap();
            let [open] = packets.as_slice() else {
                panic!("only OpenInventory may precede the server acknowledgement");
            };
            let McpePacketData::InteractPacket(open) = &open.data else {
                panic!("expected Interact");
            };
            assert_eq!(open.action, EnumsInteractPacketPayloadAction::Openinventory);
            assert_eq!(open.target_runtime_id.actor_runtime_id, player.runtime_id);
            synchronize(&mut player, 9, 64, Some(9876));
            let packets = open_inventory(&mut player);
            assert_eq!(packets.len(), 1);
            assert_eq!(
                assert_drop(
                    &packets[0],
                    EnumsContainerEnumName::Inventorycontainer,
                    9,
                    count,
                    9876
                ),
                -1
            );
            assert_eq!(player.inventory_window, Some((2, 255)));
            let closed = player.order(Order::CloseInventory).unwrap();
            let [close] = closed.as_slice() else {
                panic!("expected one close");
            };
            let McpePacketData::ContainerClosePacket(close) = &close.data else {
                panic!("expected ContainerClose");
            };
            assert_eq!((close.container_id, close.container_type), (2, 255));
            assert!(!close.server_initiated_close);
            assert!(player.inventory_window.is_none());
            assert!(player.order(Order::CloseInventory).unwrap().is_empty());
        }
    }

    #[test]
    fn pending_inventory_drop_rejects_overlap_and_revalidates_server_slot() {
        let mut player = player();
        synchronize(&mut player, 9, 64, Some(71));
        let drop = Order::Drop(DropOrder {
            source: DropSource::Inventory,
            slot: 9,
            count: 64,
        });
        player.order(drop.clone()).unwrap();
        assert!(player.order(drop).is_err());
        assert!(player.order(Order::CloseInventory).is_err());
        synchronize(&mut player, 9, 1, Some(72));
        assert!(
            player
                .handle(McpePacketData::ContainerOpenPacket(ContainerOpenPacket {
                    container_id: 2,
                    container_type: 255,
                    ..Default::default()
                }))
                .is_err()
        );
        assert_eq!(
            player.next_request_id, -1,
            "failed pending Drop must not consume an ID or send a request"
        );
    }

    #[test]
    fn already_open_inventory_uses_latest_slot_without_reopening() {
        let mut player = player();
        synchronize(&mut player, 9, 64, Some(71));
        open_inventory(&mut player);
        synchronize(&mut player, 9, 63, Some(72));
        let packets = player
            .order(Order::Drop(DropOrder {
                source: DropSource::Inventory,
                slot: 9,
                count: 1,
            }))
            .unwrap();
        let [request] = packets.as_slice() else {
            panic!("already open inventory must not send another Interact");
        };
        assert_drop(
            request,
            EnumsContainerEnumName::Inventorycontainer,
            9,
            1,
            72,
        );
        assert_eq!(player.inventory_window, Some((2, 255)));
    }

    #[test]
    fn hotbar_drop_closes_open_screen_before_request_and_ids_advance() {
        let mut player = player();
        synchronize(&mut player, 0, 64, Some(71));
        open_inventory(&mut player);
        let drop = Order::Drop(DropOrder {
            source: DropSource::Hotbar,
            slot: 0,
            count: 1,
        });
        let packets = player.order(drop.clone()).unwrap();
        let [close, request] = packets.as_slice() else {
            panic!("expected close before Drop");
        };
        assert!(matches!(
            &close.data,
            McpePacketData::ContainerClosePacket(_)
        ));
        assert_eq!(
            assert_drop(request, EnumsContainerEnumName::Hotbarcontainer, 0, 1, 71),
            -1
        );
        let next = player.order(drop).unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(
            assert_drop(&next[0], EnumsContainerEnumName::Hotbarcontainer, 0, 1, 71),
            -3
        );
    }

    #[test]
    fn full_content_and_slot_updates_replace_authoritative_inventory() {
        let mut player = player();
        synchronize(&mut player, 0, 64, Some(100));
        synchronize(&mut player, 9, 64, Some(101));
        player
            .handle(McpePacketData::InventoryContentPacket(
                InventoryContentPacket {
                    container_id: 0,
                    slots: vec![CerealizerNetworkItemStackDescriptorSerializedData {
                        id: 1,
                        stacksize: 7,
                        net_id_variant: Some(999),
                        ..Default::default()
                    }],
                    ..Default::default()
                },
            ))
            .unwrap();
        let packets = player
            .order(Order::Drop(DropOrder {
                source: DropSource::Hotbar,
                slot: 0,
                count: 7,
            }))
            .unwrap();
        assert_drop(
            &packets[0],
            EnumsContainerEnumName::Hotbarcontainer,
            0,
            7,
            999,
        );
        assert!(
            player
                .order(Order::Drop(DropOrder {
                    source: DropSource::Inventory,
                    slot: 9,
                    count: 1,
                }))
                .is_err()
        );
        synchronize(&mut player, 0, 5, Some(1000));
        let packets = player
            .order(Order::Drop(DropOrder {
                source: DropSource::Hotbar,
                slot: 0,
                count: 5,
            }))
            .unwrap();
        assert_drop(
            &packets[0],
            EnumsContainerEnumName::Hotbarcontainer,
            0,
            5,
            1000,
        );
    }

    #[test]
    fn drop_rejects_unknown_empty_or_idless_slots_and_invalid_counts() {
        let mut player = player();
        let drop = |slot, count| {
            Order::Drop(DropOrder {
                source: DropSource::Hotbar,
                slot,
                count,
            })
        };
        assert!(player.order(drop(0, 1)).is_err());
        for net_id in [None, Some(0), Some(-1)] {
            synchronize(&mut player, 0, 64, net_id);
            assert!(player.order(drop(0, 1)).is_err());
        }
        synchronize(&mut player, 0, 0, Some(1));
        assert!(player.order(drop(0, 1)).is_err());
        synchronize(&mut player, 0, 3, Some(1));
        assert!(player.order(drop(0, 0)).is_err());
        assert!(player.order(drop(0, 4)).is_err());
        assert!(player.order(drop(9, 1)).is_err());
        assert!(
            player
                .order(Order::Drop(DropOrder {
                    source: DropSource::Inventory,
                    slot: 36,
                    count: 1,
                }))
                .is_err()
        );
        assert_eq!(player.next_request_id, -1);
        assert!(player.pending_drop.is_none());
    }

    #[test]
    fn mining_sends_start_continue_then_predict() {
        let mut player = player();
        player
            .order(parse_order("bds-sweep:mine 1 -59 0 4 3").unwrap())
            .unwrap();
        let kinds: Vec<_> = (0..5)
            .map(|_| {
                player
                    .block_actions()
                    .iter()
                    .map(|a| a.player_action_type)
                    .collect::<Vec<_>>()
            })
            .collect();
        use EnumsPlayerActionType::*;
        assert_eq!(
            kinds,
            [
                vec![Startdestroyblock],
                vec![Continuedestroyblock],
                vec![Continuedestroyblock],
                vec![Predictdestroyblock],
                vec![],
            ]
        );
    }

    #[test]
    fn stop_aborts_once() {
        let mut player = player();
        player
            .order(parse_order("bds-sweep:mine 1 -59 0 4 0").unwrap())
            .unwrap();
        player.block_actions();
        player.order(Order::Stop).unwrap();
        let actions = player.block_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].player_action_type,
            EnumsPlayerActionType::Abortdestroyblock
        );
        assert!(player.block_actions().is_empty());
    }
}
