//! Scripted client actions: `!` lines of a `--commands` script.
//!
//! A command script mixes slash commands with client packets the extractor
//! sends as a player would, so a capture can show how BDS answers packets no
//! command produces (interactions, emotes, book edits, serverbound packets
//! BDS may ignore). Every action except `!wait` sends packets, and
//! `command-outputs.json` attributes the server packets after the first of
//! them to the action, exactly like a command.
//!
//! | Line | Sends |
//! |---|---|
//! | `!wait MS` | nothing; pumps packets for `MS` milliseconds |
//! | `!interact ACTION TARGET [X Y Z]` | `Interact`; `ACTION` is `mouseover`, `stop-riding`, `npc-open`, `open-inventory`, `invalid` or a number |
//! | `!pick-actor TARGET [MAX_SLOTS] [data]` | `ActorPickRequest` for the target's unique id |
//! | `!pick-block X Y Z [MAX_SLOTS] [data]` | `BlockPickRequest` for the block at X Y Z |
//! | `!equip SLOT` | `MobEquipment` selecting hotbar SLOT with the item the server last put there |
//! | `!use-item SLOT` | `InventoryTransaction` item use on air (`Use`, player input) with hotbar SLOT's item, as a client opening a book |
//! | `!emote EMOTE_ID [FLAGS] [TICKS]` | `Emote` for the player itself |
//! | `!emote-list UUID...` | `EmoteList` for the player itself |
//! | `!animate ACTION [DATA]` | `Animate` for the player itself; `ACTION` is `swing`, `wake-up`, `critical-hit`, `magic-critical-hit` or a number |
//! | `!book SLOT replace\|add PAGE TEXT...` | `BookEdit` replacing or adding a page |
//! | `!book SLOT delete PAGE` / `!book SLOT swap A B` | `BookEdit` deleting or swapping pages |
//! | `!book SLOT sign TITLE AUTHOR` | `BookEdit` signing the book (empty XUID) |
//! | `!blob-status [missing=ID,..] [found=ID,..]` | `ClientCacheBlobStatus` |
//! | `!party-changed [PARTY_ID LEADER]` | `PartyChanged`, without party info when no id is given |
//! | `!packs-ready` | `ResourcePacksReadyForValidation` |
//! | `!ddui-closed FORM_ID REASON` | `ServerboundDataDrivenScreenClosed` |
//! | `!data-store NAME PROPERTY PATH VALUE` | `ServerboundDataStore`; `VALUE` is `true`, `false`, a number or a string |
//! | `!cookie COOKIE true\|false` | `PartyDestinationCookieResponse` |
//!
//! `TARGET` is `self`, `peer` (the last other player an `AddPlayer`
//! announced), `actor` (the last actor an `AddActor` announced) or a runtime
//! id.

use anyhow::{Context, Result, bail, ensure};
use jolyne::valentine::types::{
    ActorRuntimeId, BedrockDduiDataStoreUpdate, BedrockDduiDataStoreUpdateData, BlockPos,
    BookEditActionAddPage, BookEditActionDeletePage, BookEditActionFinalize,
    BookEditActionReplacePage, BookEditActionSwapPages, BookEditPacketOperation,
    CerealizerNetworkItemStackDescriptorSerializedData, EnumsAnimatePacketPayloadAction,
    EnumsHandSlot, EnumsInteractPacketPayloadAction, EnumsItemUseInventoryTransactionActionType,
    EnumsItemUseInventoryTransactionPredictedResult, EnumsItemUseInventoryTransactionTriggerType,
    EnumsRewindType, InventoryTransactionPacketTransaction, ItemUseInventoryTransaction,
    PlayerPartyInfo, Vec3,
};
use jolyne::valentine::{
    ActorPickRequestPacket, AnimatePacket, BlockPickRequestPacket, BookEditPacket,
    ClientCacheBlobStatusPacket, EmoteListPacket, EmotePacket, InteractPacket,
    InventoryTransactionPacket, McpePacket, McpePacketData, McpePacketName, MobEquipmentPacket,
    PartyChangedPacket, PartyDestinationCookieResponsePacket,
    ResourcePacksReadyForValidationPacket, ServerboundDataDrivenScreenClosedPacket,
    ServerboundDataStorePacket,
};

/// One line of a command script.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// A slash command, without its leading `/`.
    Command(String),
    /// A `!` line.
    Action(Action),
}

impl Step {
    /// The script line as written (`/command` or `!action ...`).
    pub fn label(&self) -> String {
        match self {
            Step::Command(command) => format!("/{command}"),
            Step::Action(action) => action.line.clone(),
        }
    }
}

/// A parsed `!` line.
#[derive(Debug, Clone, PartialEq)]
pub struct Action {
    /// The line as written.
    pub line: String,
    pub kind: ActionKind,
}

/// Who an action is aimed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Me,
    Peer,
    Actor,
    Runtime(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ActionKind {
    Wait(u64),
    Interact {
        action: EnumsInteractPacketPayloadAction,
        target: Target,
        position: Option<Vec3>,
    },
    PickActor {
        target: Target,
        max_slots: u8,
        with_data: bool,
    },
    PickBlock {
        position: BlockPos,
        max_slots: u8,
        with_data: bool,
    },
    Equip(u8),
    UseItem(u8),
    Emote {
        id: String,
        flags: u8,
        ticks: u32,
    },
    EmoteList(Vec<uuid::Uuid>),
    Animate {
        action: EnumsAnimatePacketPayloadAction,
        data: f32,
    },
    Book {
        slot: i32,
        operation: BookEditPacketOperation,
    },
    BlobStatus {
        missing: Vec<u64>,
        found: Vec<u64>,
    },
    PartyChanged(Option<PlayerPartyInfo>),
    PacksReady,
    DduiClosed {
        form_id: u32,
        reason: String,
    },
    DataStore(BedrockDduiDataStoreUpdate),
    Cookie {
        cookie: String,
        accepted: bool,
    },
}

/// What actions resolve targets and items against: actors the session saw,
/// the player's inventory as the server last sent it and its position.
#[derive(Debug, Clone, Default)]
pub struct Actors {
    /// The player's own runtime and unique ids.
    pub me: (u64, i64),
    /// The last other player an `AddPlayer` announced.
    pub peer: Option<(u64, i64)>,
    /// The last actor an `AddActor` announced.
    pub actor: Option<(u64, i64)>,
    /// The player's inventory (container 0), as the last `InventoryContent`
    /// and `InventorySlot` set it.
    pub inventory: Vec<CerealizerNetworkItemStackDescriptorSerializedData>,
    /// The player's position (eye height) as the server last set it.
    pub position: Vec3,
}

impl Actors {
    fn resolve(&self, target: Target) -> Result<(u64, i64)> {
        match target {
            Target::Me => Ok(self.me),
            Target::Peer => self.peer.context("no other player has been announced"),
            Target::Actor => self.actor.context("no actor has been announced"),
            // Players' unique ids equal their runtime ids in every BDS capture.
            Target::Runtime(id) => Ok((id, id as i64)),
        }
    }

    /// The item the server last put in inventory slot `slot` (air if none).
    fn item(&self, slot: u8) -> CerealizerNetworkItemStackDescriptorSerializedData {
        self.inventory
            .get(usize::from(slot))
            .cloned()
            .unwrap_or_default()
    }

    /// Follows a server packet that changes the player's inventory or
    /// position.
    pub fn follow(&mut self, packet: &McpePacketData) {
        match packet {
            McpePacketData::InventoryContentPacket(content) if content.container_id == 0 => {
                self.inventory = content.slots.clone();
            }
            McpePacketData::InventorySlotPacket(update) if update.container_id == 0 => {
                let slot = update.slot as usize;
                if self.inventory.len() <= slot {
                    self.inventory.resize(slot + 1, Default::default());
                }
                self.inventory[slot] = update.item.clone();
            }
            McpePacketData::MovePlayerPacket(moved)
                if moved.player_runtime_id.actor_runtime_id == self.me.0 =>
            {
                self.position = moved.position.clone();
            }
            McpePacketData::CorrectPlayerMovePredictionPacket(corrected)
                if corrected.prediction_type == EnumsRewindType::Player =>
            {
                self.position = corrected.pos.clone();
            }
            _ => {}
        }
    }
}

/// Parses one script line: `!` lines are actions, anything else a command.
pub fn parse_line(line: &str) -> Result<Step> {
    let Some(rest) = line.strip_prefix('!') else {
        return Ok(Step::Command(
            line.strip_prefix('/').unwrap_or(line).to_owned(),
        ));
    };
    let words: Vec<&str> = rest.split_whitespace().collect();
    let (&verb, args) = words.split_first().context("empty action")?;
    let kind = parse_action(verb, args).with_context(|| format!("action `{line}`"))?;
    Ok(Step::Action(Action {
        line: line.to_owned(),
        kind,
    }))
}

fn arg<'a>(args: &[&'a str], index: usize, what: &str) -> Result<&'a str> {
    args.get(index)
        .copied()
        .with_context(|| format!("missing {what}"))
}

fn number<T: std::str::FromStr>(text: &str, what: &str) -> Result<T>
where
    T::Err: std::error::Error + Send + Sync + 'static,
{
    text.parse()
        .with_context(|| format!("invalid {what} `{text}`"))
}

fn target(text: &str) -> Result<Target> {
    Ok(match text {
        "self" => Target::Me,
        "peer" => Target::Peer,
        "actor" => Target::Actor,
        id => Target::Runtime(number(id, "runtime id")?),
    })
}

fn hotbar_slot(text: &str) -> Result<u8> {
    let slot: u8 = number(text, "hotbar slot")?;
    ensure!(slot < 9, "hotbar slot {slot} is not 0..8");
    Ok(slot)
}

fn ids(text: &str) -> Result<Vec<u64>> {
    text.split(',')
        .filter(|id| !id.is_empty())
        .map(|id| number(id, "blob id"))
        .collect()
}

fn parse_action(verb: &str, args: &[&str]) -> Result<ActionKind> {
    Ok(match verb {
        "wait" => ActionKind::Wait(number(arg(args, 0, "milliseconds")?, "milliseconds")?),
        "interact" => {
            let action = match arg(args, 0, "interact action")? {
                "invalid" => EnumsInteractPacketPayloadAction::Invalid,
                "stop-riding" => EnumsInteractPacketPayloadAction::Stopriding,
                "mouseover" => EnumsInteractPacketPayloadAction::Interactupdate,
                "npc-open" => EnumsInteractPacketPayloadAction::Npcopen,
                "open-inventory" => EnumsInteractPacketPayloadAction::Openinventory,
                other => EnumsInteractPacketPayloadAction::Unknown(number(other, "action")?),
            };
            let position = match args.len() {
                2 => None,
                5 => Some(Vec3 {
                    x: number(args[2], "x")?,
                    y: number(args[3], "y")?,
                    z: number(args[4], "z")?,
                }),
                _ => bail!("expected ACTION TARGET [X Y Z]"),
            };
            ActionKind::Interact {
                action,
                target: target(arg(args, 1, "target")?)?,
                position,
            }
        }
        "pick-actor" => ActionKind::PickActor {
            target: target(arg(args, 0, "target")?)?,
            max_slots: args
                .get(1)
                .map(|slots| number(slots, "max slots"))
                .transpose()?
                .unwrap_or(9),
            with_data: args.get(2) == Some(&"data"),
        },
        "pick-block" => ActionKind::PickBlock {
            position: BlockPos {
                x: number(arg(args, 0, "x")?, "x")?,
                y: number(arg(args, 1, "y")?, "y")?,
                z: number(arg(args, 2, "z")?, "z")?,
            },
            max_slots: args
                .get(3)
                .map(|slots| number(slots, "max slots"))
                .transpose()?
                .unwrap_or(9),
            with_data: args.get(4) == Some(&"data"),
        },
        "equip" => ActionKind::Equip(hotbar_slot(arg(args, 0, "slot")?)?),
        "use-item" => ActionKind::UseItem(hotbar_slot(arg(args, 0, "slot")?)?),
        "emote" => ActionKind::Emote {
            id: arg(args, 0, "emote id")?.to_owned(),
            flags: args
                .get(1)
                .map(|flags| number(flags, "flags"))
                .transpose()?
                .unwrap_or(0),
            ticks: args
                .get(2)
                .map(|ticks| number(ticks, "ticks"))
                .transpose()?
                .unwrap_or(0),
        },
        "emote-list" => ActionKind::EmoteList(
            args.iter()
                .map(|id| number(id, "emote uuid"))
                .collect::<Result<_>>()?,
        ),
        "animate" => ActionKind::Animate {
            action: match arg(args, 0, "animate action")? {
                "swing" => EnumsAnimatePacketPayloadAction::Swing,
                "wake-up" => EnumsAnimatePacketPayloadAction::Wakeup,
                "critical-hit" => EnumsAnimatePacketPayloadAction::Criticalhit,
                "magic-critical-hit" => EnumsAnimatePacketPayloadAction::Magiccriticalhit,
                other => EnumsAnimatePacketPayloadAction::Unknown(number(other, "action")?),
            },
            data: args
                .get(1)
                .map(|data| number(data, "data"))
                .transpose()?
                .unwrap_or(0.0),
        },
        "book" => {
            let slot = number(arg(args, 0, "slot")?, "slot")?;
            let index = |i: usize| -> Result<i32> { number(arg(args, i, "page")?, "page") };
            let text = || {
                args.get(3..)
                    .map(|words| words.join(" "))
                    .unwrap_or_default()
            };
            let operation = match arg(args, 1, "book operation")? {
                "replace" => BookEditPacketOperation::ReplacePage(BookEditActionReplacePage {
                    page_index: index(2)?,
                    page_text: text().into(),
                    photo_name: "".into(),
                }),
                "add" => BookEditPacketOperation::AddPage(BookEditActionAddPage {
                    page_index: index(2)?,
                    page_text: text().into(),
                    photo_name: "".into(),
                }),
                "delete" => BookEditPacketOperation::DeletePage(BookEditActionDeletePage {
                    page_index: index(2)?,
                }),
                "swap" => BookEditPacketOperation::SwapPages(BookEditActionSwapPages {
                    page_index: index(2)?,
                    swap_with_index: index(3)?,
                }),
                "sign" => BookEditPacketOperation::Finalize(BookEditActionFinalize {
                    title: arg(args, 2, "title")?.into(),
                    author: arg(args, 3, "author")?.into(),
                    xuid: "".into(),
                }),
                other => bail!("unknown book operation `{other}`"),
            };
            ActionKind::Book { slot, operation }
        }
        "blob-status" => {
            let (mut missing, mut found) = (Vec::new(), Vec::new());
            for arg in args {
                match arg.split_once('=') {
                    Some(("missing", list)) => missing = ids(list)?,
                    Some(("found", list)) => found = ids(list)?,
                    _ => bail!("expected missing=ID,.. or found=ID,.."),
                }
            }
            ActionKind::BlobStatus { missing, found }
        }
        "party-changed" => ActionKind::PartyChanged(match args {
            [] => None,
            [id, leader] => Some(PlayerPartyInfo {
                party_id: (*id).into(),
                is_party_leader: number(leader, "leader flag")?,
            }),
            _ => bail!("expected [PARTY_ID LEADER]"),
        }),
        "packs-ready" => ActionKind::PacksReady,
        "ddui-closed" => ActionKind::DduiClosed {
            form_id: number(arg(args, 0, "form id")?, "form id")?,
            reason: args.get(1..).map(|w| w.join(" ")).unwrap_or_default(),
        },
        "data-store" => {
            ensure!(args.len() == 4, "expected NAME PROPERTY PATH VALUE");
            let data = match args[3] {
                "true" => BedrockDduiDataStoreUpdateData::Bool(true),
                "false" => BedrockDduiDataStoreUpdateData::Bool(false),
                value => match value.parse::<f64>() {
                    Ok(number) => BedrockDduiDataStoreUpdateData::Double(number),
                    Err(_) => BedrockDduiDataStoreUpdateData::String(value.into()),
                },
            };
            ActionKind::DataStore(BedrockDduiDataStoreUpdate {
                data_store_name: args[0].into(),
                property: args[1].into(),
                path: args[2].into(),
                data,
                property_update_count: 1,
                path_update_count: 1,
            })
        }
        "cookie" => ActionKind::Cookie {
            cookie: arg(args, 0, "cookie")?.to_owned(),
            accepted: number(arg(args, 1, "accepted flag")?, "accepted flag")?,
        },
        other => bail!("unknown action `{other}`"),
    })
}

impl ActionKind {
    /// The packets this action sends, in order.
    pub fn packets(&self, actors: &Actors) -> Result<Vec<McpePacket>> {
        let me = ActorRuntimeId {
            actor_runtime_id: actors.me.0,
        };
        Ok(match self {
            ActionKind::Wait(_) => Vec::new(),
            ActionKind::Interact {
                action,
                target,
                position,
            } => vec![
                InteractPacket {
                    action: *action,
                    target_runtime_id: ActorRuntimeId {
                        actor_runtime_id: actors.resolve(*target)?.0,
                    },
                    position: position.clone(),
                }
                .into(),
            ],
            ActionKind::PickActor {
                target,
                max_slots,
                with_data,
            } => vec![
                ActorPickRequestPacket {
                    actor_id: actors.resolve(*target)?.1,
                    max_slots: *max_slots,
                    with_data: *with_data,
                }
                .into(),
            ],
            ActionKind::PickBlock {
                position,
                max_slots,
                with_data,
            } => vec![
                BlockPickRequestPacket {
                    position: position.clone(),
                    with_data: *with_data,
                    max_slots: *max_slots,
                }
                .into(),
            ],
            ActionKind::Equip(slot) => vec![
                MobEquipmentPacket {
                    target_runtime_id: me.clone(),
                    item: actors.item(*slot),
                    slot: *slot,
                    selected_slot: *slot,
                    container_id: 0,
                }
                .into(),
            ],
            ActionKind::UseItem(slot) => vec![
                InventoryTransactionPacket {
                    transaction: InventoryTransactionPacketTransaction::ItemUseInventoryTransaction(
                        Box::new(ItemUseInventoryTransaction {
                            action_type: EnumsItemUseInventoryTransactionActionType::Use,
                            trigger_type: EnumsItemUseInventoryTransactionTriggerType::Playerinput,
                            slot: i32::from(*slot),
                            hand: EnumsHandSlot::Mainhand,
                            item: actors.item(*slot),
                            from_position: actors.position.clone(),
                            client_interact_prediction:
                                EnumsItemUseInventoryTransactionPredictedResult::Success,
                            ..Default::default()
                        }),
                    ),
                    ..Default::default()
                }
                .into(),
            ],
            ActionKind::Emote { id, flags, ticks } => vec![
                EmotePacket {
                    actor_runtime_id: me,
                    emote_id: id.as_str().into(),
                    emote_length_ticks: *ticks,
                    xuid: "".into(),
                    platform_id: "".into(),
                    flags: *flags,
                }
                .into(),
            ],
            ActionKind::EmoteList(ids) => vec![
                EmoteListPacket {
                    runtimeid: me,
                    emotepieceids: ids.clone(),
                }
                .into(),
            ],
            ActionKind::Animate { action, data } => vec![
                AnimatePacket {
                    action: *action,
                    target_actor_runtime_id: me,
                    data: *data,
                    swing_source: None,
                }
                .into(),
            ],
            ActionKind::Book { slot, operation } => vec![
                BookEditPacket {
                    book_slot: *slot,
                    operation: operation.clone(),
                }
                .into(),
            ],
            ActionKind::BlobStatus { missing, found } => vec![
                ClientCacheBlobStatusPacket {
                    missing_ids: missing.clone(),
                    found_ids: found.clone(),
                }
                .into(),
            ],
            ActionKind::PartyChanged(info) => vec![
                PartyChangedPacket {
                    party_info: info.clone(),
                }
                .into(),
            ],
            ActionKind::PacksReady => vec![ResourcePacksReadyForValidationPacket {}.into()],
            ActionKind::DduiClosed { form_id, reason } => vec![
                ServerboundDataDrivenScreenClosedPacket {
                    form_id: *form_id,
                    close_reason: reason.as_str().into(),
                }
                .into(),
            ],
            ActionKind::DataStore(update) => vec![
                ServerboundDataStorePacket {
                    update: update.clone(),
                }
                .into(),
            ],
            ActionKind::Cookie { cookie, accepted } => vec![
                PartyDestinationCookieResponsePacket {
                    cookie: cookie.as_str().into(),
                    accepted: *accepted,
                }
                .into(),
            ],
        })
    }
}

/// Names of the packets an action sends, for attributing recorded packets.
pub fn sent_names(packets: &[McpePacket]) -> Vec<McpePacketName> {
    packets
        .iter()
        .map(|packet| packet.data.packet_id())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_parse_into_commands_and_actions() {
        assert_eq!(parse_line("/list").unwrap(), Step::Command("list".into()));
        assert_eq!(parse_line("list").unwrap(), Step::Command("list".into()));
        let Step::Action(action) = parse_line("!interact mouseover peer 1 2.5 3").unwrap() else {
            panic!("not an action");
        };
        let actors = Actors {
            me: (1, 1),
            peer: Some((7, 7)),
            ..Actors::default()
        };
        let packets = action.kind.packets(&actors).unwrap();
        let McpePacket {
            data: jolyne::valentine::McpePacketData::InteractPacket(interact),
            ..
        } = &packets[0]
        else {
            panic!("not an interact");
        };
        assert_eq!(interact.target_runtime_id.actor_runtime_id, 7);
        assert_eq!(interact.position.as_ref().unwrap().y, 2.5);
        for line in [
            "!wait 500",
            "!pick-actor peer 9 data",
            "!pick-block 0 -60 0",
            "!pick-block 1 2 3 9 data",
            "!equip 0",
            "!use-item 8",
            "!emote 4c8ae710-df2e-47cd-814d-cc7bf21a3d67 1 40",
            "!emote-list 4c8ae710-df2e-47cd-814d-cc7bf21a3d67",
            "!animate swing",
            "!book 0 replace 0 hello there",
            "!book 0 sign Title Author",
            "!blob-status missing=1,2 found=3",
            "!party-changed",
            "!party-changed abc true",
            "!packs-ready",
            "!ddui-closed 3 closed by user",
            "!data-store store prop path 1.5",
            "!cookie abc true",
        ] {
            let Step::Action(action) = parse_line(line).unwrap() else {
                panic!("{line} is not an action");
            };
            action.kind.packets(&actors).unwrap();
        }
        assert!(parse_line("!nope").is_err());
        assert!(parse_line("!interact mouseover").is_err());
        let Step::Action(action) = parse_line("!pick-actor peer").unwrap() else {
            unreachable!()
        };
        assert!(action.kind.packets(&Actors::default()).is_err());
    }
}
