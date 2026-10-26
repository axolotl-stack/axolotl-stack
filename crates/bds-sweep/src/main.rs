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
//! * `bds-sweep:attack RUNTIME_ID`: attack that actor with the held item
//!   (`InventoryTransaction` use-item-on-actor, hotbar slot 0 as last sent
//!   by the server).
//! * `bds-sweep:drop hotbar SLOT COUNT`: closed-screen Q (`COUNT=1`) or
//!   ctrl-Q (`COUNT=stack size`) from hotbar index 0..8;
//! * `bds-sweep:drop inventory SLOT COUNT`: open the native inventory, wait
//!   for `ContainerOpen`, then drop from main-inventory index 0..35;
//! * `bds-sweep:close-inventory`: close the inventory opened by this client.
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
        ContainerClosePacket, EnumsClientPlayMode, EnumsContainerEnumName, EnumsInputMode,
        EnumsInteractPacketPayloadAction, EnumsItemStackRequestActionType,
        EnumsItemUseOnActorInventoryTransactionActionType, EnumsNewInteractionModel,
        EnumsPlayerActionType, EnumsPlayerAuthInputPacketPayloadInputData, EnumsPlayerRespawnState,
        FullContainerName, InteractPacket, InventoryTransactionPacket,
        InventoryTransactionPacketTransaction, ItemStackRequestCerealDropActionData,
        ItemStackRequestCerealSlotInfoData, ItemStackRequestPacket,
        ItemStackRequestPacketDataRequestData, ItemStackRequestPacketDataRequestDataActionsItem,
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum Order {
    Mine(Mining),
    Stop,
    /// Attack the actor with this runtime id with the held item.
    Attack(u64),
    Drop(DropOrder),
    CloseInventory,
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
        _ => None,
    }
}

/// The own player's state needed to tick like a vanilla client.
struct Player {
    runtime_id: u64,
    position: Vec3,
    /// Pitch (x) and yaw (y), as the server last set them.
    rotation: Vec2,
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
            Order::Attack(target) => return Ok(vec![self.attack(target)]),
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
        let mut input_data = Vec::new();
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
            run_session(play, game).await
        } else {
            let (play, game) = tokio::time::timeout(Duration::from_secs(args.timeout), async {
                BedrockStream::connect(args.addr).await?.join(config).await
            })
            .await
            .context("login timed out")?
            .context("login")?;
            info!("spawned as {}", args.name);
            run_session(play, game).await
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
) -> Result<()> {
    let mut player = Player {
        runtime_id: game.start_game.runtime_id.actor_runtime_id,
        position: game.start_game.position.clone(),
        rotation: game.start_game.rotation.clone(),
        tick: 0,
        mining: None,
        pending: None,
        inventory: std::array::from_fn(|_| None),
        selected_slot: 0,
        inventory_window: None,
        pending_drop: None,
        next_request_id: -1,
    };
    let mut ticker = tokio::time::interval(Duration::from_millis(50));
    loop {
        tokio::select! {
            _ = ticker.tick() => play.send_packet(player.auth_input()).await?,
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
            mining: None,
            pending: None,
            inventory: std::array::from_fn(|_| None),
            selected_slot: 0,
            inventory_window: None,
            pending_drop: None,
            next_request_id: -1,
        }
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
