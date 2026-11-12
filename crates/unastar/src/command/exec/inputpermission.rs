//! `/inputpermission set|query <targets> <permission> [state]`, on each
//! player's [`InputLocks`] (the selector argument `haspermission=` reads
//! them).
//!
//! Measured on BDS 1.26.51: `set` sends `UpdateClientInputLocks` with every
//! input the player now has off, and both forms answer with English text
//! BDS formats itself from `en_US.lang` instead of a translation key:
//! `set` with `commands.inputpermission.set.outputoneplayer` ("Camera input
//! disabled for BDSExtractor  ", parameter: the player count), `query` with
//! `commands.inputpermission.queryverbose` ("<Camera>: 0 enabled | 1
//! disabled") or `…query` when a state is given ("<Camera>: 1 disabled"),
//! with the names of the players found as the parameter.

use crate::command::prelude::*;
use crate::entity::components::{InputLocks, InputPermission};
use jolyne::valentine::UpdateClientInputLocksPacket;
use unastar_data::lang::en_us;

arg_enum! {
    /// Captured `permission`.
    pub enum PermissionName in "permission" {
        Camera = "camera",
        Movement = "movement",
        Jump = "jump",
        LateralMovement = "lateral_movement",
        Sneak = "sneak",
        Dismount = "dismount",
        Mount = "mount",
        MoveBackward = "move_backward",
        MoveForward = "move_forward",
        MoveLeft = "move_left",
        MoveRight = "move_right",
    }
}

arg_enum! {
    /// Captured `state`.
    pub enum PermissionState in "state" {
        Enabled = "enabled",
        Disabled = "disabled",
    }
}

impl PermissionName {
    fn permission(self) -> InputPermission {
        match self {
            Self::Camera => InputPermission::Camera,
            Self::Movement => InputPermission::Movement,
            Self::Jump => InputPermission::Jump,
            Self::LateralMovement => InputPermission::LateralMovement,
            Self::Sneak => InputPermission::Sneak,
            Self::Dismount => InputPermission::Dismount,
            Self::Mount => InputPermission::Mount,
            Self::MoveBackward => InputPermission::MoveBackward,
            Self::MoveForward => InputPermission::MoveForward,
            Self::MoveLeft => InputPermission::MoveLeft,
            Self::MoveRight => InputPermission::MoveRight,
        }
    }
}

overloads! {
    pub enum InputPermissionArgs for "inputpermission" {
        0 => Set {
            option: EnumValue,
            targets: Target,
            permission: PermissionName,
            state: PermissionState,
        },
        1 => Query {
            option: EnumValue,
            targets: Target,
            permission: PermissionName,
            state: Option<PermissionState>,
        },
    }
}

/// English text of a lang key, as BDS formats it into these answers.
fn lang(key: &str) -> String {
    en_us().get(key).unwrap_or(key).to_owned()
}

fn state_word(enabled: bool) -> String {
    lang(if enabled {
        "commands.inputpermission.enabled"
    } else {
        "commands.inputpermission.disabled"
    })
}

pub fn run(ctx: &mut CommandContext<'_>, args: InputPermissionArgs) -> CommandResult {
    match args {
        InputPermissionArgs::Set {
            targets,
            permission,
            state,
            ..
        } => {
            let players = ctx.players(Some(&targets))?;
            let input = permission.permission();
            let enabled = state == PermissionState::Enabled;
            for &player in &players {
                let mut locks = ctx
                    .world
                    .get::<InputLocks>(player)
                    .copied()
                    .unwrap_or_default();
                if enabled {
                    locks.0 &= !input.bit();
                } else {
                    locks.0 |= input.bit();
                }
                ctx.world.entity_mut(player).insert(locks);
                ctx.send(
                    player,
                    UpdateClientInputLocksPacket {
                        input_lock_component_data: locks.0,
                    },
                );
            }
            let name = lang(&format!("commands.inputpermission.{}", input.name()));
            let text = match players.as_slice() {
                [player] => en_us().format(
                    "commands.inputpermission.set.outputoneplayer",
                    &[name, state_word(enabled), ctx.name(*player)],
                ),
                _ => en_us().format(
                    "commands.inputpermission.set.outputmultipleplayers",
                    &[name, state_word(enabled), players.len().to_string()],
                ),
            };
            Ok(
                CommandOutput::success(text, vec![players.len().to_string()])
                    .with_count(players.len()),
            )
        }
        InputPermissionArgs::Query {
            targets,
            permission,
            state,
            ..
        } => {
            let players = ctx.players(Some(&targets))?;
            let input = permission.permission();
            let (enabled, disabled): (Vec<Entity>, Vec<Entity>) = players.iter().partition(|&&p| {
                ctx.world
                    .get::<InputLocks>(p)
                    .copied()
                    .unwrap_or_default()
                    .enabled(input)
            });
            let name = lang(&format!("commands.inputpermission.{}", input.name()));
            // Measured: the parameter names the players the query found (with
            // the input enabled, or in the state asked for); the verbose form
            // counts 1, the other the players in that state.
            let (text, found, count) = match state {
                None => (
                    en_us().format(
                        "commands.inputpermission.queryverbose",
                        &[name, enabled.len().to_string(), disabled.len().to_string()],
                    ),
                    enabled,
                    1,
                ),
                Some(state) => {
                    let wanted = state == PermissionState::Enabled;
                    let found = if wanted { enabled } else { disabled };
                    let text = en_us().format(
                        "commands.inputpermission.query",
                        &[name, found.len().to_string(), state_word(wanted)],
                    );
                    let count = found.len();
                    (text, found, count)
                }
            };
            Ok(CommandOutput::success(text, vec![ctx.names(&found)]).with_count(count))
        }
    }
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::{Replay, assert_same_packets};

    #[test]
    fn inputpermission_matches_the_bds_capture() {
        let steps = Replay::new().assert_segment("inputpermission", &[]);
        assert_same_packets(&steps, &["UpdateClientInputLocksPacket"], &[]);
    }
}
