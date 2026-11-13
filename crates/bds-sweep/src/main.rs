//! Scripted player for the BDS sweep harness (`tools/bds-sweep`).
//!
//! Joins an offline BDS over loopback and plays like a vanilla client so the
//! server simulates it: `PlayerAuthInput` every 50 ms reporting a movement
//! the client simulates itself ([`physics`], against the blocks BDS sends,
//! [`world`]), latency probes answered, teleports, respawns and dimension
//! changes acknowledged, BDS's corrections replayed. The Endstone sweep
//! plugin and plain-BDS scripts (`tools/bds-sweep/player_script.py`) drive
//! it with chat orders:
//!
//! * `bds-sweep:move F S`, `look YAW PITCH`, `sprint on|off`,
//!   `sneak on|off`, `jump [TICKS]`, `glide`: hold movement keys, turn,
//!   press sprint or sneak, jump, open the elytra;
//! * `bds-sweep:mine X Y Z FACE PREDICT`: start destroying the block (block
//!   action `StartDestroyBlock`), then `ContinueDestroyBlock` every tick; when
//!   `PREDICT` > 0, send `PredictDestroyBlock` that many ticks after the start
//!   and stop;
//! * `bds-sweep:stop`: abort the current mining (`AbortDestroyBlock`);
//! * `bds-sweep:attack RUNTIME_ID`: attack that actor with the held item
//!   (`InventoryTransaction` use-item-on-actor, hotbar slot 0 as last sent
//!   by the server).
//! * `bds-sweep:drop hotbar SLOT COUNT`: closed-screen Q (`COUNT=1`) or
//!   ctrl-Q (`COUNT=stack size`) from hotbar index 0..8;
//! * `bds-sweep:drop inventory SLOT COUNT`: open the native inventory, wait
//!   for `ContainerOpen`, then drop from main-inventory index 0..35;
//! * `bds-sweep:close-inventory`: close the inventory opened by this client.
//! * `bds-sweep:press FLAG...`: add these `PlayerAuthInput` input flags
//!   (lower-case names, `startsneaking`) to the next input only;
//! * `bds-sweep:hold FLAG...` / `bds-sweep:release FLAG...`: add them to
//!   every input from the next one on, until released;
//! * `bds-sweep:use`: use the held item in the air (`InventoryTransaction`
//!   item use, hotbar slot as last synchronized); BDS rejects it for armour
//!   and resends the whole inventory, which refreshes stack net IDs;
//! * `bds-sweep:quit`: close the connection and exit (BDS keeps the player
//!   until the connection times out, about 10 s later);
//! * `bds-sweep:equip-head SLOT` / `bds-sweep:equip-feet SLOT`: open the
//!   inventory (as an inventory drop does) and move one item from hotbar
//!   index SLOT to the head or feet armour slot (`ItemStackRequest` place).
//! * `bds-sweep:teleports follow|late N|ignore|fake`: how the next server
//!   teleports are taken (probes of the teleport acknowledgement): followed
//!   as a vanilla client does, `N` inputs late, never (no `HandledTeleport`
//!   either), or acknowledged without moving;
//! * `bds-sweep:corrections follow|ignore`: rewind on
//!   `CorrectPlayerMovePrediction` (the default) or keep the own prediction;
//! * `bds-sweep:offset DX DY DZ`: report the position this many thousandths
//!   of a block away from the simulated one (`offset 0 0 0` ends it, and a
//!   drift);
//! * `bds-sweep:drift DX DY DZ`: grow that offset by this many thousandths
//!   of a block every input;
//! * `bds-sweep:walk DX DY DZ DYAW TICKS FLAGS [DPITCH]`: for the next
//!   `TICKS` inputs, move the reported position by (`DX`, `DY`, `DZ`)
//!   thousandths of a block and turn the yaw (and head yaw) by `DYAW` and the
//!   pitch by `DPITCH` thousandths of a degree each input; `FLAGS` bit 0 also
//!   reports forward movement (`Up`, move vector (0, 1)), bit 1 reports
//!   `VerticalCollision` (standing on the ground).
//!
//! Drop orders require server-synchronized nonempty slots with stack net IDs.
//! Inventory drops leave the screen open; hotbar drops close it first.
//! `--capture-dir DIR` records the entire session, including login and both
//! directions, in `DIR/packets.jsonl` using the client-diff wire recorder.
//! Packets are flushed as recorded and again on session completion/error.
//!
//! Exits successfully when the server closes the session.

use bds_sweep::{physics, scenario, world};

use anyhow::{Context, Result, ensure};
use clap::Parser;
use client_diff::recording::{self, Recorder};
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::{
    BedrockStream, Client, ClientConfig, GameData, JolyneError, Play,
    stream::{Handshake, transport::Transport},
    valentine::{
        ActorRuntimeId, BlockPos, CerealizerNetworkItemStackDescriptorSerializedData,
        CommandOriginDatajson, CommandRequestPacket, ContainerClosePacket, DataItemEntryPayload,
        EnumsClientPlayMode, EnumsContainerEnumName, EnumsInputMode,
        EnumsInteractPacketPayloadAction, EnumsItemStackRequestActionType,
        EnumsItemUseInventoryTransactionActionType,
        EnumsItemUseInventoryTransactionPredictedResult,
        EnumsItemUseInventoryTransactionTriggerType,
        EnumsItemUseOnActorInventoryTransactionActionType, EnumsMobEffectPacketPayloadEvent,
        EnumsNewInteractionModel, EnumsPlayerActionType,
        EnumsPlayerAuthInputPacketPayloadInputData, EnumsPlayerRespawnState, FullContainerName,
        InteractPacket, InventoryTransactionPacket, InventoryTransactionPacketTransaction,
        ItemStackRequestCerealDropActionData, ItemStackRequestCerealPlaceActionData,
        ItemStackRequestCerealSlotInfoData, ItemStackRequestPacket,
        ItemStackRequestPacketDataRequestData, ItemStackRequestPacketDataRequestDataActionsItem,
        ItemUseInventoryTransaction, ItemUseOnActorInventoryTransaction, McpePacket,
        McpePacketData, McpePacketName, MobEquipmentPacket, PlayerActionPacket,
        PlayerAuthInputPacket, PlayerBlockActionData, PlayerInputTick, RespawnPacket,
        TypedClientNetIdstructItemStackRequestIdTagint32T0, Vec2, Vec3, actor_flags,
    },
};
use physics::{Body, Changes, Controls};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{info, warn};

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
    /// Play this movement corpus (`tools/movement-diff/corpus.json`) when
    /// the `scenarios` order arrives.
    #[arg(long)]
    scenarios: Option<PathBuf>,
    /// Where to write each scenario's runtime id and first input tick.
    #[arg(long, requires = "scenarios")]
    scenario_log: Option<PathBuf>,
    /// How far off the scenario's reported position is, per axis
    /// (`x,y,z`; harness validation reruns with other offsets).
    #[arg(long, requires = "scenarios", value_delimiter = ',', num_args = 1,
          allow_hyphen_values = true, default_values_t = SCENARIO_OFFSET)]
    scenario_offset: Vec<f32>,
    /// Where a scenario's derived input flags come from.
    #[arg(long, requires = "scenarios", value_enum, default_value_t = FlagSource::Simulation)]
    flag_source: FlagSource,
}

/// How a scenario input's derived flags (`StartSprinting`, `StartJumping`,
/// the collisions...) are chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum FlagSource {
    /// From the client's own simulation, as a vanilla client derives them
    /// from its prediction.
    Simulation,
    /// From the keys alone, with no simulation: starts and stops on the
    /// key edges, no collision or swim flags (harness validation: BDS's
    /// copy must not depend on them).
    Keys,
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
    /// Attack the actor with this runtime id with the held item.
    Attack(u64),
    Drop(DropOrder),
    CloseInventory,
    /// Input flags for the next `PlayerAuthInput` only.
    Press(Vec<EnumsPlayerAuthInputPacketPayloadInputData>),
    /// Input flags for every `PlayerAuthInput` until released.
    Hold(Vec<EnumsPlayerAuthInputPacketPayloadInputData>),
    Release(Vec<EnumsPlayerAuthInputPacketPayloadInputData>),
    /// Use the held item in the air.
    UseItem,
    /// Leave the server.
    Quit,
    /// Move one item of this hotbar slot to an armour slot (0 head, 3 feet).
    Equip {
        slot: u8,
        armour: u8,
    },
    /// Hold the movement keys: forward (W +) and strafe (A +), each -1..=1.
    Move {
        forward: f32,
        strafe: f32,
    },
    /// Turn to this yaw and pitch (degrees).
    Look {
        yaw: f32,
        pitch: f32,
    },
    /// Hold or release the sprint key.
    Sprint(bool),
    /// Hold or release the sneak key.
    Sneak(bool),
    /// Hold the jump key for this many ticks.
    Jump(u32),
    /// Open the elytra (start gliding) while in the air.
    Glide,
    /// Report scripted positions for a while ([`Walk`]).
    Walk(Walk),
    /// How server teleports are taken from now on.
    Teleports(TeleportMode),
    /// Whether corrections are rewound.
    Corrections(bool),
    /// Report the position this far (thousandths of a block) from the body.
    Offset([i64; 3]),
    /// Grow the offset by this much (thousandths of a block) every input.
    Drift([i64; 3]),
    /// Play the movement corpus given with `--scenarios`.
    Scenarios,
}

/// How the client takes a server teleport (`MovePlayer`, a respawn's
/// position): the vanilla way, or a probe of what BDS does with the inputs
/// sent before the acknowledgement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TeleportMode {
    /// Move there and say `HandledTeleport` with the next input.
    Follow,
    /// Keep moving from where the player was for this many inputs, then
    /// follow.
    Late(u32),
    /// Never move there, never say `HandledTeleport`.
    Ignore,
    /// Say `HandledTeleport` with the next input without moving there.
    Fake,
}

/// A teleport a `Late` client has not followed yet.
#[derive(Debug, Clone, Copy, PartialEq)]
struct PendingTeleport {
    /// Inputs left before it is followed.
    inputs: u32,
    position: [f32; 3],
    on_ground: bool,
    pitch: f32,
    yaw: f32,
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
        "verticalcollision" => Flag::Verticalcollision,
        "horizontalcollision" => Flag::Horizontalcollision,
        "handledteleport" => Flag::Handledteleport,
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

/// The first `n` decimal numbers of an order (signs and fractions allowed;
/// trailing quotes or escapes after the last are ignored).
fn decimals<'a>(words: impl Iterator<Item = &'a str>, n: usize) -> Option<Vec<f32>> {
    let values: Vec<f32> = words
        .take(n)
        .map(|word| {
            word.trim_end_matches(|c: char| !c.is_ascii_digit())
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite())
        })
        .collect::<Option<_>>()?;
    (values.len() == n).then_some(values)
}

/// `on` or `off` (leading letters of the word).
fn switch(word: Option<&str>) -> Option<bool> {
    let word: String = word?
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .collect();
    match word.as_str() {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    }
}

/// The order word itself: its leading letters and dashes.
fn order_word(word: &str) -> &str {
    let end = word
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(word.len());
    &word[..end]
}

/// Parses a plugin order from a chat message (anywhere in the text).
fn parse_order(text: &str) -> Option<Order> {
    let rest = &text[text.find("bds-sweep:")? + "bds-sweep:".len()..];
    let mut words = rest.split_whitespace();
    let first = words.next()?;
    match order_word(first) {
        "move" => {
            let values = decimals(words, 2)?;
            let in_range = |v: f32| (-1.0..=1.0).contains(&v);
            return (in_range(values[0]) && in_range(values[1])).then_some(Order::Move {
                forward: values[0],
                strafe: values[1],
            });
        }
        "look" => {
            let values = decimals(words, 2)?;
            return (-90.0..=90.0).contains(&values[1]).then_some(Order::Look {
                yaw: values[0],
                pitch: values[1],
            });
        }
        "sprint" => return Some(Order::Sprint(switch(words.next())?)),
        "sneak" => return Some(Order::Sneak(switch(words.next())?)),
        "jump" => {
            let ticks = match words.next() {
                Some(word) if word.starts_with(|c: char| c.is_ascii_digit()) => {
                    u32::try_from(numbers(std::iter::once(word), 1)?[0]).ok()?
                }
                _ => 1,
            };
            return (ticks > 0).then_some(Order::Jump(ticks));
        }
        "glide" => return Some(Order::Glide),
        _ => {}
    }
    match first {
        "stop" | "stop\"" => Some(Order::Stop),
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
        word if word.starts_with("scenarios") && !word[9..].starts_with(char::is_alphanumeric) => {
            Some(Order::Scenarios)
        }
        "press" => Some(Order::Press(input_flags(words)?)),
        "hold" => Some(Order::Hold(input_flags(words)?)),
        "release" => Some(Order::Release(input_flags(words)?)),
        word if word.starts_with("use") && !word[3..].starts_with(char::is_alphanumeric) => {
            Some(Order::UseItem)
        }
        word if word.starts_with("quit") && !word[4..].starts_with(char::is_alphanumeric) => {
            Some(Order::Quit)
        }
        "equip-head" | "equip-feet" => Some(Order::Equip {
            slot: u8::try_from(numbers(words, 1)?[0]).ok()?,
            armour: if first == "equip-head" { 0 } else { 3 },
        }),
        "teleports" => {
            let mode = words.next()?;
            let mode: String = mode.chars().take_while(char::is_ascii_alphabetic).collect();
            Some(Order::Teleports(match mode.as_str() {
                "follow" => TeleportMode::Follow,
                "ignore" => TeleportMode::Ignore,
                "fake" => TeleportMode::Fake,
                "late" => TeleportMode::Late(u32::try_from(numbers(words, 1)?[0]).ok()?),
                _ => return None,
            }))
        }
        "corrections" => {
            let mode: String = words
                .next()?
                .chars()
                .take_while(char::is_ascii_alphabetic)
                .collect();
            match mode.as_str() {
                "follow" => Some(Order::Corrections(true)),
                "ignore" => Some(Order::Corrections(false)),
                _ => None,
            }
        }
        "offset" => {
            let values = numbers(words, 3)?;
            Some(Order::Offset([values[0], values[1], values[2]]))
        }
        "drift" => {
            let values = numbers(words, 3)?;
            Some(Order::Drift([values[0], values[1], values[2]]))
        }
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
        _ => None,
    }
}

/// One input tick as simulated, kept for rewinds.
#[derive(Debug, Clone, Copy)]
struct Frame {
    tick: u64,
    controls: Controls,
    /// A `walk` input: the feet moved by this step instead of the physics.
    step: Option<[f32; 3]>,
    /// The body after this tick's move.
    after: Body,
}

/// Input ticks kept for `CorrectPlayerMovePrediction` rewinds (BDS's
/// default rewind history is 40 ticks).
const HISTORY: usize = 200;

/// The own player's state needed to tick like a vanilla client.
struct Player {
    runtime_id: u64,
    /// The simulated body and the keys held for it.
    body: Body,
    controls: Controls,
    /// Ticks the jump key stays held (`jump` order).
    jump_ticks: u32,
    /// The jump key was held on the last input.
    jump_was_held: bool,
    /// A `glide` order waits for the next input.
    glide: bool,
    /// A teleport arrived: the next input says `HandledTeleport`.
    teleported: bool,
    /// The next input does not move the body (the first after a teleport).
    rest: bool,
    world: world::World,
    history: VecDeque<Frame>,
    tick: u64,
    mining: Option<Mining>,
    /// A block action to send with the next input (abort).
    pending: Option<PlayerBlockActionData>,
    /// Authoritative native main inventory, including hotbar indices 0..8.
    inventory: [Option<CerealizerNetworkItemStackDescriptorSerializedData>; 36],
    selected_slot: u8,
    /// Server-confirmed inventory window (id, type), never a guessed window.
    inventory_window: Option<(u8, u8)>,
    pending_drop: Option<DropOrder>,
    next_request_id: i32,
    /// Input flags sent with every input (`hold`) and with the next one
    /// only (`press`).
    held: Vec<EnumsPlayerAuthInputPacketPayloadInputData>,
    pressed: Vec<EnumsPlayerAuthInputPacketPayloadInputData>,
    /// A `quit` order arrived: close the session.
    quit: bool,
    /// Hotbar slot to equip on the head once the inventory opens.
    pending_equip: Option<(u8, u8)>,
    /// `CorrectPlayerMovePrediction` packets received.
    corrections: u32,
    /// A `walk` order in progress.
    walk: Option<Walk>,
    /// How server teleports are taken, and one a `Late` client has not
    /// followed yet.
    teleport_mode: TeleportMode,
    pending_teleport: Option<PendingTeleport>,
    /// Corrections are rewound.
    follow_corrections: bool,
    /// The reported position's distance from the body's, and its growth
    /// per input.
    offset: [f32; 3],
    drift: [f32; 3],
    /// The movement corpus being played (`--scenarios`).
    script: Option<Script>,
    /// The controls of the last input.
    previous_controls: Controls,
    /// The login identity, the origin of the commands this client sends.
    uuid: uuid::Uuid,
}

/// How far from its own simulation a scripted scenario reports the player,
/// on every axis: over BDS's acceptance threshold, so BDS's copy is never
/// moved towards the reported position and moves by its own simulation
/// alone. Offsetting only the height is not enough: BDS takes a reported
/// horizontal position within 0.1 of its copy on each horizontal axis even
/// when the height is 3 blocks off.
const SCENARIO_OFFSET: [f32; 3] = [3.0, 3.0, 3.0];

/// The input flags a client with no simulation sends for `controls`, after
/// `previous`: the keys and their edges only.
fn key_flags(
    controls: &Controls,
    previous: &Controls,
    glide: bool,
    teleported: bool,
) -> Vec<EnumsPlayerAuthInputPacketPayloadInputData> {
    use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
    let (forward, strafe) = (controls.forward, controls.strafe);
    let sprinting = controls.sprint && forward > 0.0 && !controls.sneak;
    let was_sprinting = previous.sprint && previous.forward > 0.0 && !previous.sneak;
    [
        (forward > 0.0, Flag::Up),
        (forward < 0.0, Flag::Down),
        (strafe > 0.0, Flag::Left),
        (strafe < 0.0, Flag::Right),
        (forward > 0.0 && strafe > 0.0, Flag::Upleft),
        (forward > 0.0 && strafe < 0.0, Flag::Upright),
        (forward < 0.0 && strafe > 0.0, Flag::Downleft),
        (forward < 0.0 && strafe < 0.0, Flag::Downright),
        (controls.sprint, Flag::Sprintdown),
        (sprinting, Flag::Sprinting),
        (sprinting && !was_sprinting, Flag::Startsprinting),
        (!sprinting && was_sprinting, Flag::Stopsprinting),
        (controls.sneak, Flag::Sneakdown),
        (controls.sneak, Flag::Sneakcurrentraw),
        (controls.sneak, Flag::Sneaking),
        (controls.sneak && !previous.sneak, Flag::Startsneaking),
        (controls.sneak && !previous.sneak, Flag::Sneakpressedraw),
        (!controls.sneak && previous.sneak, Flag::Stopsneaking),
        (!controls.sneak && previous.sneak, Flag::Sneakreleasedraw),
        (glide, Flag::Startgliding),
        (controls.jump, Flag::Jumpdown),
        (controls.jump, Flag::Jumping),
        (controls.jump, Flag::Jumpcurrentraw),
        (controls.jump && !previous.jump, Flag::Jumppressedraw),
        (!controls.jump && previous.jump, Flag::Jumpreleasedraw),
        (controls.jump && !previous.jump, Flag::Startjumping),
        (teleported, Flag::Handledteleport),
    ]
    .into_iter()
    .filter_map(|(on, flag)| on.then_some(flag))
    .collect()
}
/// Idle inputs between a scenario's last input and the next setup.
const SCENARIO_LEAD: u32 = 3;
/// Inputs to wait after a scenario's setup commands for their blocks.
const SCENARIO_SETTLE: u32 = 20;
/// Where the player waits while a scenario is built.
const LOBBY: &str = "/tp @s -14.5 100 10.5 0 0";

/// The movement corpus as the scripted client plays it, one scenario after
/// the other: build it from the lobby, teleport to the start, and from the
/// second input after the teleport (BDS rests the first) play its inputs.
struct Script {
    scenarios: Vec<scenario::Scenario>,
    /// The reported position's offset while a scenario plays.
    offset: [f32; 3],
    flags: FlagSource,
    next: usize,
    stage: Stage,
    runs: Vec<scenario::ScenarioRun>,
    log: Option<PathBuf>,
}

enum Stage {
    /// Waiting for the `scenarios` order.
    Waiting,
    /// One command per input, then `settle` inputs for the blocks.
    Setup {
        /// Idle inputs before the first command: BDS may run a command
        /// before it simulates an input that arrived in the same tick, so a
        /// lobby teleport right after a scenario's last input could move the
        /// player before that input's move.
        lead: u32,
        commands: VecDeque<String>,
        settle: u32,
    },
    /// The start teleport was sent `waited` inputs ago.
    Teleport { waited: u32 },
    /// Teleported: the scenario's first input is input `first`.
    Arrived { first: u64 },
    Play {
        first: u64,
        inputs: Vec<scenario::TickInput>,
    },
    /// All played; quit after `linger` idle inputs (BDS relays the last
    /// scenario input only if the player is still there).
    Done { linger: u32 },
}

/// What the script does with one input.
#[derive(Default)]
struct ScriptTick {
    /// A scenario input: play it, reporting the far position.
    input: Option<scenario::TickInput>,
    /// Commands to send after the input.
    commands: Vec<String>,
    /// Turn to this yaw and pitch from the next input on.
    look: Option<(f32, f32)>,
}

impl Script {
    fn load(
        path: &std::path::Path,
        log: Option<PathBuf>,
        offset: [f32; 3],
        flags: FlagSource,
    ) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Ok(Self {
            scenarios: scenario::Corpus::parse(&text)?.scenarios,
            offset,
            flags,
            next: 0,
            stage: Stage::Waiting,
            runs: Vec::new(),
            log,
        })
    }

    fn start(&mut self) -> Result<()> {
        ensure!(
            matches!(self.stage, Stage::Waiting),
            "the scenarios already started"
        );
        self.setup_next()
    }

    /// Build the next scenario, or finish.
    fn setup_next(&mut self) -> Result<()> {
        let Some(scenario) = self.scenarios.get(self.next) else {
            self.stage = Stage::Done { linger: 20 };
            if let Some(log) = &self.log {
                std::fs::write(log, serde_json::to_string_pretty(&self.runs)? + "\n")
                    .with_context(|| format!("writing {}", log.display()))?;
            }
            info!(scenarios = self.runs.len(), "scenarios done");
            return Ok(());
        };
        let mut commands = VecDeque::from([LOBBY.to_owned()]);
        commands.extend(scenario.setup_commands()?);
        info!(scenario = scenario.name, "setting up");
        self.stage = Stage::Setup {
            lead: SCENARIO_LEAD,
            commands,
            settle: SCENARIO_SETTLE,
        };
        Ok(())
    }

    fn done(&self) -> bool {
        matches!(self.stage, Stage::Done { linger: 0 })
    }

    /// A teleport of this player after input `tick`.
    fn teleported(&mut self, tick: u64) {
        if let Stage::Teleport { .. } = self.stage {
            // BDS rests the input after the teleport and simulates the next.
            self.stage = Stage::Arrived { first: tick + 2 };
        }
    }

    /// The script's part of input `tick`.
    fn tick(&mut self, tick: u64, runtime_id: u64) -> Result<ScriptTick> {
        let mut out = ScriptTick::default();
        match &mut self.stage {
            Stage::Waiting => {}
            Stage::Done { linger } => *linger = linger.saturating_sub(1),
            Stage::Setup {
                lead,
                commands,
                settle,
            } => {
                if *lead > 0 {
                    *lead -= 1;
                } else if let Some(command) = commands.pop_front() {
                    out.commands.push(command);
                } else if *settle > 0 {
                    *settle -= 1;
                } else {
                    let scenario = &self.scenarios[self.next];
                    out.commands.push(scenario.start_command());
                    // Inputs sent before the teleport arrives already face the
                    // start's way, as the scenario's first inputs must.
                    out.look = Some((scenario.start.yaw, scenario.start.pitch));
                    self.stage = Stage::Teleport { waited: 0 };
                }
            }
            Stage::Teleport { waited } => {
                *waited += 1;
                ensure!(*waited < 100, "no teleport to the scenario start");
            }
            Stage::Arrived { first } => {
                // Inputs already on their way when the teleport arrived went
                // out idle; every scenario starts with idle inputs for them.
                if tick >= *first {
                    let first = *first;
                    let scenario = &self.scenarios[self.next];
                    info!(scenario = scenario.name, first, tick, "playing");
                    self.runs.push(scenario::ScenarioRun {
                        name: scenario.name.clone(),
                        runtime_id,
                        first_input: first,
                        ticks: scenario.ticks(),
                    });
                    self.stage = Stage::Play {
                        first,
                        inputs: scenario.tick_inputs(),
                    };
                    return self.tick(tick, runtime_id);
                }
            }
            Stage::Play { first, inputs } => {
                let index = usize::try_from(tick - *first)?;
                out.input = Some(inputs[index]);
                let number = u32::try_from(index + 1)?;
                let last = index + 1 == inputs.len();
                let scenario = &self.scenarios[self.next];
                out.commands
                    .extend(scenario.commands_after(number).map(str::to_owned));
                if last {
                    self.next += 1;
                    self.setup_next()?;
                }
            }
        }
        Ok(out)
    }
}

/// The input flags a vanilla client sets for this tick's keys and move.
fn movement_flags(
    controls: &Controls,
    body: &Body,
    changes: &Changes,
    jump_was_held: bool,
    started_gliding: bool,
    teleported: bool,
) -> Vec<EnumsPlayerAuthInputPacketPayloadInputData> {
    use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
    let (forward, strafe) = (controls.forward, controls.strafe);
    [
        (forward > 0.0, Flag::Up),
        (forward < 0.0, Flag::Down),
        (strafe > 0.0, Flag::Left),
        (strafe < 0.0, Flag::Right),
        (forward > 0.0 && strafe > 0.0, Flag::Upleft),
        (forward > 0.0 && strafe < 0.0, Flag::Upright),
        (forward < 0.0 && strafe > 0.0, Flag::Downleft),
        (forward < 0.0 && strafe < 0.0, Flag::Downright),
        (controls.sprint, Flag::Sprintdown),
        (body.sprinting, Flag::Sprinting),
        (changes.started_sprinting, Flag::Startsprinting),
        (changes.stopped_sprinting, Flag::Stopsprinting),
        (controls.sneak, Flag::Sneakdown),
        (controls.sneak, Flag::Sneakcurrentraw),
        (body.sneaking, Flag::Sneaking),
        (changes.started_sneaking, Flag::Startsneaking),
        (changes.started_sneaking, Flag::Sneakpressedraw),
        (changes.stopped_sneaking, Flag::Stopsneaking),
        (changes.stopped_sneaking, Flag::Sneakreleasedraw),
        (changes.started_swimming, Flag::Startswimming),
        (changes.stopped_swimming, Flag::Stopswimming),
        (started_gliding, Flag::Startgliding),
        (changes.stopped_gliding, Flag::Stopgliding),
        (controls.jump, Flag::Jumpdown),
        (controls.jump, Flag::Jumping),
        (controls.jump, Flag::Jumpcurrentraw),
        (controls.jump && !jump_was_held, Flag::Jumppressedraw),
        (!controls.jump && jump_was_held, Flag::Jumpreleasedraw),
        (changes.jumped, Flag::Startjumping),
        (body.vertical_collision, Flag::Verticalcollision),
        (body.horizontal_collision, Flag::Horizontalcollision),
        (teleported, Flag::Handledteleport),
    ]
    .into_iter()
    .filter_map(|(on, flag)| on.then_some(flag))
    .collect()
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
    fn new(runtime_id: u64, position: Vec3, rotation: Vec2, uuid: uuid::Uuid) -> Self {
        let mut body = Body::at([0.0; 3]);
        body.feet = body.feet_from_network([position.x, position.y, position.z]);
        Self {
            runtime_id,
            body,
            controls: Controls {
                pitch: rotation.x,
                yaw: rotation.y,
                ..Default::default()
            },
            jump_ticks: 0,
            jump_was_held: false,
            glide: false,
            teleported: false,
            rest: false,
            world: world::World::default(),
            history: VecDeque::new(),
            tick: 0,
            mining: None,
            pending: None,
            inventory: std::array::from_fn(|_| None),
            selected_slot: 0,
            inventory_window: None,
            pending_drop: None,
            next_request_id: -1,
            held: Vec::new(),
            pressed: Vec::new(),
            quit: false,
            pending_equip: None,
            corrections: 0,
            walk: None,
            teleport_mode: TeleportMode::Follow,
            pending_teleport: None,
            follow_corrections: true,
            offset: [0.0; 3],
            drift: [0.0; 3],
            script: None,
            previous_controls: Controls::default(),
            uuid,
        }
    }

    /// A command this player runs (`CommandRequest`).
    fn command(&self, text: String) -> McpePacket {
        McpePacket::from(CommandRequestPacket {
            command: text.into(),
            origin: CommandOriginDatajson {
                type_: "player".into(),
                uuid: self.uuid,
                ..Default::default()
            },
            is_internal: false,
            version: "latest".into(),
        })
    }

    /// The wire position (the eyes of a standing player), moved by the
    /// `offset` order.
    fn position(&self) -> Vec3 {
        let [x, y, z] = self.body.network_position();
        Vec3 {
            x: x + self.offset[0],
            y: y + self.offset[1],
            z: z + self.offset[2],
        }
    }

    /// Put the body where the server says, at rest.
    fn place(&mut self, position: &Vec3, on_ground: bool) {
        self.body.feet = self
            .body
            .feet_from_network([position.x, position.y, position.z]);
        self.body.velocity = [0.0; 3];
        self.body.on_ground = on_ground;
        self.history.clear();
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
            Order::Attack(target) => return Ok(vec![self.attack(target)]),
            Order::Drop(drop) => return self.drop_order(drop),
            Order::Walk(walk) => self.walk = Some(walk),
            Order::Teleports(mode) => self.teleport_mode = mode,
            Order::Corrections(follow) => self.follow_corrections = follow,
            Order::Offset(offset) => {
                self.offset = offset.map(|v| v as f32 / 1000.0);
                self.drift = [0.0; 3];
            }
            Order::Drift(drift) => self.drift = drift.map(|v| v as f32 / 1000.0),
            Order::Scenarios => {
                let script = self
                    .script
                    .as_mut()
                    .context("the scenarios order needs --scenarios")?;
                script.start()?;
            }
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
            Order::Release(flags) => self.held.retain(|flag| !flags.contains(flag)),
            Order::Move { forward, strafe } => {
                self.controls.forward = forward;
                self.controls.strafe = strafe;
            }
            Order::Look { yaw, pitch } => {
                self.controls.yaw = yaw;
                self.controls.pitch = pitch;
            }
            Order::Sprint(on) => self.controls.sprint = on,
            Order::Sneak(on) => self.controls.sneak = on,
            Order::Jump(ticks) => self.jump_ticks = ticks,
            Order::Glide => {
                // A vanilla client opens the elytra on a jump press in the air.
                self.glide = true;
                self.jump_ticks = self.jump_ticks.max(1);
            }
            Order::UseItem => return Ok(vec![self.use_item()]),
            Order::Quit => self.quit = true,
            Order::Equip { slot, armour } => {
                if self.inventory_window.is_some() {
                    return Ok(vec![self.equip_request(slot, armour)?]);
                }
                self.pending_equip = Some((slot, armour));
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
                    from_position: self.position(),
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
                    from_position: self.position(),
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

    /// `ItemStackRequest`: place one item of hotbar `slot` in armour slot
    /// `armour` (0 head, 3 feet).
    fn equip_request(&mut self, slot: u8, armour: u8) -> Result<McpePacket> {
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
        info!(
            slot,
            armour, request_id, net_id_variant, "equipping an armour slot"
        );
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
                                slot: armour,
                                net_id_variant: 0,
                            },
                        },
                    ),
                ],
                ..Default::default()
            }],
        }))
    }

    /// This tick's controls: the held keys, and the jump key while a `jump`
    /// order lasts.
    fn tick_controls(&mut self) -> Controls {
        let mut controls = self.controls;
        controls.jump = self.jump_ticks > 0;
        self.jump_ticks = self.jump_ticks.saturating_sub(1);
        controls
    }

    /// A `Late` teleport whose inputs ran out: put the body there at rest and
    /// acknowledge it with this input.
    fn follow_late_teleport(&mut self) {
        let Some(pending) = &mut self.pending_teleport else {
            return;
        };
        if pending.inputs > 0 {
            pending.inputs -= 1;
            return;
        }
        let pending = self.pending_teleport.take().expect("checked above");
        self.body.feet = self.body.feet_from_network(pending.position);
        self.body.velocity = [0.0; 3];
        self.body.on_ground = pending.on_ground;
        self.history.clear();
        self.rest = true;
        self.controls.pitch = pending.pitch;
        self.controls.yaw = pending.yaw;
        self.teleported = true;
        info!(tick = self.tick, position = ?pending.position, "followed a late teleport");
    }

    /// The next input of a `walk` order: the order as it was, and the step
    /// it moves the feet by. Turns the controls by its yaw and pitch steps.
    fn walk_input(&mut self) -> Option<(Walk, [f32; 3])> {
        let walk = self.walk.as_mut()?;
        let thousandths = |value: i64| value as f32 / 1000.0;
        let step = walk.step.map(thousandths);
        self.controls.yaw += thousandths(walk.turn);
        self.controls.pitch += thousandths(walk.tilt);
        let current = *walk;
        walk.remaining = walk.remaining.saturating_sub(1);
        if walk.remaining == 0 {
            self.walk = None;
        }
        Some((current, step))
    }

    /// One tick: move the body, then the `PlayerAuthInput` that reports it
    /// and the sub-chunk requests the new position makes due.
    fn auth_input(&mut self) -> Result<Vec<McpePacket>> {
        use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
        self.tick += 1;
        for (offset, drift) in self.offset.iter_mut().zip(self.drift) {
            *offset += drift;
        }
        let script = match &mut self.script {
            Some(script) => Some(script.tick(self.tick, self.runtime_id)?),
            None => None,
        };
        if let Some((yaw, pitch)) = script.as_ref().and_then(|tick| tick.look) {
            self.controls.yaw = yaw;
            self.controls.pitch = pitch;
        }
        if let Some(script) = &script {
            match script.input {
                Some(input) => {
                    self.controls = input.controls;
                    self.jump_ticks = u32::from(input.controls.jump);
                    self.glide |= input.glide;
                    if let Some(script) = &self.script {
                        self.offset = script.offset;
                    }
                }
                None => {
                    self.controls = Controls {
                        yaw: self.controls.yaw,
                        pitch: self.controls.pitch,
                        ..Default::default()
                    };
                    self.offset = [0.0; 3];
                }
            }
        }
        self.follow_late_teleport();
        let walk = self.walk_input();
        let controls = self.tick_controls();
        let previous = std::mem::replace(&mut self.previous_controls, controls);
        let key_derived = script.as_ref().is_some_and(|tick| tick.input.is_some())
            && self
                .script
                .as_ref()
                .is_some_and(|script| script.flags == FlagSource::Keys);
        let glide_key = self.glide;
        let started_gliding = std::mem::take(&mut self.glide)
            && !self.body.on_ground
            && !self.body.gliding
            && !self.body.in_water;
        if started_gliding {
            self.body.gliding = true;
        }
        let was_in_water = self.body.in_water;
        let changes = if let Some((_, step)) = walk {
            for (feet, step) in self.body.feet.iter_mut().zip(step) {
                *feet += step;
            }
            self.rest = false;
            physics::Changes::default()
        } else if std::mem::take(&mut self.rest) {
            physics::Changes::default()
        } else {
            physics::step(&mut self.body, &controls, &self.world)
        };
        if self.body.in_water != was_in_water {
            info!(tick = self.tick, in_water = self.body.in_water, feet = ?self.body.feet, "water");
        }
        self.history.push_back(Frame {
            tick: self.tick,
            controls,
            step: walk.map(|(_, step)| step),
            after: self.body,
        });
        while self.history.len() > HISTORY {
            self.history.pop_front();
        }
        let actions = self.block_actions();
        let mut input_data = self.held.clone();
        let teleported = std::mem::take(&mut self.teleported);
        let derived = match walk {
            // A walk reports its own keys and ground contact.
            Some((walk, _)) => [
                (walk.forward, Flag::Up),
                (walk.grounded, Flag::Verticalcollision),
                (teleported, Flag::Handledteleport),
            ]
            .into_iter()
            .filter_map(|(on, flag)| on.then_some(flag))
            .collect(),
            None if key_derived => key_flags(&controls, &previous, glide_key, teleported),
            None => movement_flags(
                &controls,
                &self.body,
                &changes,
                self.jump_was_held,
                started_gliding,
                teleported,
            ),
        };
        for flag in self.pressed.drain(..).chain(derived) {
            if !input_data.contains(&flag) {
                input_data.push(flag);
            }
        }
        self.jump_was_held = controls.jump;
        let player_block_actions = if actions.is_empty() {
            None
        } else {
            input_data.push(EnumsPlayerAuthInputPacketPayloadInputData::Performblockactions);
            Some(actions)
        };
        let rotation = Vec2 {
            x: controls.pitch,
            y: controls.yaw,
        };
        // The keys as pressed: BDS applies the sneak's 0.3 itself (a vector
        // already scaled is slowed again on its diagonals). A walk presses
        // forward or nothing.
        let move_vector = match walk {
            Some((walk, _)) => Vec2 {
                x: 0.0,
                y: if walk.forward { 1.0 } else { 0.0 },
            },
            None => Vec2 {
                x: controls.strafe,
                y: controls.forward,
            },
        };
        let [vx, vy, vz] = self.body.velocity;
        let (yaw, pitch) = (controls.yaw.to_radians(), controls.pitch.to_radians());
        let mut packets = vec![McpePacket::from(PlayerAuthInputPacket {
            player_rotation: rotation.clone(),
            player_head_rotation: controls.yaw,
            interact_rotation: rotation,
            position: self.position(),
            move_vector: move_vector.clone(),
            analog_move_vector: move_vector.clone(),
            raw_move_vector: move_vector,
            pos_delta: Vec3 {
                x: vx,
                y: vy,
                z: vz,
            },
            camera_orientation: Vec3 {
                x: -yaw.sin() * pitch.cos(),
                y: -pitch.sin(),
                z: yaw.cos() * pitch.cos(),
            },
            input_data,
            input_mode: EnumsInputMode::Mouse,
            play_mode: EnumsClientPlayMode::Normal,
            new_interaction_model: EnumsNewInteractionModel::Crosshair,
            client_tick: PlayerInputTick {
                inputtick: self.tick,
            },
            player_block_actions,
            ..Default::default()
        })];
        packets.extend(
            self.world
                .request_near(self.body.feet)
                .into_iter()
                .map(McpePacket::from),
        );
        for command in script.into_iter().flat_map(|s| s.commands) {
            packets.push(self.command(command));
        }
        if self.script.as_ref().is_some_and(Script::done) {
            self.quit = true;
        }
        Ok(packets)
    }

    /// Set the body's state after input `tick` to what the server says and
    /// replay the inputs sent since, as the client's rewind does. Returns
    /// the state the client had predicted for that tick.
    fn rewind(&mut self, tick: u64, apply: impl FnOnce(&mut Body)) -> Result<Body> {
        let index = self.history.iter().position(|frame| frame.tick == tick);
        let Some(index) = index else {
            // Older than the history, or not sent yet: it is the state now.
            let predicted = self.body;
            apply(&mut self.body);
            self.history.clear();
            return Ok(predicted);
        };
        let predicted = self.history[index].after;
        let mut body = predicted;
        apply(&mut body);
        self.history[index].after = body;
        for frame in self.history.iter_mut().skip(index + 1) {
            match frame.step {
                Some(step) => {
                    for (feet, step) in body.feet.iter_mut().zip(step) {
                        *feet += step;
                    }
                }
                None => {
                    physics::step(&mut body, &frame.controls, &self.world);
                }
            }
            frame.after = body;
        }
        self.body = body;
        Ok(predicted)
    }

    /// A teleport after input `tick`: BDS does not move the player for the
    /// next input (it answers an input already on its way with a correction
    /// to the teleport, and simulates nothing for one that handles it), so
    /// the body rests at the destination through input `tick + 1` and the
    /// inputs after it run from there.
    fn teleport(&mut self, tick: u64, position: [f32; 3], on_ground: bool) -> Result<()> {
        let place = |body: &mut Body| {
            body.feet = body.feet_from_network(position);
            body.velocity = [0.0; 3];
            body.on_ground = on_ground;
        };
        if self.history.iter().any(|frame| frame.tick == tick + 1) {
            self.rewind(tick + 1, place)?;
        } else {
            place(&mut self.body);
            self.history.clear();
            self.rest = true;
        }
        Ok(())
    }

    /// `CorrectPlayerMovePrediction`: BDS's state after that input.
    fn correct(
        &mut self,
        packet: &jolyne::valentine::CorrectPlayerMovePredictionPacket,
    ) -> Result<()> {
        use jolyne::valentine::EnumsRewindType;
        if packet.prediction_type != EnumsRewindType::Player {
            warn!(?packet, "vehicle correction ignored");
            return Ok(());
        }
        let tick = packet.tick.inputtick;
        let position = [packet.pos.x, packet.pos.y, packet.pos.z];
        let velocity = [packet.pos_delta.x, packet.pos_delta.y, packet.pos_delta.z];
        if !self.follow_corrections {
            self.corrections += 1;
            warn!(
                tick,
                ?position,
                ?velocity,
                on_ground = packet.on_ground,
                "BDS correction ignored"
            );
            return Ok(());
        }
        let predicted = self.rewind(tick, |body| {
            body.feet = body.feet_from_network(position);
            body.velocity = velocity;
            body.on_ground = packet.on_ground;
        })?;
        let feet = predicted.feet_from_network(position);
        let agreed = (0..3).all(|axis| {
            (feet[axis] - predicted.feet[axis]).abs() < 1.0e-4
                && (velocity[axis] - predicted.velocity[axis]).abs() < 1.0e-4
        });
        if agreed {
            // The answer to an input sent before a teleport arrived.
            info!(tick, "BDS confirmed the predicted state");
            return Ok(());
        }
        self.corrections += 1;
        warn!(
            tick,
            predicted_feet = ?predicted.feet,
            corrected_feet = ?feet,
            predicted_velocity = ?predicted.velocity,
            corrected_velocity = ?velocity,
            on_ground = packet.on_ground,
            "BDS corrected the movement"
        );
        Ok(())
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
                let position = [packet.position.x, packet.position.y, packet.position.z];
                info!(
                    tick = packet.tick.inputtick,
                    ?position,
                    mode = ?packet.position_mode,
                    probe = ?self.teleport_mode,
                    "server teleport"
                );
                if let Some(script) = &mut self.script {
                    script.teleported(packet.tick.inputtick);
                }
                match self.teleport_mode {
                    TeleportMode::Follow => {
                        self.teleport(packet.tick.inputtick, position, packet.on_ground)?;
                        self.controls.pitch = packet.rotation.x;
                        self.controls.yaw = packet.rotation.y;
                        self.teleported = true;
                    }
                    TeleportMode::Late(inputs) => {
                        self.pending_teleport = Some(PendingTeleport {
                            inputs,
                            position,
                            on_ground: packet.on_ground,
                            pitch: packet.rotation.x,
                            yaw: packet.rotation.y,
                        });
                    }
                    TeleportMode::Ignore => {}
                    TeleportMode::Fake => self.teleported = true,
                }
            }
            McpePacketData::SetActorMotionPacket(packet)
                if packet.target_runtime_id.actor_runtime_id == self.runtime_id =>
            {
                self.body.velocity = [packet.motion.x, packet.motion.y, packet.motion.z];
            }
            McpePacketData::UpdateAttributesPacket(packet)
                if packet.target_runtime_id.actor_runtime_id == self.runtime_id =>
            {
                if let Some(movement) = packet
                    .attribute_list
                    .iter()
                    .find(|attribute| attribute.name == "minecraft:movement")
                {
                    info!(
                        speed = movement.current_value,
                        tick = packet.tick.inputtick,
                        "movement speed"
                    );
                    self.body.movement_speed = movement.current_value;
                }
            }
            McpePacketData::SetActorDataPacket(packet)
                if packet.target_runtime_id.actor_runtime_id == self.runtime_id =>
            {
                for row in &packet.actor_data.data {
                    if let (0, DataItemEntryPayload::DataItemInt64Payload(flags)) =
                        (row.id, &row.payload)
                    {
                        let bit = |flag: u32| flags.value as u64 & (1u64 << flag) != 0;
                        info!(
                            tick = packet.tick.inputtick,
                            sneaking = bit(actor_flags::SNEAKING),
                            sprinting = bit(actor_flags::SPRINTING),
                            swimming = bit(actor_flags::SWIMMING),
                            gliding = bit(actor_flags::GLIDING),
                            "server flags"
                        );
                    }
                }
            }
            McpePacketData::MobEffectPacket(packet)
                if packet.target_runtime_id.actor_runtime_id == self.runtime_id =>
            {
                let on = packet.event_id != EnumsMobEffectPacketPayloadEvent::Remove;
                // Effect ids 8 (jump boost) and 27 (slow falling).
                match packet.effect_id {
                    8 => {
                        self.body.jump_boost = if on {
                            u8::try_from(packet.effect_amplifier + 1)?
                        } else {
                            0
                        }
                    }
                    27 => self.body.slow_falling = on,
                    _ => {}
                }
            }
            McpePacketData::LevelChunkPacket(packet) => {
                return Ok(self
                    .world
                    .level_chunk(&packet, self.body.feet)?
                    .into_iter()
                    .map(McpePacket::from)
                    .collect());
            }
            McpePacketData::SubChunkPacket(packet) => self.world.sub_chunk(&packet)?,
            McpePacketData::UpdateBlockPacket(packet) => {
                let pos = &packet.block_position;
                self.world.set_block(
                    [pos.x, pos.y, pos.z],
                    packet.block_runtime_id,
                    packet.layer as usize,
                )?;
            }
            McpePacketData::UpdateSubChunkBlocksPacket(packet) => {
                for (layer, entries) in [
                    packet.blocks_changed.blocks_changed_standards,
                    packet.blocks_changed.blocks_changed_extras,
                ]
                .into_iter()
                .enumerate()
                {
                    for entry in entries {
                        let pos = &entry.pos;
                        self.world
                            .set_block([pos.x, pos.y, pos.z], entry.runtime_id, layer)?;
                    }
                }
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
                if let Some((slot, armour)) = self.pending_equip.take() {
                    return Ok(vec![self.equip_request(slot, armour)?]);
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
            McpePacketData::CorrectPlayerMovePredictionPacket(packet) => self.correct(&packet)?,
            McpePacketData::ChangeDimensionPacket(packet) => {
                self.place(&packet.position, false);
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
                EnumsPlayerRespawnState::Readytospawn => {
                    info!(position = ?packet.position, probe = ?self.teleport_mode, "respawn position");
                    if matches!(
                        self.teleport_mode,
                        TeleportMode::Follow | TeleportMode::Late(_)
                    ) {
                        self.place(&packet.position, false);
                    }
                }
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
    let mut config = ClientHandshakeConfig::random(args.addr, &args.name);
    if let Some(uuid) = args.uuid {
        config.uuid = uuid;
    }
    let uuid = config.uuid;
    let script = args
        .scenarios
        .as_deref()
        .map(|path| {
            let offset = <[f32; 3]>::try_from(args.scenario_offset.as_slice())
                .context("--scenario-offset takes x,y,z")?;
            Script::load(path, args.scenario_log.clone(), offset, args.flag_source)
        })
        .transpose()?;
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
            run_session(play, game, uuid, script).await
        } else {
            let (play, game) = tokio::time::timeout(Duration::from_secs(args.timeout), async {
                BedrockStream::connect(args.addr).await?.join(config).await
            })
            .await
            .context("login timed out")?
            .context("login")?;
            info!("spawned as {}", args.name);
            run_session(play, game, uuid, script).await
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
    uuid: uuid::Uuid,
    script: Option<Script>,
) -> Result<()> {
    let mut player = Player::new(
        game.start_game.runtime_id.actor_runtime_id,
        game.start_game.position.clone(),
        game.start_game.rotation.clone(),
        uuid,
    );
    player.script = script;
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                for packet in player.auth_input()? {
                    play.send_packet(packet).await?;
                }
                if player.quit {
                    info!(corrections = player.corrections, "quitting");
                    play.close().await?;
                    return Ok(());
                }
            }
            packet = play.recv_packet_raw() => {
                let packet = match packet {
                    Ok(packet) => packet,
                    Err(JolyneError::Disconnected(disconnect)) => {
                        info!(
                            corrections = player.corrections,
                            "server closed the session: {:?}",
                            disconnect.packet.body()
                        );
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
                            | McpePacketName::SetActorMotionPacket
                            | McpePacketName::UpdateAttributesPacket
                            | McpePacketName::SetActorDataPacket
                            | McpePacketName::MobEffectPacket
                            | McpePacketName::LevelChunkPacket
                            | McpePacketName::SubChunkPacket
                            | McpePacketName::UpdateBlockPacket
                            | McpePacketName::UpdateSubChunkBlocksPacket
                    )
                ) {
                    for reply in player.handle(packet.decode()?.data)? {
                        play.send_packet(reply).await?;
                    }
                    if player.quit {
                        info!(corrections = player.corrections, "quitting");
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
        Player::new(1, Vec3::default(), Vec2::default(), uuid::Uuid::nil())
    }

    #[test]
    fn walk_orders_step_position_and_yaw_each_input() {
        let order = parse_order(r#"{ message: "bds-sweep:walk 200 0 -100 10000 2 3" }"#).unwrap();
        let mut player = player();
        player.order(order).unwrap();
        let first = player.auth_input().unwrap();
        let McpePacketData::PlayerAuthInputPacket(input) = &first[0].data else {
            panic!("an input first: {first:?}");
        };
        use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
        assert_eq!(input.input_data, vec![Flag::Up, Flag::Verticalcollision]);
        assert_eq!(input.move_vector, Vec2 { x: 0.0, y: 1.0 });
        // The walk's two inputs, then one with the body at rest (the
        // world around it is not loaded).
        for _ in 0..2 {
            player.auth_input().unwrap();
        }
        assert!((player.position().x - 0.4).abs() < 1e-6);
        assert!((player.position().z + 0.2).abs() < 1e-6);
        assert!((player.controls.yaw - 20.0).abs() < 1e-4);
        assert!(player.walk.is_none(), "the walk ends after its inputs");
        let tilted = parse_order("bds-sweep:walk 0 0 0 0 2 2 -1500").unwrap();
        player.order(tilted).unwrap();
        player.auth_input().unwrap();
        player.auth_input().unwrap();
        assert!((player.controls.pitch + 3.0).abs() < 1e-4);
    }

    #[test]
    fn probe_orders_parse() {
        assert_eq!(
            parse_order("bds-sweep:teleports late 3"),
            Some(Order::Teleports(TeleportMode::Late(3)))
        );
        assert_eq!(
            parse_order(r#"{ message: "bds-sweep:teleports ignore" }"#),
            Some(Order::Teleports(TeleportMode::Ignore))
        );
        assert_eq!(
            parse_order("bds-sweep:corrections ignore"),
            Some(Order::Corrections(false))
        );
        assert_eq!(
            parse_order("bds-sweep:offset 0 -450 300"),
            Some(Order::Offset([0, -450, 300]))
        );
        assert_eq!(parse_order("bds-sweep:teleports sometimes"), None);
    }

    #[test]
    fn a_late_teleport_is_followed_and_acknowledged_after_its_inputs() {
        use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
        let mut player = player();
        player
            .order(Order::Teleports(TeleportMode::Late(2)))
            .unwrap();
        player
            .handle(McpePacketData::MovePlayerPacket(
                jolyne::valentine::MovePlayerPacket {
                    player_runtime_id: ActorRuntimeId {
                        actor_runtime_id: 1,
                    },
                    position: Vec3 {
                        x: 5.0,
                        y: 1.62001,
                        z: 0.0,
                    },
                    ..Default::default()
                },
            ))
            .unwrap();
        let flags = |packets: Vec<McpePacket>| match &packets[0].data {
            McpePacketData::PlayerAuthInputPacket(input) => (
                input.position.x,
                input.input_data.contains(&Flag::Handledteleport),
            ),
            other => panic!("not an input: {other:?}"),
        };
        assert_eq!(flags(player.auth_input().unwrap()), (0.0, false));
        assert_eq!(flags(player.auth_input().unwrap()), (0.0, false));
        assert_eq!(flags(player.auth_input().unwrap()), (5.0, true));
        assert_eq!(flags(player.auth_input().unwrap()), (5.0, false));
    }

    #[test]
    fn an_offset_moves_only_the_reported_position() {
        let mut player = player();
        player.order(Order::Offset([0, 0, 600])).unwrap();
        assert!((player.position().z - 0.6).abs() < 1e-6);
        assert_eq!(player.body.feet[2], 0.0);
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
            Some(Order::Release(vec![Flag::Sprinting]))
        );
        assert_eq!(parse_order(&tellraw("use")), Some(Order::UseItem));
        assert_eq!(parse_order(&tellraw("quit")), Some(Order::Quit));
        assert_eq!(
            parse_order(&tellraw("equip-head 3")),
            Some(Order::Equip { slot: 3, armour: 0 })
        );
        assert_eq!(parse_order(&tellraw("press nothing")), None);
        assert_eq!(parse_order(&tellraw("user")), None);
    }

    #[test]
    fn held_flags_repeat_until_released_and_pressed_flags_go_once() {
        use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
        let mut player = player();
        let flags = |player: &mut Player| match &player.auth_input().unwrap()[0].data {
            McpePacketData::PlayerAuthInputPacket(input) => input.input_data.clone(),
            other => panic!("expected input, got {other:?}"),
        };
        player.order(Order::Hold(vec![Flag::Sneaking])).unwrap();
        player
            .order(Order::Press(vec![Flag::Startsneaking]))
            .unwrap();
        assert_eq!(flags(&mut player), [Flag::Sneaking, Flag::Startsneaking]);
        assert_eq!(flags(&mut player), [Flag::Sneaking]);
        player.order(Order::Release(vec![Flag::Sneaking])).unwrap();
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

    /// A player standing on a stone floor at y -61 (feet at -60), with the
    /// sub-chunks around it known.
    fn standing_player() -> Player {
        let mut player = player();
        // Stone from y -64 up to -61 (the floor's top at -60) around the
        // origin, air above.
        let stone = unastar_data::blocks::get("minecraft:stone")
            .unwrap()
            .min_state_id;
        let floor = (-32..32).flat_map(|x| {
            (-32..32).flat_map(move |z| (-64..-60).map(move |y| ([x, y, z], stone, 0)))
        });
        player.world = world::World::with_blocks(floor, 2).unwrap();
        player.body.feet = [0.5, -60.0, 0.5];
        player.body.on_ground = true;
        player
    }

    fn input(player: &mut Player) -> PlayerAuthInputPacket {
        match player.auth_input().unwrap().remove(0).data {
            McpePacketData::PlayerAuthInputPacket(input) => *input,
            other => panic!("expected input, got {other:?}"),
        }
    }

    #[test]
    fn movement_orders_parse_from_tellraw_text() {
        let tellraw = |order: &str| {
            format!(
                "{:?}",
                format!("{{\"rawtext\":[{{\"text\":\"bds-sweep:{order}\"}}]}}\n")
            )
        };
        assert_eq!(
            parse_order(&tellraw("move 1 -0.5")),
            Some(Order::Move {
                forward: 1.0,
                strafe: -0.5
            })
        );
        assert_eq!(parse_order(&tellraw("move 2 0")), None);
        assert_eq!(
            parse_order(&tellraw("look -90 30.5")),
            Some(Order::Look {
                yaw: -90.0,
                pitch: 30.5
            })
        );
        assert_eq!(parse_order(&tellraw("look 0 91")), None);
        assert_eq!(
            parse_order(&tellraw("sprint on")),
            Some(Order::Sprint(true))
        );
        assert_eq!(
            parse_order(&tellraw("sneak off")),
            Some(Order::Sneak(false))
        );
        assert_eq!(parse_order(&tellraw("jump")), Some(Order::Jump(1)));
        assert_eq!(parse_order(&tellraw("jump 40")), Some(Order::Jump(40)));
        assert_eq!(parse_order(&tellraw("glide")), Some(Order::Glide));
        assert_eq!(
            parse_order(&tellraw("equip-feet 2")),
            Some(Order::Equip { slot: 2, armour: 3 })
        );
    }

    #[test]
    fn walking_reports_keys_position_and_velocity_like_a_vanilla_client() {
        use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
        let mut player = standing_player();
        // At rest on the floor: the eyes 1.62001 up, gravity's -0.0784.
        input(&mut player);
        let still = input(&mut player);
        assert_eq!(still.position.y, -60.0 + 1.620_01);
        assert_eq!(still.pos_delta.y, -0.0784);
        assert_eq!(still.input_data, [Flag::Verticalcollision]);
        player
            .order(Order::Move {
                forward: 1.0,
                strafe: 0.0,
            })
            .unwrap();
        player.order(Order::Sneak(true)).unwrap();
        let first = input(&mut player);
        assert_eq!(
            first.input_data,
            [
                Flag::Up,
                Flag::Sneakdown,
                Flag::Sneakcurrentraw,
                Flag::Sneaking,
                Flag::Startsneaking,
                Flag::Sneakpressedraw,
                Flag::Verticalcollision
            ]
        );
        // The keys as pressed: BDS applies the sneak's 0.3 itself.
        assert_eq!(first.move_vector.y, 1.0);
        assert_eq!(first.raw_move_vector.y, 1.0);
        assert!((first.position.z - (0.5 + 0.1 * 0.98 * 0.3)).abs() < 1e-6);
    }

    #[test]
    fn a_teleport_rests_the_next_input_and_acknowledges_it() {
        use EnumsPlayerAuthInputPacketPayloadInputData as Flag;
        let mut player = standing_player();
        input(&mut player);
        let last = player.tick;
        player
            .handle(McpePacketData::MovePlayerPacket(
                jolyne::valentine::MovePlayerPacket {
                    player_runtime_id: ActorRuntimeId {
                        actor_runtime_id: player.runtime_id,
                    },
                    position: Vec3 {
                        x: 0.5,
                        y: -40.0 + 1.620_01,
                        z: 0.5,
                    },
                    on_ground: true,
                    tick: PlayerInputTick { inputtick: last },
                    ..Default::default()
                },
            ))
            .unwrap();
        // The first input after it stays at the destination at rest.
        let rest = input(&mut player);
        assert_eq!(rest.position.y, -40.0 + 1.620_01);
        assert_eq!(rest.pos_delta.y, 0.0);
        assert!(rest.input_data.contains(&Flag::Handledteleport));
        // Then it falls.
        let next = input(&mut player);
        assert_eq!(next.position.y, -40.0 + 1.620_01);
        assert_eq!(next.pos_delta.y, -0.0784);
        let falling = input(&mut player);
        assert!(falling.position.y < -40.0 + 1.620_01);
        assert!(!falling.input_data.contains(&Flag::Handledteleport));
    }

    #[test]
    fn a_correction_rewinds_to_its_tick_and_replays_the_later_inputs() {
        let mut player = standing_player();
        player
            .order(Order::Move {
                forward: 1.0,
                strafe: 0.0,
            })
            .unwrap();
        for _ in 0..5 {
            input(&mut player);
        }
        let corrected_tick = player.tick - 2;
        let before = player.body;
        player
            .handle(McpePacketData::CorrectPlayerMovePredictionPacket(
                jolyne::valentine::CorrectPlayerMovePredictionPacket {
                    pos: Vec3 {
                        x: 0.5,
                        y: -60.0 + 1.620_01,
                        z: 0.0,
                    },
                    pos_delta: Vec3::default(),
                    on_ground: true,
                    tick: PlayerInputTick {
                        inputtick: corrected_tick,
                    },
                    ..Default::default()
                },
            ))
            .unwrap();
        assert_eq!(player.corrections, 1);
        // Two inputs replayed from rest at z 0: well short of where it was.
        assert!(player.body.feet[2] > 0.0 && player.body.feet[2] < before.feet[2] - 0.5);
        assert_eq!(player.history.len(), 5);
    }
}
