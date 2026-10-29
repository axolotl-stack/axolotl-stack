//! `/op <player>` and `/deop <player>`.
//!
//! Both are `admin` commands: BDS answers an operator typing them with
//! `commands.generic.error.permissions` (measured), so in practice only the
//! console runs them. They grant or revoke [`Operator`] on online players,
//! answer `commands.op.success` / `commands.deop.success` (or `.failed` when
//! there is nothing to change) and resend the player's abilities so its
//! client shows the new permission. The change is kept in the
//! [`OperatorList`] for players who rejoin while the server runs; it is not
//! written back to the config file.

use crate::command::Operator;
use crate::command::prelude::*;
use crate::entity::components::{GameMode, PlayerSession, RuntimeEntityId};
use crate::server::game::join::abilities_packet;
use bevy_ecs::prelude::Resource;

/// Who is an operator: names (ASCII case-insensitive) or XUIDs, initialized
/// from the config's `operators` and changed by `/op` and `/deop`. Joining
/// players are granted [`Operator`] when an entry names them.
#[derive(Resource, Debug, Clone, Default)]
pub struct OperatorList {
    entries: Vec<String>,
}

impl OperatorList {
    pub fn new(entries: Vec<String>) -> Self {
        Self { entries }
    }

    /// Whether an entry names the player.
    pub fn grants(&self, name: &str, xuid: Option<&str>) -> bool {
        self.entries.iter().any(|entry| matches(entry, name, xuid))
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
        let xuid = ctx
            .world
            .get::<PlayerSession>(player)
            .and_then(|s| s.xuid.clone());
        let mut list = ctx.world.get_resource_or_insert_with(OperatorList::default);
        if grant {
            list.grant(&name, xuid.as_deref());
            ctx.world.entity_mut(player).insert(Operator);
        } else {
            list.revoke(&name, xuid.as_deref());
            ctx.world.entity_mut(player).remove::<Operator>();
        }
        resend_abilities(ctx, player, grant);
        // [INFERENCE] The lang's line to the target, sent as BDS sends its
        // other target messages.
        ctx.notify(player, &format!("{key_prefix}.message"), vec![]);
        output.add_success(format!("{key_prefix}.success"), vec![name]);
    }
    if output.is_success() {
        Ok(output)
    } else {
        Err(output)
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
}
