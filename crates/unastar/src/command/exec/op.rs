//! `/op <player>`, `/deop <player>` and `/permission list|reload|set`.
//!
//! All are `admin` commands: BDS answers an operator typing them with
//! `commands.generic.error.permissions` (measured), so in practice only the
//! console runs them. `/op` and `/deop` grant or revoke [`Operator`] on
//! online players, tell the player (`commands.op.message`), resend its
//! abilities to everyone and answer `commands.op.success` /
//! `commands.deop.success` (or `.failed` when there is nothing to change).
//!
//! Persistence copies BDS (measured, and its own documentation): a player
//! signed in with an XUID keeps its level in `permissions.json`
//! (`[{"permission": "operator", "xuid": "…"}]`, read again at start and by
//! `/permission reload`); for a player without one BDS logs `Missing id,
//! won't persist permissions on disk.` and the change lasts until the
//! server stops (the [`OperatorList`] keeps it for rejoins).

use crate::command::Operator;
use crate::command::prelude::*;
use crate::entity::components::{GameMode, Player, PlayerSession, RuntimeEntityId};
use crate::server::game::join::abilities_packet;
use bevy_ecs::prelude::Resource;

/// One `permissions.json` entry, in BDS's field order.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PermissionEntry {
    pub permission: String,
    pub xuid: String,
}

/// What BDS logs when it cannot persist a level (no XUID).
const NOT_PERSISTED: &str = "Missing id, won't persist permissions on disk.";

/// Who is an operator: names (ASCII case-insensitive) or XUIDs, initialized
/// from the config's `operators` and changed by `/op` and `/deop` for the
/// session, and the levels `permissions.json` stores by XUID, which decide
/// for the players they list. Joining players are granted [`Operator`] when
/// the list says so.
#[derive(Resource, Debug, Clone, Default)]
pub struct OperatorList {
    entries: Vec<String>,
    permissions: Vec<PermissionEntry>,
}

impl OperatorList {
    pub fn new(entries: Vec<String>) -> Self {
        Self {
            entries,
            permissions: Vec::new(),
        }
    }

    /// Reads `permissions.json` from the server files (empty when missing
    /// or unreadable).
    pub fn load_permissions(&mut self, files: &super::admin::ServerFiles) {
        self.permissions = files
            .path("permissions.json")
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
    }

    fn save_permissions(&self, files: &super::admin::ServerFiles) {
        if let Some(path) = files.path("permissions.json") {
            super::admin::write_json(&path, &self.permissions);
        }
    }

    /// The level `permissions.json` stores for an XUID.
    fn stored(&self, xuid: Option<&str>) -> Option<&str> {
        let xuid = xuid?;
        self.permissions
            .iter()
            .find(|entry| entry.xuid == xuid)
            .map(|entry| entry.permission.as_str())
    }

    /// Stores a level for an XUID (replacing an earlier one).
    fn store(&mut self, xuid: &str, level: &str) {
        self.permissions.retain(|entry| entry.xuid != xuid);
        self.permissions.push(PermissionEntry {
            permission: level.to_owned(),
            xuid: xuid.to_owned(),
        });
    }

    /// Whether the player is an operator: its stored level when
    /// `permissions.json` lists it, else whether an entry names it.
    pub fn grants(&self, name: &str, xuid: Option<&str>) -> bool {
        match self.stored(xuid) {
            Some(level) => level == "operator",
            None => self.entries.iter().any(|entry| matches(entry, name, xuid)),
        }
    }

    fn grant(&mut self, name: &str, xuid: Option<&str>) {
        if !self.grants(name, xuid) {
            self.entries.push(name.to_owned());
        }
    }

    fn revoke(&mut self, name: &str, xuid: Option<&str>) {
        self.entries.retain(|entry| !matches(entry, name, xuid));
    }
}

fn matches(entry: &str, name: &str, xuid: Option<&str>) -> bool {
    !entry.is_empty() && (entry.eq_ignore_ascii_case(name) || xuid == Some(entry))
}

overloads! {
    pub enum OpArgs for "op" {
        0 => Op { player: Target },
    }
}

overloads! {
    pub enum DeopArgs for "deop" {
        0 => Deop { player: Target },
    }
}

pub fn run_op(ctx: &mut CommandContext<'_>, args: OpArgs) -> CommandResult {
    let OpArgs::Op { player } = args;
    change(ctx, &player, true)
}

pub fn run_deop(ctx: &mut CommandContext<'_>, args: DeopArgs) -> CommandResult {
    let DeopArgs::Deop { player } = args;
    change(ctx, &player, false)
}

fn change(ctx: &mut CommandContext<'_>, target: &Target, grant: bool) -> CommandResult {
    let key_prefix = if grant {
        "commands.op"
    } else {
        "commands.deop"
    };
    let players = ctx.players(Some(target))?;
    let mut output = CommandOutput::default();
    for player in players {
        let name = ctx.name(player);
        let is_operator = ctx.world.get::<Operator>(player).is_some();
        if is_operator == grant {
            output.add_failure(format!("{key_prefix}.failed"), vec![name]);
            continue;
        }
        set_level(ctx, player, grant);
        // Measured: the target is told before the answer.
        ctx.notify(player, &format!("{key_prefix}.message"), vec![]);
        output.add_success(format!("{key_prefix}.success"), vec![name]);
    }
    if output.is_success() {
        Ok(output)
    } else {
        Err(output)
    }
}

/// Makes `player` an operator or a member: the component, the persisted or
/// session level, and its abilities resent to everyone.
fn set_level(ctx: &mut CommandContext<'_>, player: Entity, operator: bool) {
    let name = ctx.name(player);
    let xuid = ctx
        .world
        .get::<PlayerSession>(player)
        .and_then(|s| s.xuid.clone())
        .filter(|xuid| !xuid.is_empty());
    let files = super::admin::server_files(ctx.world);
    let mut list = ctx.world.get_resource_or_insert_with(OperatorList::default);
    match &xuid {
        Some(xuid) => {
            list.store(xuid, if operator { "operator" } else { "member" });
            list.save_permissions(&files);
        }
        None => {
            if operator {
                list.grant(&name, None);
            } else {
                list.revoke(&name, None);
            }
        }
    }
    if xuid.is_none() {
        ctx.console_note(true, NOT_PERSISTED);
    }
    if operator {
        ctx.world.entity_mut(player).insert(Operator);
    } else {
        ctx.world.entity_mut(player).remove::<Operator>();
    }
    resend_abilities(ctx, player, operator);
}

arg_enum! {
    /// Captured `PermissionsAction`.
    pub enum PermissionsAction in "PermissionsAction" {
        Set = "set",
        List = "list",
        Reload = "reload",
    }
}

arg_enum! {
    /// Captured `PermissionLevel`.
    pub enum PermissionLevel in "PermissionLevel" {
        Visitor = "visitor",
        Member = "member",
        Operator = "operator",
    }
}

overloads! {
    pub enum PermissionArgs for "permission" {
        0 => Action { action: PermissionsAction },
        1 => Player {
            action: PermissionsAction,
            player: Target,
            permission: Option<PermissionLevel>,
        },
    }
}

pub fn run_permission(ctx: &mut CommandContext<'_>, args: PermissionArgs) -> CommandResult {
    let (action, target, level) = match args {
        PermissionArgs::Action { action } => (action, None, None),
        PermissionArgs::Player {
            action,
            player,
            permission,
        } => (action, Some(player), permission),
    };
    match action {
        PermissionsAction::List => {
            let list = ctx.world.get_resource_or_insert_with(OperatorList::default);
            let permissions = serde_json::to_value(&list.permissions).unwrap_or_default();
            let mut output = CommandOutput::success(
                super::admin::data_set(serde_json::json!({"command": "ops", "result": []})),
                vec![],
            );
            output.add_success(
                super::admin::data_set(
                    serde_json::json!({"command": "permissions", "result": permissions}),
                ),
                vec![],
            );
            Ok(output)
        }
        PermissionsAction::Reload => {
            let files = super::admin::server_files(ctx.world);
            ctx.world
                .get_resource_or_insert_with(OperatorList::default)
                .load_permissions(&files);
            // Every online player's level again: the file decides for the
            // players it lists, the others keep theirs.
            let players: Vec<(Entity, Option<String>)> = {
                let mut query = ctx
                    .world
                    .query_filtered::<(Entity, &PlayerSession), bevy_ecs::prelude::With<Player>>();
                query
                    .iter(ctx.world)
                    .map(|(entity, session)| (entity, session.xuid.clone()))
                    .collect()
            };
            for (player, xuid) in players {
                let stored = ctx
                    .world
                    .resource::<OperatorList>()
                    .stored(xuid.as_deref())
                    .map(|level| level == "operator");
                let operator =
                    stored.unwrap_or_else(|| ctx.world.get::<Operator>(player).is_some());
                if operator {
                    ctx.world.entity_mut(player).insert(Operator);
                } else {
                    ctx.world.entity_mut(player).remove::<Operator>();
                }
                resend_abilities(ctx, player, operator);
            }
            let mut output = CommandOutput::success("commands.ops.reloaded", vec![]);
            output.add_success("commands.permissions.reloaded", vec![]);
            Ok(output)
        }
        PermissionsAction::Set => {
            let Some(target) = target else {
                return Err(CommandOutput::no_targets());
            };
            let players = ctx.players(Some(&target))?;
            if let Some(level) = level {
                for player in players {
                    set_level(ctx, player, level == PermissionLevel::Operator);
                }
            }
            // Measured: without an XUID only the not-persisted line prints;
            // [INFERENCE] nothing else is printed with one either.
            Ok(CommandOutput::default())
        }
    }
}

/// The player's abilities with its new permission, to every player (as a
/// game mode change resends them).
fn resend_abilities(ctx: &mut CommandContext<'_>, player: Entity, operator: bool) {
    let runtime_id = ctx
        .world
        .get::<RuntimeEntityId>(player)
        .map_or(0, |id| id.0);
    let mode = ctx
        .world
        .get::<GameMode>(player)
        .copied()
        .unwrap_or(GameMode::Survival);
    ctx.broadcast(abilities_packet(runtime_id, mode, operator));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::exec::test_support::Harness;
    use crate::command::{CommandSource, execute};
    use crate::server::PlayerSpawnData;
    use jolyne::valentine::McpePacketData;
    use jolyne::valentine::types::EnumsCommandPermissionLevel;

    fn console(h: &mut Harness, line: &str) -> CommandOutput {
        execute(h.world(), CommandSource::server(), line)
    }

    fn abilities(h: &mut Harness, player: Entity) -> Vec<EnumsCommandPermissionLevel> {
        h.sent(player)
            .into_iter()
            .filter_map(|p| match p {
                McpePacketData::UpdateAbilitiesPacket(p) => Some(p.data.command_permissions),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn operators_cannot_op_like_bds() {
        let mut h = Harness::new();
        let alice = h.player("Alice", true);
        assert_eq!(
            h.run(alice, "/op NoSuchPlayer"),
            CommandOutput::failure("commands.generic.error.permissions", vec!["op".into()])
        );
        assert_eq!(
            h.run(alice, "/deop NoSuchPlayer"),
            CommandOutput::failure("commands.generic.error.permissions", vec!["deop".into()])
        );
    }

    #[test]
    fn console_ops_and_deops_online_players() {
        let mut h = Harness::new();
        let bob = h.player("Bob", false);

        let output = console(&mut h, "/op Bob");
        assert_eq!(
            output,
            CommandOutput::success("commands.op.success", vec!["Bob".into()])
        );
        assert!(h.world().get::<Operator>(bob).is_some());
        let sent = h.sent(bob);
        assert!(sent.iter().any(|p| matches!(p,
            McpePacketData::UpdateAbilitiesPacket(p)
                if p.data.command_permissions == EnumsCommandPermissionLevel::Gamedirectors)));
        assert!(
            sent.iter()
                .any(|p| matches!(p, McpePacketData::TextPacket(t)
            if format!("{t:?}").contains("commands.op.message")))
        );
        // Bob can now run operator commands.
        assert!(h.run(bob, "/xp 1").is_success());

        assert_eq!(
            console(&mut h, "/op Bob"),
            CommandOutput::failure("commands.op.failed", vec!["Bob".into()])
        );

        let output = console(&mut h, "/deop Bob");
        assert_eq!(
            output,
            CommandOutput::success("commands.deop.success", vec!["Bob".into()])
        );
        assert!(h.world().get::<Operator>(bob).is_none());
        assert_eq!(abilities(&mut h, bob), [EnumsCommandPermissionLevel::Any]);
        assert_eq!(
            console(&mut h, "/deop Bob"),
            CommandOutput::failure("commands.deop.failed", vec!["Bob".into()])
        );
        assert_eq!(console(&mut h, "/op Nobody"), CommandOutput::no_targets());
    }

    #[test]
    fn changes_apply_to_players_who_rejoin() {
        let mut h = Harness::new();
        h.player("Carol", false);
        assert!(console(&mut h, "/op Carol").is_success());

        let join = |h: &mut Harness, session_id: u64, name: &str| {
            let (outbound_tx, _rx) = tokio::sync::mpsc::channel(256);
            let entity = h
                .server
                .spawn_player(PlayerSpawnData {
                    session_id,
                    display_name: name.to_string(),
                    xuid: None,
                    uuid: None,
                    runtime_id: session_id as i64,
                    position: DVec3::new(0.5, 17.0, 0.5),
                    outbound_tx,
                    saved_data: None,
                    chunk_radius: 2,
                })
                .expect("a fresh player spawns");
            h.world().get::<Operator>(entity).is_some()
        };
        assert!(
            join(&mut h, 50, "carol"),
            "the op grant outlives the session"
        );
        assert!(!join(&mut h, 51, "Dave"));

        assert!(console(&mut h, "/deop Carol").is_success());
        assert!(!join(&mut h, 52, "Carol"));
    }

    /// A signed-in player's level survives a restart through
    /// `permissions.json`, BDS's format; without an XUID BDS logs that it
    /// cannot persist it.
    #[test]
    fn signed_in_players_keep_their_level_in_permissions_json() {
        let files = tempfile::tempdir().unwrap();
        let server_files = super::super::admin::ServerFiles(Some(files.path().to_owned()));
        let mut h = Harness::new();
        h.world().insert_resource(server_files.clone());
        let join = |h: &mut Harness, session_id: u64, name: &str, xuid: Option<&str>| {
            let (outbound_tx, _rx) = tokio::sync::mpsc::channel(256);
            h.server
                .spawn_player(PlayerSpawnData {
                    session_id,
                    display_name: name.to_string(),
                    xuid: xuid.map(str::to_owned),
                    uuid: None,
                    runtime_id: session_id as i64,
                    position: DVec3::new(0.5, 17.0, 0.5),
                    outbound_tx,
                    saved_data: None,
                    chunk_radius: 2,
                })
                .expect("a fresh player spawns")
        };
        join(&mut h, 60, "Erin", Some("2535400000000001"));
        assert!(console(&mut h, "/op Erin").is_success());
        assert!(
            h.world()
                .get_resource::<crate::command::ConsoleNotes>()
                .is_none_or(|notes| notes.0.is_empty()),
            "an XUID is persisted without the warning"
        );
        let text = std::fs::read_to_string(files.path().join("permissions.json")).unwrap();
        let stored: Vec<PermissionEntry> = serde_json::from_str(&text).unwrap();
        assert_eq!(
            stored,
            [PermissionEntry {
                permission: "operator".into(),
                xuid: "2535400000000001".into()
            }]
        );
        assert_eq!(
            console(&mut h, "/permission list").to_text().join("\n"),
            "###* {\"command\":\"ops\",\"result\":[]}\n *###\n###* {\"command\":\"permissions\",\"result\":[{\"permission\":\"operator\",\"xuid\":\"2535400000000001\"}]}\n *###"
        );

        // A restarted server reads the file back.
        let mut restarted = Harness::new();
        restarted.world().insert_resource(server_files.clone());
        restarted
            .world()
            .resource_mut::<OperatorList>()
            .load_permissions(&server_files);
        let erin = join(&mut restarted, 61, "Erin", Some("2535400000000001"));
        assert!(restarted.world().get::<Operator>(erin).is_some());
        assert!(console(&mut restarted, "/deop Erin").is_success());
        let text = std::fs::read_to_string(files.path().join("permissions.json")).unwrap();
        assert!(text.contains("\"member\""), "{text}");

        // Without an XUID the change is the session's only.
        let frank = join(&mut restarted, 62, "Frank", None);
        assert!(console(&mut restarted, "/op Frank").is_success());
        assert_eq!(
            restarted
                .world()
                .resource::<crate::command::ConsoleNotes>()
                .0,
            [crate::command::ConsoleNote {
                error: true,
                text: NOT_PERSISTED.into()
            }]
        );
        assert!(restarted.world().get::<Operator>(frank).is_some());
    }
}
