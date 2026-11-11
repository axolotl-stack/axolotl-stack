//! Several scripted players in one process, for packet-order measurements.
//!
//! Joins three offline players (`RaceA`, `RaceB`, `RaceC`) to one server and
//! plays a script in which every line says which player sends what, and in
//! which order, within one 50 ms client tick. Because one task sends for every
//! player, the order of packets that reach the server within a tick is the
//! script's order. Packets on one script line are sent as one game batch;
//! separate lines are separate batches.
//!
//! Script lines: `TICK PLAYER ACTION [; ACTION ...]`, `PLAYER` one of `A`,
//! `B`, `C` (or `*` for `mark LABEL` and `exit`). Actions:
//!
//! * `input [move=X,Z] [pos=X,Y,Z] [dpos=DX,DY,DZ] [rot=PITCH,YAW] [flags=F|F]`:
//!   a `PlayerAuthInput` (`pos` and `dpos` change the position the player
//!   reports from then on; `flags` are input-data names such as
//!   `startsprinting`);
//! * `noinput`: send no input this tick (the client tick does not advance);
//! * `attack P [from=X,Y,Z]`: an `InventoryTransaction` attack on player `P`
//!   with the held (empty) hand;
//! * `swing`: an `Animate` swing;
//! * `useitem`: an `InventoryTransaction` item use in the air with the held item;
//! * `interact`: an `Interact` asking to open the player's own inventory;
//! * `close`: a `ContainerClose` of the window the server last opened for the player;
//! * `cmd TEXT`: a `CommandRequest` (the rest of the action);
//! * `say TEXT`: a chat `Text` packet.
//!
//! Every player that has no `input` or `noinput` on a tick sends one idle
//! input first, in the order A, B, C, before the tick's script lines.
//!
//! Output: `OUT/events.jsonl`, one record per packet sent or received (all of
//! them except chunk and other bulk packets) with a global sequence number,
//! microseconds since start, the script tick, the player, the direction, the
//! batch number of sent packets, the packet's `Debug` form and its hex
//! (header included, no batch length).

use anyhow::{Context, Result, bail, ensure};
use clap::Parser;
use jolyne::raw::RawPacket;
use jolyne::stream::client::ClientHandshakeConfig;
use jolyne::{
    BedrockStream, Client, JolyneError, Play,
    stream::RakNetTransport,
    valentine::{
        ActorRuntimeId, AnimatePacket, CerealizerNetworkItemStackDescriptorSerializedData,
        CommandOriginDatajson, CommandRequestPacket, ContainerClosePacket,
        EnumsAnimatePacketPayloadAction, EnumsClientPlayMode, EnumsHandSlot, EnumsInputMode,
        EnumsInteractPacketPayloadAction, EnumsItemUseInventoryTransactionActionType,
        EnumsItemUseInventoryTransactionPredictedResult,
        EnumsItemUseInventoryTransactionTriggerType,
        EnumsItemUseOnActorInventoryTransactionActionType, EnumsNewInteractionModel,
        EnumsPlayerActionType, EnumsPlayerAuthInputPacketPayloadInputData as Flag,
        EnumsPlayerRespawnState, InteractPacket, InventoryTransactionPacket,
        InventoryTransactionPacketTransaction, ItemUseInventoryTransaction,
        ItemUseOnActorInventoryTransaction, McpePacket, McpePacketData, McpePacketName,
        PlayerActionPacket, PlayerAuthInputPacket, PlayerInputTick, RespawnPacket, TextPacket,
        TextPacketBody, TextPacketPayloadAuthorAndMessage,
        TextPacketPayloadAuthorAndMessageMessageType, Vec2, Vec3,
    },
};
use std::fmt::Write as _;
use std::io::{BufWriter, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tracing::info;

#[derive(Parser, Debug)]
#[command(about = "Three scripted players in one process, for packet-order captures")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:19132")]
    addr: SocketAddr,
    /// The script to play.
    #[arg(long)]
    script: PathBuf,
    /// Output directory (`events.jsonl`).
    #[arg(long)]
    out: PathBuf,
    /// The order in which the players join, as labels (`ABC`, `BAC`, …).
    #[arg(long, default_value = "ABC")]
    join_order: String,
    /// Login timeout in seconds, per player.
    #[arg(long, default_value = "120")]
    timeout: u64,
}

const NAMES: [&str; 3] = ["RaceA", "RaceB", "RaceC"];
const LABELS: [&str; 3] = ["A", "B", "C"];

#[derive(Debug, Clone)]
enum Action {
    Input(InputSpec),
    NoInput,
    Attack { target: usize, from: Option<Vec3> },
    Swing,
    UseItem,
    Interact,
    Close,
    Command(String),
    Say(String),
}

#[derive(Debug, Clone, Default)]
struct InputSpec {
    move_vector: Option<(f32, f32)>,
    pos: Option<[f32; 3]>,
    dpos: Option<[f32; 3]>,
    rot: Option<(f32, f32)>,
    flags: Vec<Flag>,
}

#[derive(Debug, Clone)]
enum Line {
    Send { player: usize, actions: Vec<Action> },
    Mark(String),
    Exit,
}

fn player_index(word: &str) -> Result<usize> {
    LABELS
        .iter()
        .position(|label| label.eq_ignore_ascii_case(word))
        .with_context(|| format!("unknown player {word}"))
}

fn floats<const N: usize>(text: &str) -> Result<[f32; N]> {
    let values: Vec<f32> = text
        .split(',')
        .map(|v| v.trim().parse::<f32>())
        .collect::<Result<_, _>>()
        .with_context(|| format!("numbers in {text}"))?;
    values
        .try_into()
        .map_err(|_| anyhow::anyhow!("{text}: expected {N} numbers"))
}

fn flag(name: &str) -> Result<Flag> {
    Ok(match name.to_ascii_lowercase().as_str() {
        "up" => Flag::Up,
        "down" => Flag::Down,
        "sprinting" => Flag::Sprinting,
        "sprintdown" => Flag::Sprintdown,
        "startsprinting" => Flag::Startsprinting,
        "stopsprinting" => Flag::Stopsprinting,
        "jumping" => Flag::Jumping,
        "jumpdown" => Flag::Jumpdown,
        "startjumping" => Flag::Startjumping,
        "wantup" => Flag::Wantup,
        "jumppressedraw" => Flag::Jumppressedraw,
        "jumpcurrentraw" => Flag::Jumpcurrentraw,
        "missedswing" => Flag::Missedswing,
        "handledteleport" => Flag::Handledteleport,
        "verticalcollision" => Flag::Verticalcollision,
        "horizontalcollision" => Flag::Horizontalcollision,
        "sneaking" => Flag::Sneaking,
        "startsneaking" => Flag::Startsneaking,
        "stopsneaking" => Flag::Stopsneaking,
        other => bail!("unknown input flag {other}"),
    })
}

fn parse_action(text: &str) -> Result<Action> {
    let text = text.trim();
    let (verb, rest) = text.split_once(' ').unwrap_or((text, ""));
    Ok(match verb {
        "input" => {
            let mut spec = InputSpec::default();
            for word in rest.split_whitespace() {
                let (key, value) = word
                    .split_once('=')
                    .with_context(|| format!("input option {word}"))?;
                match key {
                    "move" => {
                        let [x, z] = floats::<2>(value)?;
                        spec.move_vector = Some((x, z));
                    }
                    "pos" => spec.pos = Some(floats::<3>(value)?),
                    "dpos" => spec.dpos = Some(floats::<3>(value)?),
                    "rot" => {
                        let [p, y] = floats::<2>(value)?;
                        spec.rot = Some((p, y));
                    }
                    "flags" => {
                        spec.flags = value.split('|').map(flag).collect::<Result<_>>()?;
                    }
                    other => bail!("unknown input option {other}"),
                }
            }
            Action::Input(spec)
        }
        "noinput" => Action::NoInput,
        "swing" => Action::Swing,
        "useitem" => Action::UseItem,
        "interact" => Action::Interact,
        "close" => Action::Close,
        "attack" => {
            let mut words = rest.split_whitespace();
            let target = player_index(words.next().context("attack target")?)?;
            let mut from = None;
            for word in words {
                match word.split_once('=') {
                    Some(("from", value)) => {
                        let [x, y, z] = floats::<3>(value)?;
                        from = Some(Vec3 { x, y, z });
                    }
                    _ => bail!("unknown attack option {word}"),
                }
            }
            Action::Attack { target, from }
        }
        "cmd" => Action::Command(rest.to_owned()),
        "say" => Action::Say(rest.to_owned()),
        other => bail!("unknown action {other}"),
    })
}

fn parse_script(text: &str) -> Result<Vec<(u64, Line)>> {
    let mut lines = Vec::new();
    for (number, raw) in text.lines().enumerate() {
        let raw = raw.trim();
        if raw.is_empty() || raw.starts_with('#') {
            continue;
        }
        let parsed = (|| -> Result<(u64, Line)> {
            let (tick, rest) = raw.split_once(' ').context("tick")?;
            let tick: u64 = tick.parse().context("tick")?;
            let (who, rest) = rest.trim().split_once(' ').unwrap_or((rest.trim(), ""));
            if who == "*" {
                let (verb, label) = rest.split_once(' ').unwrap_or((rest, ""));
                return Ok((
                    tick,
                    match verb {
                        "mark" => Line::Mark(label.to_owned()),
                        "exit" => Line::Exit,
                        other => bail!("unknown marker {other}"),
                    },
                ));
            }
            let player = player_index(who)?;
            let actions = rest.split(';').map(parse_action).collect::<Result<_>>()?;
            Ok((tick, Line::Send { player, actions }))
        })()
        .with_context(|| format!("script line {}: {raw}", number + 1))?;
        lines.push(parsed);
    }
    ensure!(
        lines.windows(2).all(|w| w[0].0 <= w[1].0),
        "script ticks must not decrease"
    );
    Ok(lines)
}

struct Player {
    stream: BedrockStream<Play, Client, RakNetTransport>,
    runtime_id: u64,
    position: Vec3,
    rotation: Vec2,
    tick: u64,
    /// The server teleported this player; the next input says it was handled.
    teleported: bool,
    /// Hotbar slot 0 as the server last sent it; attacks name it as the held item.
    held: CerealizerNetworkItemStackDescriptorSerializedData,
    /// The window the server last opened for this player (id, type).
    window: Option<(u8, u8)>,
}

impl Player {
    fn input(&mut self, spec: &InputSpec) -> McpePacket {
        self.tick += 1;
        if let Some([x, y, z]) = spec.pos {
            self.position = Vec3 { x, y, z };
        }
        let mut delta = Vec3::default();
        if let Some([dx, dy, dz]) = spec.dpos {
            self.position.x += dx;
            self.position.y += dy;
            self.position.z += dz;
            delta = Vec3 {
                x: dx,
                y: dy,
                z: dz,
            };
        }
        if let Some((pitch, yaw)) = spec.rot {
            self.rotation = Vec2 { x: pitch, y: yaw };
        }
        let mut input_data = spec.flags.clone();
        if std::mem::take(&mut self.teleported) {
            input_data.push(Flag::Handledteleport);
        }
        let (mx, mz) = spec.move_vector.unwrap_or((0.0, 0.0));
        McpePacket::from(PlayerAuthInputPacket {
            player_rotation: self.rotation.clone(),
            player_head_rotation: self.rotation.y,
            interact_rotation: self.rotation.clone(),
            position: self.position.clone(),
            move_vector: Vec2 { x: mx, y: mz },
            analog_move_vector: Vec2 { x: mx, y: mz },
            raw_move_vector: Vec2 { x: mx, y: mz },
            input_data,
            input_mode: EnumsInputMode::Mouse,
            play_mode: EnumsClientPlayMode::Normal,
            new_interaction_model: EnumsNewInteractionModel::Crosshair,
            client_tick: PlayerInputTick {
                inputtick: self.tick,
            },
            pos_delta: delta,
            ..Default::default()
        })
    }

    fn attack(&self, target: u64, from: Option<Vec3>) -> McpePacket {
        McpePacket::from(InventoryTransactionPacket {
            transaction: InventoryTransactionPacketTransaction::ItemUseOnActorInventoryTransaction(
                ItemUseOnActorInventoryTransaction {
                    runtime_id: ActorRuntimeId {
                        actor_runtime_id: target,
                    },
                    action_type: EnumsItemUseOnActorInventoryTransactionActionType::Attack,
                    slot: 0,
                    item: self.held.clone(),
                    from_position: from.unwrap_or_else(|| self.position.clone()),
                    ..Default::default()
                },
            ),
            ..Default::default()
        })
    }

    fn use_item(&self) -> McpePacket {
        McpePacket::from(InventoryTransactionPacket {
            transaction: InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(
                Box::new(ItemUseInventoryTransaction {
                    action_type: EnumsItemUseInventoryTransactionActionType::Use,
                    trigger_type: EnumsItemUseInventoryTransactionTriggerType::Playerinput,
                    face: 255,
                    slot: 0,
                    hand: EnumsHandSlot::Mainhand,
                    item: self.held.clone(),
                    from_position: self.position.clone(),
                    client_interact_prediction:
                        EnumsItemUseInventoryTransactionPredictedResult::Success,
                    ..Default::default()
                }),
            ),
            ..Default::default()
        })
    }

    fn open_inventory(&self) -> McpePacket {
        McpePacket::from(InteractPacket {
            action: EnumsInteractPacketPayloadAction::Openinventory,
            target_runtime_id: ActorRuntimeId {
                actor_runtime_id: self.runtime_id,
            },
            position: None,
        })
    }

    fn close_window(&mut self) -> Option<McpePacket> {
        let (container_id, container_type) = self.window.take()?;
        Some(McpePacket::from(ContainerClosePacket {
            container_id,
            container_type,
            server_initiated_close: false,
        }))
    }

    fn swing(&self) -> McpePacket {
        McpePacket::from(AnimatePacket {
            action: EnumsAnimatePacketPayloadAction::Swing,
            target_actor_runtime_id: ActorRuntimeId {
                actor_runtime_id: self.runtime_id,
            },
            data: 0.0,
            swing_source: Some("attack".into()),
        })
    }

    fn action(&self, action: EnumsPlayerActionType) -> McpePacket {
        McpePacket::from(PlayerActionPacket {
            player_runtime_id: ActorRuntimeId {
                actor_runtime_id: self.runtime_id,
            },
            action,
            ..Default::default()
        })
    }

    /// Replies a vanilla client owes the server, and own-position updates.
    fn handle(&mut self, packet: &McpePacketData) -> Vec<McpePacket> {
        match packet {
            McpePacketData::MovePlayerPacket(packet)
                if packet.player_runtime_id.actor_runtime_id == self.runtime_id =>
            {
                self.position = packet.position.clone();
                self.rotation = packet.rotation.clone();
                self.teleported = true;
            }
            McpePacketData::InventoryContentPacket(content) if content.container_id == 0 => {
                self.held = content.slots.first().cloned().unwrap_or_default();
            }
            McpePacketData::InventorySlotPacket(slot)
                if slot.container_id == 0 && slot.slot == 0 =>
            {
                self.held = slot.item.clone();
            }
            McpePacketData::ContainerOpenPacket(open) => {
                self.window = Some((open.container_id, open.container_type));
            }
            McpePacketData::ContainerClosePacket(close) if close.server_initiated_close => {
                self.window = None;
            }
            McpePacketData::CorrectPlayerMovePredictionPacket(packet) => {
                self.position = packet.pos.clone();
            }
            McpePacketData::RespawnPacket(packet) => match packet.state {
                EnumsPlayerRespawnState::Searchingforspawn => {
                    return vec![
                        McpePacket::from(RespawnPacket {
                            position: packet.position.clone(),
                            state: EnumsPlayerRespawnState::Clientreadytospawn,
                            player_runtime_id: ActorRuntimeId {
                                actor_runtime_id: self.runtime_id,
                            },
                        }),
                        self.action(EnumsPlayerActionType::Respawn),
                    ];
                }
                EnumsPlayerRespawnState::Readytospawn => {
                    self.position = packet.position.clone();
                }
                _ => {}
            },
            McpePacketData::NetworkStackLatencyPacket(latency) if latency.is_from_server => {
                let mut reply = latency.clone();
                reply.is_from_server = false;
                return vec![McpePacket::from(reply)];
            }
            _ => {}
        }
        Vec::new()
    }
}

fn chat(name: &str, text: &str) -> McpePacket {
    McpePacket::from(TextPacket {
        localize: false,
        body: TextPacketBody::AuthorAndMessage(TextPacketPayloadAuthorAndMessage {
            message_type: TextPacketPayloadAuthorAndMessageMessageType::Chat,
            player_name: name.into(),
            message: text.into(),
        }),
        ..Default::default()
    })
}

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

/// Packets not worth a record: world data and other bulk traffic.
fn skipped(name: Option<McpePacketName>) -> bool {
    use McpePacketName as N;
    matches!(
        name,
        Some(
            N::LevelChunkPacket
                | N::SubChunkPacket
                | N::SubChunkRequestPacket
                | N::NetworkChunkPublisherUpdatePacket
                | N::BiomeDefinitionListPacket
                | N::AvailableCommandsPacket
                | N::CraftingDataPacket
                | N::CreativeContentPacket
                | N::NetworkStackLatencyPacket
                | N::PlayerListPacket
                | N::ResourcePackStackPacket
                | N::SetTimePacket
        )
    )
}

/// Hex of a packet's inner frame without its length prefix.
fn frame_hex(frame: &[u8]) -> String {
    let mut i = 0;
    while i < frame.len() && frame[i] & 0x80 != 0 {
        i += 1;
    }
    let mut out = String::with_capacity((frame.len() - i) * 2);
    for byte in &frame[(i + 1).min(frame.len())..] {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

struct Log {
    out: BufWriter<std::fs::File>,
    started: Instant,
    seq: u64,
    batch: u64,
}

impl Log {
    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        tick: u64,
        player: usize,
        dir: &str,
        batch: Option<u64>,
        name: &str,
        debug: &str,
        hex: &str,
    ) -> Result<()> {
        let t_us = self.started.elapsed().as_micros();
        let batch = batch.map_or("null".to_owned(), |b| b.to_string());
        writeln!(
            self.out,
            "{{\"seq\":{},\"t_us\":{t_us},\"tick\":{tick},\"player\":\"{}\",\"dir\":\"{dir}\",\"batch\":{batch},\"name\":\"{name}\",\"debug\":{},\"hex\":\"{hex}\"}}",
            self.seq,
            LABELS[player],
            json_string(debug),
        )?;
        self.seq += 1;
        Ok(())
    }

    fn mark(&mut self, tick: u64, label: &str) -> Result<()> {
        let t_us = self.started.elapsed().as_micros();
        writeln!(
            self.out,
            "{{\"seq\":{},\"t_us\":{t_us},\"tick\":{tick},\"mark\":{}}}",
            self.seq,
            json_string(label)
        )?;
        self.seq += 1;
        Ok(())
    }
}

/// Sends `packets` from `player` as one batch (or one packet), recording them.
async fn send(
    log: &mut Log,
    tick: u64,
    index: usize,
    player: &mut Player,
    packets: Vec<McpePacket>,
) -> Result<()> {
    if packets.is_empty() {
        return Ok(());
    }
    let batch = log.batch;
    log.batch += 1;
    for packet in &packets {
        let raw = RawPacket::from_packet(packet)?;
        let name = format!("{:?}", raw.name());
        log.record(
            tick,
            index,
            "c2s",
            Some(batch),
            name.trim_start_matches("Some(").trim_end_matches(')'),
            &format!("{:?}", packet.data),
            &frame_hex(raw.inner_frame()),
        )?;
    }
    if packets.len() == 1 {
        player
            .stream
            .send_packet(packets.into_iter().next().unwrap())
            .await?;
    } else {
        player.stream.send_batch(&packets, true).await?;
    }
    Ok(())
}

async fn received(
    log: &mut Log,
    tick: u64,
    index: usize,
    player: &mut Player,
    packet: Result<RawPacket, JolyneError>,
) -> Result<bool> {
    let packet = match packet {
        Ok(packet) => packet,
        Err(JolyneError::Disconnected(disconnect)) => {
            info!(
                "{} disconnected: {:?}",
                NAMES[index],
                disconnect.packet.body()
            );
            return Ok(false);
        }
        Err(error) => return Err(error.into()),
    };
    let name = packet.name();
    if name.is_none() {
        return Ok(true);
    }
    let hex = if skipped(name) {
        None
    } else {
        Some(frame_hex(packet.inner_frame()))
    };
    let decoded = match packet.decode() {
        Ok(packet) => packet.data,
        Err(error) => {
            log.record(
                tick,
                index,
                "s2c",
                None,
                &format!("{name:?}"),
                &format!("decode error: {error}"),
                &hex.unwrap_or_default(),
            )?;
            return Ok(true);
        }
    };
    if let Some(hex) = hex {
        let label = format!("{name:?}");
        log.record(
            tick,
            index,
            "s2c",
            None,
            label.trim_start_matches("Some(").trim_end_matches(')'),
            &format!("{decoded:?}"),
            &hex,
        )?;
    }
    let replies = player.handle(&decoded);
    if !replies.is_empty() {
        send(log, tick, index, player, replies).await?;
    }
    Ok(true)
}

async fn join(args: &Args, index: usize) -> Result<Player> {
    let config = ClientHandshakeConfig::random(args.addr, NAMES[index]);
    let (stream, game) = tokio::time::timeout(Duration::from_secs(args.timeout), async {
        BedrockStream::connect(args.addr).await?.join(config).await
    })
    .await
    .context("login timed out")?
    .with_context(|| format!("login of {}", NAMES[index]))?;
    info!(
        "{} spawned, runtime id {}",
        NAMES[index], game.start_game.runtime_id.actor_runtime_id
    );
    Ok(Player {
        stream,
        runtime_id: game.start_game.runtime_id.actor_runtime_id,
        position: game.start_game.position.clone(),
        rotation: game.start_game.rotation.clone(),
        tick: 0,
        teleported: false,
        held: Default::default(),
        window: None,
    })
}

/// One script tick: idle inputs of players without a scripted one, then the
/// tick's lines in order. Returns false on `exit`.
async fn play_tick(
    log: &mut Log,
    tick: u64,
    players: &mut [&mut Player; 3],
    lines: &[Line],
) -> Result<bool> {
    let runtime_ids = [
        players[0].runtime_id,
        players[1].runtime_id,
        players[2].runtime_id,
    ];
    let mut scripted = [false; 3];
    for line in lines {
        if let Line::Send { player, actions } = line
            && actions
                .iter()
                .any(|action| matches!(action, Action::Input(_) | Action::NoInput))
        {
            scripted[*player] = true;
        }
    }
    for index in 0..3 {
        if !scripted[index] {
            let packet = players[index].input(&InputSpec::default());
            send(log, tick, index, players[index], vec![packet]).await?;
        }
    }
    for line in lines {
        match line {
            Line::Mark(label) => log.mark(tick, label)?,
            Line::Exit => return Ok(false),
            Line::Send { player, actions } => {
                let player_ref = &mut *players[*player];
                let mut packets = Vec::new();
                for action in actions {
                    match action {
                        Action::Input(spec) => packets.push(player_ref.input(spec)),
                        Action::NoInput => {}
                        Action::Attack { target, from } => {
                            packets.push(player_ref.attack(runtime_ids[*target], from.clone()))
                        }
                        Action::Swing => packets.push(player_ref.swing()),
                        Action::UseItem => packets.push(player_ref.use_item()),
                        Action::Interact => packets.push(player_ref.open_inventory()),
                        Action::Close => packets.extend(player_ref.close_window()),
                        Action::Command(text) => packets.push(command_request(text)),
                        Action::Say(text) => packets.push(chat(NAMES[*player], text)),
                    }
                }
                send(log, tick, *player, player_ref, packets).await?;
            }
        }
    }
    Ok(true)
}

// Match bds-extractor's stack allowance for the large generated packet codecs
// on Windows debug builds.
const STACK_SIZE: usize = 64 * 1024 * 1024;

fn main() -> Result<()> {
    let args = Args::parse();
    std::thread::Builder::new()
        .name("order-probe".into())
        .stack_size(STACK_SIZE)
        .spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .thread_stack_size(STACK_SIZE)
                .build()
                .context("building the probe runtime")?
                .block_on(run(args))
        })
        .context("spawning the probe thread")?
        .join()
        .map_err(|_| anyhow::anyhow!("probe thread panicked"))?
}

async fn run(args: Args) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".parse().expect("static filter")),
        )
        .init();
    let script = parse_script(
        &std::fs::read_to_string(&args.script)
            .with_context(|| format!("reading {}", args.script.display()))?,
    )?;
    std::fs::create_dir_all(&args.out)?;
    let mut log = Log {
        out: BufWriter::new(std::fs::File::create(args.out.join("events.jsonl"))?),
        started: Instant::now(),
        seq: 0,
        batch: 0,
    };
    let mut joined: [Option<Player>; 3] = [None, None, None];
    ensure!(
        args.join_order.len() == 3,
        "--join-order names all three players"
    );
    for label in args.join_order.chars() {
        let index = player_index(&label.to_string())?;
        ensure!(joined[index].is_none(), "{label} joins twice");
        joined[index] = Some(join(&args, index).await?);
    }
    let [Some(mut a), Some(mut b), Some(mut c)] = joined else {
        bail!("--join-order names all three players");
    };
    for (index, player) in [&a, &b, &c].into_iter().enumerate() {
        log.mark(
            0,
            &format!("runtime id {} {}", LABELS[index], player.runtime_id),
        )?;
    }

    // Tokio's timer is too coarse on Windows (50 ms ticks come out ~62 ms
    // apart, fewer inputs than the server's 20 per second); a thread that
    // sleeps to absolute 50 ms deadlines keeps the client at 20 per second.
    let (tick_tx, mut ticks) = tokio::sync::mpsc::unbounded_channel::<()>();
    std::thread::Builder::new()
        .name("order-probe-clock".into())
        .spawn(move || {
            let period = Duration::from_millis(50);
            let mut deadline = Instant::now();
            loop {
                if tick_tx.send(()).is_err() {
                    return;
                }
                deadline += period;
                let now = Instant::now();
                if deadline > now {
                    std::thread::sleep(deadline - now);
                }
            }
        })
        .context("spawning the probe clock")?;
    let mut tick = 0u64;
    let mut next = 0usize;
    loop {
        tokio::select! {
            Some(()) = ticks.recv() => {
                let start = next;
                while next < script.len() && script[next].0 <= tick {
                    next += 1;
                }
                let lines: Vec<Line> = script[start..next].iter().map(|(_, line)| line.clone()).collect();
                let mut players = [&mut a, &mut b, &mut c];
                if !play_tick(&mut log, tick, &mut players, &lines).await? {
                    info!("script finished");
                    break;
                }
                if next >= script.len() {
                    info!("script ran out without exit");
                    break;
                }
                tick += 1;
            }
            packet = a.stream.recv_packet_raw() => {
                if !received(&mut log, tick, 0, &mut a, packet).await? { break; }
            }
            packet = b.stream.recv_packet_raw() => {
                if !received(&mut log, tick, 1, &mut b, packet).await? { break; }
            }
            packet = c.stream.recv_packet_raw() => {
                if !received(&mut log, tick, 2, &mut c, packet).await? { break; }
            }
        }
    }
    log.out.flush()?;
    info!("events: {}", args.out.join("events.jsonl").display());
    Ok(())
}
