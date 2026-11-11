//! Scripted player for the BDS sweep harness (`tools/bds-sweep`).
//!
//! Joins an offline BDS over loopback and behaves like an idle vanilla
//! client so the server simulates it: `PlayerAuthInput` every 50 ms at the
//! position and rotation the server last set (with the camera orientation
//! of that rotation), latency probes answered,
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
//!
//! Item-use orders, shaped like the vanilla client's packets (the held item is
//! the selected hotbar slot as last sent by the server):
//!
//! * `attack-named NAME`: `attack` the actor whose `AddActor` carried the
//!   name tag `NAME` (`/summon ... <nameTag>`);
//! * `jab`: `InventoryTransaction` item use with the action `UseAsAttack`
//!   (an attack made by the item itself, no target); `jab-input` packs the
//!   same transaction into the next `PlayerAuthInput`
//!   (`PerformItemInteraction`);
//! * `swing`: an `Animate` arm swing (source `attack`) and the next input
//!   flagged `MissedSwing`; `animate` and `missed-swing` send only one of
//!   the two;
//! * `use`: item use `Use` (click air), the next input flagged
//!   `StartUsingItem`;
//! * `use-on X Y Z FACE`: item use `Place` (click a block) on that face, with
//!   the block's runtime id as last sent by the server;
//! * `release-item` / `consume`: item release `Release` / `Use`;
//! * `select SLOT`: `MobEquipment` selecting hotbar index 0..8;
//! * `glide` / `unglide`: the next input flagged `StartGliding` /
//!   `StopGliding`;
//! * `sprint` / `unsprint`: `StartSprinting` / `StopSprinting`, then
//!   `Sprinting` on every input while it lasts;
//! * `look YAW PITCH`: the rotation sent from the next input on;
//! * `move DX DY DZ TICKS`: move by `(DX, DY, DZ)` blocks on each of the
//!   next `TICKS` inputs (forward input held).
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
        ActorRuntimeId, AnimatePacket, BlockPos,
        CerealizerNetworkItemStackDescriptorSerializedData, CommandOriginDatajson,
        CommandRequestPacket, ContainerClosePacket, DataItemEntryPayload,
        EnumsAnimatePacketPayloadAction, EnumsClientPlayMode, EnumsContainerEnumName,
        EnumsHandSlot, EnumsInputMode, EnumsInteractPacketPayloadAction,
        EnumsItemReleaseInventoryTransactionActionType, EnumsItemStackRequestActionType,
        EnumsItemUseInventoryTransactionActionType,
        EnumsItemUseInventoryTransactionClientCooldownState,
        EnumsItemUseInventoryTransactionPredictedResult,
        EnumsItemUseInventoryTransactionTriggerType,
        EnumsItemUseOnActorInventoryTransactionActionType, EnumsNewInteractionModel,
        EnumsPlayerActionType, EnumsPlayerAuthInputPacketPayloadInputData, EnumsPlayerRespawnState,
        FullContainerName, InteractPacket, InventoryTransactionPacket,
        InventoryTransactionPacketTransaction, ItemReleaseInventoryTransaction,
        ItemStackRequestCerealDropActionData, ItemStackRequestCerealSlotInfoData,
        ItemStackRequestPacket, ItemStackRequestPacketDataRequestData,
        ItemStackRequestPacketDataRequestDataActionsItem, ItemUseInventoryTransaction,
        ItemUseOnActorInventoryTransaction, McpePacket, McpePacketArgs, McpePacketData,
        McpePacketName, MobEquipmentPacket, PackedItemUseLegacyInventoryTransaction,
        PlayerActionPacket, PlayerAuthInputPacket, PlayerBlockActionData, PlayerInputTick,
        RespawnPacket, TypedClientNetIdstructItemStackRequestIdTagint32T0, Vec2, Vec3,
    },
};
use std::collections::HashMap;
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
    /// Run the timed orders and commands of this file (see the crate docs).
    #[arg(long)]
    script: Option<PathBuf>,
    /// Print the packets of a `--capture-dir` recording (`packets.jsonl`)
    /// decoded, one per line with the client tick of the latest input, and
    /// exit without connecting.
    #[arg(long)]
    decode: Option<PathBuf>,
    /// With `--decode`: only these packet names (comma separated, without
    /// the `Packet` suffix).
    #[arg(long, value_delimiter = ',')]
    only: Vec<String>,
}

/// `--decode`: every record of a capture as `input t_ms dir name {packet}`.
fn decode(path: &std::path::Path, only: &[String]) -> Result<()> {
    use std::io::Write;
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut out = std::io::BufWriter::new(std::io::stdout().lock());
    let mut input = 0u64;
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let row: serde_json::Value = serde_json::from_str(line)
            .with_context(|| format!("{}: line {}", path.display(), index + 1))?;
        let name = row["name"].as_str().unwrap_or("?");
        let short = name.trim_end_matches("Packet");
        // The recorder keeps the packet without its batch length prefix.
        let bytes = hex::decode(row["hex"].as_str().unwrap_or_default())?;
        let mut framed = Vec::with_capacity(bytes.len() + 5);
        let mut len = bytes.len() as u32;
        while len >= 0x80 {
            framed.push(len as u8 | 0x80);
            len >>= 7;
        }
        framed.push(len as u8);
        framed.extend_from_slice(&bytes);
        let decoded = McpePacketData::decode_inner(&mut framed.as_slice(), McpePacketArgs);
        if let Ok((_, McpePacketData::PlayerAuthInputPacket(auth))) = &decoded {
            input = auth.client_tick.inputtick;
        }
        if !only.is_empty() && !only.iter().any(|wanted| wanted == short) {
            continue;
        }
        let body = match decoded {
            Ok((_, packet)) => format!("{packet:?}"),
            Err(error) => format!("undecodable: {error}"),
        };
        writeln!(
            out,
            "{input} {} {} {short} {body}",
            row["t_ms"].as_f64().unwrap_or_default().round(),
            row["dir"].as_str().unwrap_or("?"),
        )?;
    }
    Ok(())
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

#[derive(Debug, Clone, PartialEq)]
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
    /// Attack the actor added with this name tag.
    AttackNamed(String),
    /// Item use `UseAsAttack`: the held item attacks by itself.
    Jab,
    /// [`Order::Jab`] packed into the next `PlayerAuthInput`
    /// (`PerformItemInteraction`) instead of its own packet.
    JabInput,
    /// An arm swing that hit nothing: [`Order::Animate`] and
    /// [`Order::MissedSwing`] together, as a vanilla client sends it.
    Swing,
    /// Only the `Animate` arm swing.
    Animate,
    /// Only the `MissedSwing` input flag.
    MissedSwing,
    /// Item use `Use` (click air).
    UseAir,
    /// Item use `Place` on a block face.
    UseOn([i32; 3], i32),
    /// Item release `Release`.
    ReleaseItem,
    /// Item release `Use` (finish eating or drinking).
    Consume,
    /// Select a hotbar index.
    Select(u8),
    /// Start (`true`) or stop gliding.
    Glide(bool),
    /// Start (`true`) or stop sprinting.
    Sprint(bool),
    /// Yaw and pitch of the next inputs.
    Look(f32, f32),
    /// Move by this much on each of the next inputs.
    Move([f32; 3], u32),
    Drop(DropOrder),
    CloseInventory,
}

/// A `move` order in progress.
#[derive(Debug, Clone, PartialEq)]
struct Moving {
    delta: [f32; 3],
    left: u32,
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

/// The first `n` decimal numbers of an order (see [`numbers`]).
fn decimals<'a>(words: impl Iterator<Item = &'a str>, n: usize) -> Option<Vec<f32>> {
    let numbers: Vec<f32> = words
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
    // A last word is followed by the closing quote of the Debug form.
    let keyword = words
        .next()?
        .trim_end_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '-'));
    match keyword {
        "stop" => Some(Order::Stop),
        "release" => Some(Order::Release),
        "jab" => Some(Order::Jab),
        "jab-input" => Some(Order::JabInput),
        "swing" => Some(Order::Swing),
        "animate" => Some(Order::Animate),
        "missed-swing" => Some(Order::MissedSwing),
        "use" => Some(Order::UseAir),
        "release-item" => Some(Order::ReleaseItem),
        "consume" => Some(Order::Consume),
        "glide" => Some(Order::Glide(true)),
        "unglide" => Some(Order::Glide(false)),
        "sprint" => Some(Order::Sprint(true)),
        "unsprint" => Some(Order::Sprint(false)),
        "attack-named" => {
            let name = words
                .next()?
                .trim_end_matches(|c: char| !(c.is_ascii_alphanumeric() || c == '_'));
            (!name.is_empty()).then(|| Order::AttackNamed(name.to_owned()))
        }
        "use-on" => {
            let values = numbers(words, 4)?;
            Some(Order::UseOn(
                [values[0] as i32, values[1] as i32, values[2] as i32],
                i32::try_from(values[3])
                    .ok()
                    .filter(|face| (0..=5).contains(face))?,
            ))
        }
        "select" => Some(Order::Select(
            u8::try_from(numbers(words, 1)?[0])
                .ok()
                .filter(|slot| *slot < 9)?,
        )),
        "look" => {
            let values = decimals(words, 2)?;
            Some(Order::Look(values[0], values[1]))
        }
        "move" => {
            let values = decimals(words, 4)?;
            Some(Order::Move(
                [values[0], values[1], values[2]],
                values[3] as u32,
            ))
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
        "close-inventory" => Some(Order::CloseInventory),
        _ => None,
    }
}

/// One step of a `--script` file.
#[derive(Debug, Clone, PartialEq)]
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
    /// Runtime ids of actors added with a name tag, by name.
    actors: HashMap<String, u64>,
    /// Block runtime ids the server sent (`UpdateBlock`, layer 0).
    blocks: HashMap<[i32; 3], u32>,
    /// Input flags of the next input only.
    flags: Vec<EnumsPlayerAuthInputPacketPayloadInputData>,
    /// An item use packed into the next input.
    packed_use: Option<ItemUseInventoryTransaction>,
    sprinting: bool,
    moving: Option<Moving>,
}

/// The unit view vector of a rotation (pitch `x`, yaw `y`, degrees), as a
/// vanilla client sends its camera orientation: yaw 0 looks south (+z), -90
/// east (+x); positive pitch looks down.
fn view_vector(rotation: &Vec2) -> Vec3 {
    let (pitch, yaw) = (rotation.x.to_radians(), rotation.y.to_radians());
    Vec3 {
        x: -yaw.sin() * pitch.cos(),
        y: -pitch.sin(),
        z: yaw.cos() * pitch.cos(),
    }
}

/// The click position a vanilla client sends for a block face: the face's
/// centre in block coordinates.
fn face_offset(face: i32) -> Vec3 {
    let (x, y, z) = match face {
        0 => (0.5, 0.0, 0.5),
        1 => (0.5, 1.0, 0.5),
        2 => (0.5, 0.5, 0.0),
        3 => (0.5, 0.5, 1.0),
        4 => (0.0, 0.5, 0.5),
        _ => (1.0, 0.5, 0.5),
    };
    Vec3 { x, y, z }
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
    fn new(runtime_id: u64, position: Vec3, rotation: Vec2) -> Self {
        Self {
            runtime_id,
            position,
            rotation,
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
            actors: HashMap::new(),
            blocks: HashMap::new(),
            flags: Vec::new(),
            packed_use: None,
            sprinting: false,
            moving: None,
        }
    }

    /// The stack in the selected hotbar slot, as the server last sent it.
    fn held(&self) -> CerealizerNetworkItemStackDescriptorSerializedData {
        self.inventory[usize::from(self.selected_slot)]
            .clone()
            .unwrap_or_default()
    }

    /// `InventoryTransaction` item use with the held item: `Use` and
    /// `UseAsAttack` without a block, `Place` on `block`'s face.
    fn use_item(
        &self,
        action_type: EnumsItemUseInventoryTransactionActionType,
        block: Option<([i32; 3], i32)>,
    ) -> McpePacket {
        McpePacket::from(InventoryTransactionPacket {
            transaction: InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(
                Box::new(self.item_use(action_type, block)),
            ),
            ..Default::default()
        })
    }

    /// The item-use transaction of [`Player::use_item`].
    fn item_use(
        &self,
        action_type: EnumsItemUseInventoryTransactionActionType,
        block: Option<([i32; 3], i32)>,
    ) -> ItemUseInventoryTransaction {
        let (position, face, click_position, target_block_id) = match block {
            Some((pos, face)) => (
                BlockPos {
                    x: pos[0],
                    y: pos[1],
                    z: pos[2],
                },
                face as u8,
                face_offset(face),
                self.blocks.get(&pos).copied().unwrap_or(0),
            ),
            None => (BlockPos::default(), 0, Vec3::default(), 0),
        };
        ItemUseInventoryTransaction {
            actions: Default::default(),
            action_type,
            trigger_type: EnumsItemUseInventoryTransactionTriggerType::Playerinput,
            position,
            face,
            slot: i32::from(self.selected_slot),
            hand: EnumsHandSlot::Mainhand,
            item: self.held(),
            from_position: self.position.clone(),
            click_position,
            target_block_id,
            client_interact_prediction: EnumsItemUseInventoryTransactionPredictedResult::Success,
            client_cooldown_state: EnumsItemUseInventoryTransactionClientCooldownState::Off,
        }
    }

    /// `InventoryTransaction` item release with the held item.
    fn release_item(
        &self,
        action_type: EnumsItemReleaseInventoryTransactionActionType,
    ) -> McpePacket {
        McpePacket::from(InventoryTransactionPacket {
            transaction: InventoryTransactionPacketTransaction::ItemReleaseInventoryTransaction(
                ItemReleaseInventoryTransaction {
                    actions: Default::default(),
                    action_type,
                    slot: i32::from(self.selected_slot),
                    item: self.held(),
                    from_position: self.position.clone(),
                },
            ),
            ..Default::default()
        })
    }

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
            Order::AttackNamed(name) => {
                let target = *self
                    .actors
                    .get(&name)
                    .with_context(|| format!("no actor named {name} has been added"))?;
                return Ok(vec![self.attack(target)]);
            }
            Order::Jab => {
                return Ok(vec![self.use_item(
                    EnumsItemUseInventoryTransactionActionType::Useasattack,
                    None,
                )]);
            }
            Order::JabInput => {
                self.packed_use = Some(self.item_use(
                    EnumsItemUseInventoryTransactionActionType::Useasattack,
                    None,
                ));
            }
            Order::Swing => {
                self.flags
                    .push(EnumsPlayerAuthInputPacketPayloadInputData::Missedswing);
                return Ok(vec![self.animate_swing()]);
            }
            Order::Animate => return Ok(vec![self.animate_swing()]),
            Order::MissedSwing => self
                .flags
                .push(EnumsPlayerAuthInputPacketPayloadInputData::Missedswing),
            Order::UseAir => {
                self.flags
                    .push(EnumsPlayerAuthInputPacketPayloadInputData::Startusingitem);
                return Ok(vec![
                    self.use_item(EnumsItemUseInventoryTransactionActionType::Use, None),
                ]);
            }
            Order::UseOn(pos, face) => {
                return Ok(vec![self.use_item(
                    EnumsItemUseInventoryTransactionActionType::Place,
                    Some((pos, face)),
                )]);
            }
            Order::ReleaseItem => {
                return Ok(vec![self.release_item(
                    EnumsItemReleaseInventoryTransactionActionType::Release,
                )]);
            }
            Order::Consume => {
                return Ok(vec![self.release_item(
                    EnumsItemReleaseInventoryTransactionActionType::Use,
                )]);
            }
            Order::Select(slot) => {
                self.selected_slot = slot;
                return Ok(vec![McpePacket::from(MobEquipmentPacket {
                    target_runtime_id: ActorRuntimeId {
                        actor_runtime_id: self.runtime_id,
                    },
                    item: self.held(),
                    slot,
                    selected_slot: slot,
                    container_id: 0,
                })]);
            }
            Order::Glide(start) => self.flags.push(if start {
                EnumsPlayerAuthInputPacketPayloadInputData::Startgliding
            } else {
                EnumsPlayerAuthInputPacketPayloadInputData::Stopgliding
            }),
            Order::Sprint(start) => {
                self.sprinting = start;
                self.flags.push(if start {
                    EnumsPlayerAuthInputPacketPayloadInputData::Startsprinting
                } else {
                    EnumsPlayerAuthInputPacketPayloadInputData::Stopsprinting
                });
            }
            Order::Look(yaw, pitch) => self.rotation = Vec2 { x: pitch, y: yaw },
            Order::Move(delta, ticks) => {
                self.moving = (ticks > 0).then_some(Moving { delta, left: ticks });
            }
            Order::Drop(drop) => return self.drop_order(drop),
            Order::CloseInventory => {
                ensure!(
                    self.pending_drop.is_none(),
                    "inventory open still pending for a drop"
                );
                return Ok(self.close_inventory().into_iter().collect());
            }
        }
        Ok(Vec::new())
    }

    /// The `Animate` arm swing of an attack.
    fn animate_swing(&self) -> McpePacket {
        McpePacket::from(AnimatePacket {
            action: EnumsAnimatePacketPayloadAction::Swing,
            target_actor_runtime_id: ActorRuntimeId {
                actor_runtime_id: self.runtime_id,
            },
            data: 0.0,
            swing_source: Some("attack".into()),
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
                    slot: i32::from(self.selected_slot),
                    item: self.held(),
                    from_position: self.position.clone(),
                    ..Default::default()
                },
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

    fn auth_input(&mut self) -> McpePacket {
        self.tick += 1;
        let actions = self.block_actions();
        let mut input_data = std::mem::take(&mut self.flags);
        let player_block_actions = if actions.is_empty() {
            None
        } else {
            input_data.push(EnumsPlayerAuthInputPacketPayloadInputData::Performblockactions);
            Some(actions)
        };
        if self.sprinting {
            input_data.push(EnumsPlayerAuthInputPacketPayloadInputData::Sprinting);
        }
        let item_use_transaction = self.packed_use.take().map(|item_use_transaction| {
            input_data.push(EnumsPlayerAuthInputPacketPayloadInputData::Performiteminteraction);
            PackedItemUseLegacyInventoryTransaction {
                legacy_request_id: Default::default(),
                legacy_set_item_slots: None,
                item_use_transaction,
            }
        });
        // A `move` order: the forward key held, the position advanced.
        let (mut move_vector, mut pos_delta) = (Vec2::default(), Vec3::default());
        if let Some(moving) = &mut self.moving {
            let [dx, dy, dz] = moving.delta;
            self.position.x += dx;
            self.position.y += dy;
            self.position.z += dz;
            pos_delta = Vec3 {
                x: dx,
                y: dy,
                z: dz,
            };
            move_vector = Vec2 { x: 0.0, y: 1.0 };
            input_data.push(EnumsPlayerAuthInputPacketPayloadInputData::Up);
            moving.left -= 1;
            if moving.left == 0 {
                self.moving = None;
            }
        }
        McpePacket::from(PlayerAuthInputPacket {
            player_rotation: self.rotation.clone(),
            player_head_rotation: self.rotation.y,
            interact_rotation: self.rotation.clone(),
            camera_orientation: view_vector(&self.rotation),
            position: self.position.clone(),
            move_vector: move_vector.clone(),
            analog_move_vector: move_vector.clone(),
            raw_move_vector: move_vector,
            pos_delta,
            input_data,
            input_mode: EnumsInputMode::Mouse,
            play_mode: EnumsClientPlayMode::Normal,
            new_interaction_model: EnumsNewInteractionModel::Crosshair,
            client_tick: PlayerInputTick {
                inputtick: self.tick,
            },
            player_block_actions,
            item_use_transaction,
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
            McpePacketData::AddActorPacket(actor) => {
                let name = actor.actor_data.data.iter().find_map(|entry| {
                    match (&entry.id, &entry.payload) {
                        (4, DataItemEntryPayload::DataItemStringPayload(name)) => {
                            Some(name.value.to_string())
                        }
                        _ => None,
                    }
                });
                if let Some(name) = name.filter(|name| !name.is_empty()) {
                    self.actors
                        .insert(name, actor.target_runtime_id.actor_runtime_id);
                }
            }
            McpePacketData::UpdateBlockPacket(update) if update.layer == 0 => {
                let p = update.block_position;
                self.blocks.insert([p.x, p.y, p.z], update.block_runtime_id);
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
    if let Some(path) = &args.decode {
        return decode(path, &args.only);
    }
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
    let config = ClientHandshakeConfig::random(args.addr, &args.name);
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
    let mut player = Player::new(
        game.start_game.runtime_id.actor_runtime_id,
        game.start_game.position.clone(),
        game.start_game.rotation.clone(),
    );
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
                            | McpePacketName::AddActorPacket
                            | McpePacketName::UpdateBlockPacket
                    )
                ) {
                    for reply in player.handle(packet.decode()?.data)? {
                        play.send_packet(reply).await?;
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
        Player::new(1, Vec3::default(), Vec2::default())
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

    fn input(player: &mut Player) -> PlayerAuthInputPacket {
        let McpePacketData::PlayerAuthInputPacket(input) = player.auth_input().data else {
            panic!("auth input");
        };
        *input
    }

    fn item_use(packet: &McpePacket) -> ItemUseInventoryTransaction {
        let McpePacketData::InventoryTransactionPacket(packet) = &packet.data else {
            panic!("inventory transaction");
        };
        let InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(item_use) =
            &packet.transaction
        else {
            panic!("item use");
        };
        (**item_use).clone()
    }

    #[test]
    fn item_use_orders_parse_from_scripts_and_chat() {
        for (text, order) in [
            ("bds-sweep:jab", Order::Jab),
            ("bds-sweep:swing", Order::Swing),
            ("bds-sweep:animate", Order::Animate),
            ("bds-sweep:missed-swing", Order::MissedSwing),
            (r#"{ message: "bds-sweep:use" }"#, Order::UseAir),
            ("bds-sweep:release-item", Order::ReleaseItem),
            ("bds-sweep:consume", Order::Consume),
            ("bds-sweep:glide", Order::Glide(true)),
            ("bds-sweep:unglide", Order::Glide(false)),
            ("bds-sweep:sprint", Order::Sprint(true)),
            ("bds-sweep:select 3", Order::Select(3)),
            ("bds-sweep:use-on 1 -59 0 1", Order::UseOn([1, -59, 0], 1)),
            ("bds-sweep:look -90 12.5", Order::Look(-90.0, 12.5)),
            (
                "bds-sweep:move 0.25 0 -0.5 4",
                Order::Move([0.25, 0.0, -0.5], 4),
            ),
            (
                r#"{ message: "bds-sweep:attack-named front" }"#,
                Order::AttackNamed("front".into()),
            ),
        ] {
            assert_eq!(parse_order(text), Some(order), "{text}");
        }
        assert_eq!(parse_order("bds-sweep:select 9"), None);
        assert_eq!(parse_order("bds-sweep:use-on 1 2 3 6"), None);
    }

    #[test]
    fn item_use_orders_send_vanilla_shaped_transactions() {
        let mut p = player();
        synchronize(&mut p, 2, 1, Some(7));
        p.order(Order::Select(2)).unwrap();
        let jab = item_use(&p.order(Order::Jab).unwrap()[0]);
        assert_eq!(
            jab.action_type,
            EnumsItemUseInventoryTransactionActionType::Useasattack
        );
        assert_eq!((jab.slot, jab.item.net_id_variant), (2, Some(7)));
        let used = item_use(&p.order(Order::UseAir).unwrap()[0]);
        assert_eq!(
            used.action_type,
            EnumsItemUseInventoryTransactionActionType::Use
        );
        assert!(
            input(&mut p)
                .input_data
                .contains(&EnumsPlayerAuthInputPacketPayloadInputData::Startusingitem),
            "the next input starts using the item"
        );
        assert!(input(&mut p).input_data.is_empty(), "flags last one input");
        p.handle(McpePacketData::UpdateBlockPacket(
            jolyne::valentine::UpdateBlockPacket {
                block_position: BlockPos { x: 1, y: -59, z: 0 },
                block_runtime_id: 99,
                ..Default::default()
            },
        ))
        .unwrap();
        let on = item_use(&p.order(Order::UseOn([1, -59, 0], 1)).unwrap()[0]);
        assert_eq!(
            on.action_type,
            EnumsItemUseInventoryTransactionActionType::Place
        );
        assert_eq!((on.face, on.target_block_id), (1, 99));
        assert_eq!(on.click_position.y, 1.0);
    }

    #[test]
    fn moves_advance_the_position_with_forward_held() {
        let mut p = player();
        p.order(Order::Sprint(true)).unwrap();
        p.order(Order::Move([0.25, 0.0, 0.0], 2)).unwrap();
        let first = input(&mut p);
        assert_eq!(first.position.x, 0.25);
        assert!(
            first
                .input_data
                .contains(&EnumsPlayerAuthInputPacketPayloadInputData::Startsprinting)
        );
        assert_eq!(first.move_vector.y, 1.0);
        assert_eq!(input(&mut p).position.x, 0.5);
        let still = input(&mut p);
        assert_eq!(still.position.x, 0.5, "two inputs only");
        p.order(Order::Look(-90.0, 0.0)).unwrap();
        let east = input(&mut p).camera_orientation;
        assert!((east.x - 1.0).abs() < 1e-6 && east.y.abs() < 1e-6 && east.z.abs() < 1e-6);
        assert!(
            still
                .input_data
                .contains(&EnumsPlayerAuthInputPacketPayloadInputData::Sprinting)
        );
    }
}
