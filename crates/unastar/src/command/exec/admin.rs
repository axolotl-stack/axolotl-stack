//! Server administration commands, run from the server console (owner,
//! host and admin levels: BDS answers an operator with
//! `commands.generic.error.permissions`, measured):
//! `/allowlist`, `/permission`, `/changesetting`, `/setmaxplayers`, `/save`,
//! `/serveridentity`, `/transfer` and `/sendshowstoreoffer`.
//!
//! Measured on BDS 1.26.51 (command capture, `console` and `players`
//! segments; BDS prints one console record per answer, its messages one per
//! line, as `ERROR` when the command failed):
//!
//! - `allowlist list` prints the list as a data set, `###* {"command":
//!   "allowlist","result":[…]}` and ` *###` (`null` when empty); `add`,
//!   `remove`, `on` (refused without online authentication), `off` and
//!   `reload` answer the `commands.allowlist.*` lines;
//! - `permission list` prints the `ops` and `permissions` data sets, `reload`
//!   reloads both (two lines) and resends every player's abilities, `set`
//!   changes an online player's level (see [`super::op`] for persistence);
//! - `changesetting` answers `commands.change-setting.success [setting]` and
//!   sends nothing (difficulty numbers above 3 are `num.tooBig`);
//! - `setmaxplayers` bounds the value to the players online and to 40
//!   connections, with an extra line saying which bound applied;
//! - `save hold` / `query` / `resume` answer `Saving...`, the file list,
//!   `Changes to the world are resumed.`; out of order, `A previous save has
//!   not been completed.` or `The command is already running`;
//! - `serveridentity status|save|delete` report on
//!   `keys/server_identity_key.pem` (literal English lines);
//! - `transfer` finds the player by name and sends `Transfer`;
//!   `sendshowstoreoffer` needs a UUID offer id (the `server` form has none,
//!   so it always answers the invalid UUID line) and sends `ShowStoreOffer`.

use crate::command::prelude::*;
use crate::entity::components::{Player, PlayerName, PlayerSession};
use crate::server::game::ServerConfigResource;
use bevy_ecs::prelude::Resource;
use jolyne::valentine::types::EnumsShowStoreOfferRedirectType;
use jolyne::valentine::{ShowStoreOfferPacket, TransferPacket};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Where the server keeps `allowlist.json`, `permissions.json` and
/// `keys/server_identity_key.pem` (BDS: next to the executable; unastar: its
/// working directory). `None` keeps everything in memory (tests).
#[derive(Resource, Debug, Clone, Default)]
pub struct ServerFiles(pub Option<PathBuf>);

impl ServerFiles {
    pub fn path(&self, name: &str) -> Option<PathBuf> {
        self.0.as_ref().map(|dir| dir.join(name))
    }
}

/// The files directory of the world (empty when unset).
pub(super) fn server_files(world: &World) -> ServerFiles {
    world
        .get_resource::<ServerFiles>()
        .cloned()
        .unwrap_or_default()
}

/// One `allowlist.json` entry, in BDS's field order.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowlistEntry {
    #[serde(default)]
    pub ignores_player_limit: bool,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub xuid: Option<String>,
}

/// The allowlist: who may join while it is on (`allow-list`).
#[derive(Resource, Debug, Clone, Default)]
pub struct Allowlist {
    pub enabled: bool,
    pub entries: Vec<AllowlistEntry>,
}

impl Allowlist {
    /// Reads `allowlist.json` (empty when missing or unreadable).
    pub fn load(files: &ServerFiles) -> Vec<AllowlistEntry> {
        files
            .path("allowlist.json")
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn save(&self, files: &ServerFiles) {
        if let Some(path) = files.path("allowlist.json") {
            write_json(&path, &self.entries);
        }
    }

    fn position(&self, name: &str) -> Option<usize> {
        self.entries
            .iter()
            .position(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    /// Whether a joining player may play: always while the list is off.
    pub fn admits(&self, name: &str, xuid: Option<&str>) -> bool {
        !self.enabled
            || self.entries.iter().any(|entry| {
                entry.name.eq_ignore_ascii_case(name)
                    || xuid.is_some_and(|xuid| entry.xuid.as_deref() == Some(xuid))
            })
    }
}

pub(super) fn write_json(path: &Path, value: &impl serde::Serialize) {
    if let Ok(mut text) = serde_json::to_string_pretty(value) {
        text.push('\n');
        if let Err(error) = std::fs::write(path, text) {
            tracing::warn!(path = %path.display(), %error, "could not write server file");
        }
    }
}

/// A data set as BDS prints it on the console.
pub(super) fn data_set(json: serde_json::Value) -> String {
    format!("###* {json}\n *###")
}

arg_enum! {
    /// Captured `AllowListAction`.
    pub enum AllowListAction in "AllowListAction" {
        Add = "add",
        Remove = "remove",
        Off = "off",
        List = "list",
        Reload = "reload",
        On = "on",
    }
}

overloads! {
    pub enum AllowlistArgs for "allowlist" {
        0 => Action { action: AllowListAction, name: Option<Text> },
    }
}

pub fn run_allowlist(ctx: &mut CommandContext<'_>, args: AllowlistArgs) -> CommandResult {
    let AllowlistArgs::Action { action, name } = args;
    let name = name.map(|n| n.0).unwrap_or_default();
    let files = server_files(ctx.world);
    let online_mode = ctx
        .world
        .get_resource::<ServerConfigResource>()
        .is_some_and(|config| config.0.online_mode);
    let mut list = ctx.world.get_resource_or_insert_with(Allowlist::default);
    match action {
        AllowListAction::List => {
            let result = if list.entries.is_empty() {
                serde_json::Value::Null
            } else {
                serde_json::to_value(&list.entries).unwrap_or_default()
            };
            success!(data_set(
                serde_json::json!({"command": "allowlist", "result": result})
            ))
        }
        AllowListAction::Add => {
            if name.is_empty() || list.position(&name).is_some() {
                return failure!("commands.allowlist.add.failed", name);
            }
            list.entries.push(AllowlistEntry {
                ignores_player_limit: false,
                name: name.clone(),
                xuid: None,
            });
            list.save(&files);
            success!("commands.allowlist.add.success", name)
        }
        AllowListAction::Remove => {
            let Some(index) = list.position(&name) else {
                return failure!("commands.allowlist.remove.failed", name);
            };
            list.entries.remove(index);
            list.save(&files);
            success!("commands.allowlist.remove.success", name)
        }
        AllowListAction::On => {
            if !online_mode {
                return failure!("commands.allowlist.noOnlineAuth");
            }
            list.enabled = true;
            success!("commands.allowlist.enabled")
        }
        AllowListAction::Off => {
            list.enabled = false;
            success!("commands.allowlist.disabled")
        }
        AllowListAction::Reload => {
            list.entries = Allowlist::load(&files);
            success!("commands.allowlist.reloaded")
        }
    }
}

arg_enum! {
    /// Captured `BoolSettingName`.
    pub enum BoolSetting in "BoolSettingName" {
        AllowCheats = "allow-cheats",
    }
}

overloads! {
    pub enum ChangeSettingArgs for "changesetting" {
        0 => Bool { setting: BoolSetting, value: bool },
        1 => Difficulty { setting: EnumValue, value: EnumValue },
        2 => DifficultyNumber { setting: EnumValue, value: i32 },
    }
}

/// The server settings `/changesetting` changes: neither server.properties
/// nor (as measured: no packet follows) the running world.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ServerSettings {
    pub allow_cheats: Option<bool>,
    pub difficulty: Option<i32>,
}

pub fn run_changesetting(ctx: &mut CommandContext<'_>, args: ChangeSettingArgs) -> CommandResult {
    let mut settings = *ctx
        .world
        .get_resource_or_insert_with(ServerSettings::default);
    let name = match args {
        ChangeSettingArgs::Bool { value, .. } => {
            settings.allow_cheats = Some(value);
            "allow-cheats"
        }
        ChangeSettingArgs::Difficulty { value, .. } => {
            settings.difficulty = Some(match &*value {
                "peaceful" | "p" => 0,
                "easy" | "e" => 1,
                "normal" | "n" => 2,
                _ => 3,
            });
            "difficulty"
        }
        ChangeSettingArgs::DifficultyNumber { value, .. } => {
            if value < 0 {
                return failure!("commands.generic.num.tooSmall", value, 0);
            }
            if value > 3 {
                return failure!("commands.generic.num.tooBig", value, 3);
            }
            settings.difficulty = Some(value);
            "difficulty"
        }
    };
    ctx.world.insert_resource(settings);
    success!("commands.change-setting.success", name)
}

/// BDS's bound on `/setmaxplayers` ("maximum allowed connections").
const MAX_CONNECTIONS: i32 = 40;

/// The connection gate the network accepts players through, resized by
/// `/setmaxplayers`.
#[derive(Resource, Clone)]
pub struct PlayerLimit(pub Arc<tokio::sync::Semaphore>);

overloads! {
    pub enum SetMaxPlayersArgs for "setmaxplayers" {
        0 => Set { max_players: i32 = "maxPlayers" },
    }
}

pub fn run_setmaxplayers(ctx: &mut CommandContext<'_>, args: SetMaxPlayersArgs) -> CommandResult {
    let SetMaxPlayersArgs::Set { max_players } = args;
    let online = {
        let mut players = ctx
            .world
            .query_filtered::<(), bevy_ecs::prelude::With<Player>>();
        players.iter(ctx.world).count() as i32
    };
    let (value, bound) = if max_players < online {
        (online, Some("commands.setmaxplayers.success.lowerbound"))
    } else if max_players > MAX_CONNECTIONS {
        (
            MAX_CONNECTIONS,
            Some("commands.setmaxplayers.success.upperbound"),
        )
    } else {
        (max_players, None)
    };
    let previous = ctx
        .world
        .get_resource::<ServerConfigResource>()
        .map_or(0, |config| config.0.max_players);
    if let Some(mut config) = ctx.world.get_resource_mut::<ServerConfigResource>() {
        config.0.max_players = value as u32;
    }
    if let Some(limit) = ctx.world.get_resource::<PlayerLimit>() {
        let (previous, value) = (previous as usize, value as usize);
        if value > previous {
            limit.0.add_permits(value - previous);
        } else {
            // Never below the players online, so these permits are free.
            limit.0.forget_permits(previous - value);
        }
    }
    let mut output =
        CommandOutput::success("commands.setmaxplayers.success", vec![value.to_string()]);
    if let Some(bound) = bound {
        output.add_success(bound, vec![]);
    }
    Ok(output)
}

/// Where `/save hold` left the world.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SaveState {
    #[default]
    Running,
    /// Held for a backup: the files are ready to copy.
    Held,
}

/// The world's files, for `/save query` (unastar: `worlds/<name>/`).
#[derive(Resource, Debug, Clone)]
pub struct WorldFiles {
    /// The directory BDS's list is relative to (`worlds/`).
    pub root: PathBuf,
    /// The world's own directory under it.
    pub world: PathBuf,
}

arg_enum! {
    /// Captured `SaveMode`.
    pub enum SaveMode in "SaveMode" {
        Query = "query",
        Hold = "hold",
        Resume = "resume",
    }
}

overloads! {
    pub enum SaveArgs for "save" {
        0 => Mode { mode: SaveMode },
    }
}

pub fn run_save(ctx: &mut CommandContext<'_>, args: SaveArgs) -> CommandResult {
    let SaveArgs::Mode { mode } = args;
    let state = *ctx.world.get_resource_or_insert_with(SaveState::default);
    match (mode, state) {
        (SaveMode::Hold, SaveState::Running) => {
            ctx.world.insert_resource(SaveState::Held);
            success!("commands.save.start")
        }
        (SaveMode::Hold, SaveState::Held) => failure!("commands.generic.running"),
        (SaveMode::Query, SaveState::Held) => {
            let files = ctx
                .world
                .get_resource::<WorldFiles>()
                .map(file_list)
                .unwrap_or_default();
            let mut output = CommandOutput::success("commands.save-all.success", vec![]);
            output.add_success(files, vec![]);
            Ok(output)
        }
        (SaveMode::Resume, SaveState::Held) => {
            ctx.world.insert_resource(SaveState::Running);
            success!("commands.save-on.success")
        }
        (SaveMode::Query | SaveMode::Resume, SaveState::Running) => {
            failure!("commands.save-on.notDone")
        }
    }
}

/// `level/db/000003.log:156, level/db/CURRENT:16, …`: every file of the
/// world with its length, sorted, relative to the worlds directory.
fn file_list(files: &WorldFiles) -> String {
    let mut found = Vec::new();
    let mut pending = vec![files.world.clone()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.metadata() {
                Ok(metadata) if metadata.is_dir() => pending.push(path),
                Ok(metadata) => {
                    let relative = path.strip_prefix(&files.root).unwrap_or(&path);
                    let relative = relative.to_string_lossy().replace('\\', "/");
                    found.push(format!("{relative}:{}", metadata.len()));
                }
                Err(_) => {}
            }
        }
    }
    found.sort();
    found.join(", ")
}

/// The key the server identifies itself with (BDS: loaded from
/// `keys/server_identity_key.pem` at start, else generated).
#[derive(Resource, Clone)]
pub struct ServerIdentity(pub p384::SecretKey);

const IDENTITY_FILE: &str = "keys/server_identity_key.pem";

/// The saved identity key, if the server files hold a readable one.
pub fn load_identity(files: &ServerFiles) -> Option<p384::SecretKey> {
    use p384::pkcs8::DecodePrivateKey;
    let text = std::fs::read_to_string(files.path(IDENTITY_FILE)?).ok()?;
    p384::SecretKey::from_sec1_pem(&text)
        .ok()
        .or_else(|| p384::SecretKey::from_pkcs8_pem(&text).ok())
}

arg_enum! {
    /// Captured `ServerIdentityAction`.
    pub enum ServerIdentityAction in "ServerIdentityAction" {
        Save = "save",
        Delete = "delete",
        Status = "status",
    }
}

overloads! {
    pub enum ServerIdentityArgs for "serveridentity" {
        0 => Action { action: ServerIdentityAction },
    }
}

pub fn run_serveridentity(ctx: &mut CommandContext<'_>, args: ServerIdentityArgs) -> CommandResult {
    let ServerIdentityArgs::Action { action } = args;
    let path = server_files(ctx.world).path(IDENTITY_FILE);
    let exists = path.as_ref().is_some_and(|path| path.is_file());
    let missing = format!("No saved server identity key exists at {IDENTITY_FILE}.");
    match action {
        ServerIdentityAction::Status if exists => {
            success!(format!(
                "A saved server identity key exists at {IDENTITY_FILE}."
            ))
        }
        ServerIdentityAction::Status => success!(missing),
        ServerIdentityAction::Save => {
            let key = ctx
                .world
                .get_resource_or_insert_with(|| {
                    ServerIdentity(p384::SecretKey::random(&mut rand::thread_rng()))
                })
                .0
                .clone();
            let written = path.is_some_and(|path| {
                let pem = key
                    .to_sec1_pem(p384::pkcs8::LineEnding::LF)
                    .map(|pem| pem.to_string());
                path.parent()
                    .is_some_and(|dir| std::fs::create_dir_all(dir).is_ok())
                    && pem.is_ok_and(|pem| std::fs::write(&path, pem).is_ok())
            });
            if !written {
                return failure!(format!(
                    "Could not save server identity key to {IDENTITY_FILE}."
                ));
            }
            success!(format!("Saved server identity key to {IDENTITY_FILE}."))
        }
        ServerIdentityAction::Delete if exists => {
            let _ = path.map(std::fs::remove_file);
            let mut output = CommandOutput::success(
                format!("Deleted server identity key at {IDENTITY_FILE}."),
                vec![],
            );
            output.add_success(
                "The currently running server identity key will not change until the next game start. Restart the server to use a new identity key.",
                vec![],
            );
            Ok(output)
        }
        ServerIdentityAction::Delete => failure!(missing),
    }
}

overloads! {
    pub enum TransferArgs for "transfer" {
        0 => Transfer { player: Text = "pfidOrMSA", server: Text, port: Option<i32> },
    }
}

/// The port `/transfer` uses when none is given.
const DEFAULT_PORT: i32 = 19132;

pub fn run_transfer(ctx: &mut CommandContext<'_>, args: TransferArgs) -> CommandResult {
    let TransferArgs::Transfer {
        player,
        server,
        port,
    } = args;
    // Measured: the player is found by name; [INFERENCE] or by XUID.
    let found = {
        let mut players = ctx
            .world
            .query_filtered::<(Entity, &PlayerName, &PlayerSession), bevy_ecs::prelude::With<Player>>();
        players
            .iter(ctx.world)
            .find(|(_, name, session)| {
                name.0.eq_ignore_ascii_case(&player) || session.xuid.as_deref() == Some(&*player)
            })
            .map(|(entity, ..)| entity)
    };
    let Some(target) = found else {
        return failure!("commands.generic.player.notFound");
    };
    let port = port.unwrap_or(DEFAULT_PORT);
    let Ok(port) = u16::try_from(port) else {
        return failure!("commands.transferserver.invalid.port");
    };
    ctx.send(
        target,
        TransferPacket {
            server_address: server.0.as_str().into(),
            server_port: port,
            reload_world: false,
            gatherings_configuration: None,
        },
    );
    success!("commands.transferserver.successful")
}

arg_enum! {
    /// Captured `RedirectLocation`.
    pub enum RedirectLocation in "RedirectLocation" {
        Marketplace = "marketplace",
        Character = "character",
    }
}

overloads! {
    pub enum SendShowStoreOfferArgs for "sendshowstoreoffer" {
        0 => Offer {
            player: Target,
            redirect: RedirectLocation = "redirectType",
            offer: Text = "offerId",
        },
        1 => ServerList { player: Target, redirect: EnumValue = "redirectType" },
    }
}

pub fn run_sendshowstoreoffer(
    ctx: &mut CommandContext<'_>,
    args: SendShowStoreOfferArgs,
) -> CommandResult {
    let (target, redirect, offer) = match args {
        SendShowStoreOfferArgs::Offer {
            player,
            redirect,
            offer,
        } => (player, Some(redirect), Some(offer)),
        SendShowStoreOfferArgs::ServerList { player, .. } => (player, None, None),
    };
    let players = ctx.players(Some(&target))?;
    // Measured: the server list form has no offer id and always fails here.
    let Some(offer_id) = offer.and_then(|offer| uuid::Uuid::parse_str(&offer).ok()) else {
        return failure!("Command had invalid UUID for the offerId parameter");
    };
    let redirect_type = match redirect {
        Some(RedirectLocation::Character) => EnumsShowStoreOfferRedirectType::Dressingroomoffer,
        _ => EnumsShowStoreOfferRedirectType::Marketplaceoffer,
    };
    for &player in &players {
        ctx.send(
            player,
            ShowStoreOfferPacket {
                offer_id,
                redirect_type,
            },
        );
    }
    success!("Request packet sent to target player")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::args::assert_covers;

    #[test]
    fn the_allowlist_file_and_switch_decide_who_joins() {
        use crate::command::exec::test_support::Harness;
        use crate::command::{CommandSource, execute};
        let files = tempfile::tempdir().unwrap();
        let mut h = Harness::new();
        h.world()
            .insert_resource(ServerFiles(Some(files.path().to_owned())));
        h.world()
            .resource_mut::<ServerConfigResource>()
            .0
            .online_mode = true;
        let console =
            |h: &mut Harness, line: &str| execute(h.world(), CommandSource::server(), line);
        assert!(console(&mut h, "allowlist add Alice").is_success());
        assert!(console(&mut h, "allowlist on").is_success());
        let text = std::fs::read_to_string(files.path().join("allowlist.json")).unwrap();
        let entries: Vec<AllowlistEntry> = serde_json::from_str(&text).unwrap();
        assert_eq!(entries[0].name, "Alice");
        let list = h.world().resource::<Allowlist>().clone();
        assert!(list.admits("alice", None));
        assert!(!list.admits("Mallory", None));
        // Edited by hand, then reloaded.
        std::fs::write(
            files.path().join("allowlist.json"),
            r#"[{"name":"Mallory","xuid":"42","ignoresPlayerLimit":false}]"#,
        )
        .unwrap();
        assert!(console(&mut h, "allowlist reload").is_success());
        let list = h.world().resource::<Allowlist>().clone();
        assert!(
            list.admits("Someone", Some("42")),
            "an XUID entry admits by XUID"
        );
        assert!(!list.admits("Alice", None));
        assert!(console(&mut h, "allowlist off").is_success());
        assert!(h.world().resource::<Allowlist>().admits("Anyone", None));
    }

    #[test]
    fn enums_cover_the_captured_values() {
        assert_covers::<AllowListAction>();
        assert_covers::<BoolSetting>();
        assert_covers::<SaveMode>();
        assert_covers::<ServerIdentityAction>();
        assert_covers::<RedirectLocation>();
    }
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::{ConsoleRecord, Replay};

    #[test]
    fn console_commands_match_the_bds_capture() {
        let differ = [
            // unastar's world has other files than BDS's (the line format is
            // compared below).
            "!console save query",
        ];
        let steps = Replay::new().assert_segment("console", &differ);
        let query = steps
            .iter()
            .find(|s| s.captured.command == differ[0])
            .expect("save query");
        let [ConsoleRecord { level, text }] = query.console.as_slice() else {
            panic!("one record: {:?}", query.console);
        };
        assert_eq!(level, "INFO");
        // unastar's harness has no world directory, so its list is empty.
        assert_eq!(text, "Data saved. Files are now ready to be copied.\n");
        assert!(query.captured.console[0].text.starts_with(
            "Data saved. Files are now ready to be copied.\nbds-extractor-1.26.51/db/"
        ));
    }
}
