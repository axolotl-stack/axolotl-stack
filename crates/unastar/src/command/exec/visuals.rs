//! Commands that only drive the client's presentation: `/particle`,
//! `/camerashake`, `/hud`, `/fog` and `/playanimation`.
//!
//! Measured on BDS 1.26.51:
//! - `/particle` sends `SpawnParticleEffect` (dimension 0, actor -1, no
//!   Molang variables) at the position, the source's feet by default, for
//!   any effect name, and answers `commands.spawnParticleEmitter.success`.
//! - `/camerashake add` sends `CameraShake` (defaults: intensity 0.5, one
//!   second, positional; intensity 0-4, at least 0.01 seconds, else
//!   `commands.generic.num.tooSmall|tooBig` with two decimals) and answers
//!   `commands.screenshake.success`;
//!   `stop` sends a zero shake with action stop and answers
//!   `commands.screenshake.successStop`, both with the players' names.
//! - `/hud` sends `SetHud` with the element, or all thirteen elements for
//!   `all` (the default) in the order BDS lists them, and answers
//!   `commands.hud.success`.
//! - `/fog` keeps a stack of fog settings per player, each pushed under a
//!   user id: `push` adds one, `pop` removes the newest with the id,
//!   `remove` every one with it; each change sends `PlayerFog` with the
//!   stack's fog ids, and an unknown id answers `commands.fog.invalidUserId`.
//! - `/playanimation` sends one `AnimateEntity` naming every target (next
//!   state `default`, stop expression `query.any_animation_finished`,
//!   controller `__runtime_controller` unless given) and answers
//!   `commands.playanimation.success`.

use crate::command::prelude::*;
use crate::entity::components::{RuntimeEntityId, RuntimeId};
use bevy_ecs::prelude::Component;
use jolyne::valentine::types::{
    ActorRuntimeId, ActorUniqueId, EnumsCameraShakeAction, EnumsCameraShakeType, EnumsHudElement,
    EnumsHudVisibility,
};
use jolyne::valentine::{
    AnimateEntityPacket, CameraShakePacket, PlayerFogPacket, SetHudPacket,
    SpawnParticleEffectPacket,
};

overloads! {
    pub enum ParticleArgs for "particle" {
        0 => Spawn { effect: Text, position: Option<CommandPosition> },
    }
}

pub fn run_particle(ctx: &mut CommandContext<'_>, args: ParticleArgs) -> CommandResult {
    let ParticleArgs::Spawn { effect, position } = args;
    let at = ctx.position(position);
    ctx.broadcast(SpawnParticleEffectPacket {
        dimension_id: 0,
        actor_id: ActorUniqueId {
            actor_unique_id: -1,
        },
        position: jolyne::valentine::types::Vec3 {
            x: at.x as f32,
            y: at.y as f32,
            z: at.z as f32,
        },
        effect_name: effect.0.as_str().into(),
        molang_variables: None,
    });
    success!("commands.spawnParticleEmitter.success", &*effect)
}

arg_enum! {
    /// Captured `CameraShakeType`.
    pub enum ShakeType in "CameraShakeType" {
        Positional = "positional",
        Rotational = "rotational",
    }
}

overloads! {
    pub enum CameraShakeArgs for "camerashake" {
        0 => Add {
            action: EnumValue,
            player: Target,
            intensity: Option<f32>,
            seconds: Option<f32>,
            shake_type: Option<ShakeType> = "shakeType",
        },
        1 => Stop { action: EnumValue, player: Option<Target> },
    }
}

const MAX_SHAKE_INTENSITY: f32 = 4.0;
const MIN_SHAKE_SECONDS: f32 = 0.01;

/// `commands.generic.num.tooSmall|tooBig` for a decimal, written as BDS
/// writes it (`5.00`).
pub(super) fn float_in_range(value: f32, min: f32, max: Option<f32>) -> Result<(), CommandOutput> {
    let shown = |v: f32| format::decimal(f64::from(v));
    if value < min {
        return failure!("commands.generic.num.tooSmall", shown(value), shown(min));
    }
    if let Some(max) = max.filter(|&max| value > max) {
        return failure!("commands.generic.num.tooBig", shown(value), shown(max));
    }
    Ok(())
}

pub fn run_camerashake(ctx: &mut CommandContext<'_>, args: CameraShakeArgs) -> CommandResult {
    let (player, packet, key) = match args {
        CameraShakeArgs::Add {
            player,
            intensity,
            seconds,
            shake_type,
            ..
        } => {
            let intensity = intensity.unwrap_or(0.5);
            let seconds = seconds.unwrap_or(1.0);
            // Measured bounds: intensity 0-4, at least 0.01 seconds.
            float_in_range(intensity, 0.0, Some(MAX_SHAKE_INTENSITY))?;
            float_in_range(seconds, MIN_SHAKE_SECONDS, None)?;
            let packet = CameraShakePacket {
                intensity,
                seconds,
                shake_type: match shake_type.unwrap_or(ShakeType::Positional) {
                    ShakeType::Positional => EnumsCameraShakeType::Positional,
                    ShakeType::Rotational => EnumsCameraShakeType::Rotational,
                },
                shake_action: EnumsCameraShakeAction::Add,
            };
            (Some(player), packet, "commands.screenshake.success")
        }
        CameraShakeArgs::Stop { player, .. } => {
            let packet = CameraShakePacket {
                intensity: 0.0,
                seconds: 0.0,
                shake_type: EnumsCameraShakeType::Positional,
                shake_action: EnumsCameraShakeAction::Stop,
            };
            (player, packet, "commands.screenshake.successStop")
        }
    };
    let players = ctx.players(player.as_ref())?;
    for &player in &players {
        ctx.send(player, packet.clone());
    }
    Ok(CommandOutput::success(key, vec![ctx.names(&players)]).with_count(players.len()))
}

arg_enum! {
    /// Captured `HudVisibility`.
    pub enum HudVisibility in "HudVisibility" {
        Hide = "hide",
        Reset = "reset",
    }
}

arg_enum! {
    /// Captured `HudElement`.
    pub enum HudElement in "HudElement" {
        All = "all",
        Hunger = "hunger",
        Paperdoll = "paperdoll",
        Armor = "armor",
        Tooltips = "tooltips",
        TouchControls = "touch_controls",
        Crosshair = "crosshair",
        Hotbar = "hotbar",
        Health = "health",
        ProgressBar = "progress_bar",
        AirBubbles = "air_bubbles",
        HorseHealth = "horse_health",
        StatusEffects = "status_effects",
        ItemText = "item_text",
    }
}

/// `all`, in the order BDS sends it.
const ALL_HUD_ELEMENTS: [EnumsHudElement; 13] = [
    EnumsHudElement::Armor,
    EnumsHudElement::Crosshair,
    EnumsHudElement::Hotbar,
    EnumsHudElement::Paperdoll,
    EnumsHudElement::Tooltips,
    EnumsHudElement::Touchcontrols,
    EnumsHudElement::Health,
    EnumsHudElement::Progressbar,
    EnumsHudElement::Hunger,
    EnumsHudElement::Airbubbles,
    EnumsHudElement::Horsehealth,
    EnumsHudElement::Statuseffects,
    EnumsHudElement::Itemtext,
];

impl HudElement {
    fn wire(self) -> Vec<EnumsHudElement> {
        let one = match self {
            Self::All => return ALL_HUD_ELEMENTS.to_vec(),
            Self::Hunger => EnumsHudElement::Hunger,
            Self::Paperdoll => EnumsHudElement::Paperdoll,
            Self::Armor => EnumsHudElement::Armor,
            Self::Tooltips => EnumsHudElement::Tooltips,
            Self::TouchControls => EnumsHudElement::Touchcontrols,
            Self::Crosshair => EnumsHudElement::Crosshair,
            Self::Hotbar => EnumsHudElement::Hotbar,
            Self::Health => EnumsHudElement::Health,
            Self::ProgressBar => EnumsHudElement::Progressbar,
            Self::AirBubbles => EnumsHudElement::Airbubbles,
            Self::HorseHealth => EnumsHudElement::Horsehealth,
            Self::StatusEffects => EnumsHudElement::Statuseffects,
            Self::ItemText => EnumsHudElement::Itemtext,
        };
        vec![one]
    }
}

overloads! {
    pub enum HudArgs for "hud" {
        0 => Set {
            target: Target,
            visible: HudVisibility,
            hud_element: Option<HudElement>,
        },
    }
}

pub fn run_hud(ctx: &mut CommandContext<'_>, args: HudArgs) -> CommandResult {
    let HudArgs::Set {
        target,
        visible,
        hud_element,
    } = args;
    let players = ctx.players(Some(&target))?;
    let packet = SetHudPacket {
        hud_element: hud_element.unwrap_or(HudElement::All).wire(),
        hud_visible: match visible {
            HudVisibility::Hide => EnumsHudVisibility::Hide,
            HudVisibility::Reset => EnumsHudVisibility::Reset,
        },
    };
    for &player in &players {
        ctx.send(player, packet.clone());
    }
    Ok(CommandOutput::success("commands.hud.success", vec![]).with_count(players.len()))
}

/// A player's fog stack: `(fog id, user id)`, oldest first.
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct FogStack(pub Vec<(String, String)>);

overloads! {
    pub enum FogArgs for "fog" {
        0 => Push {
            victim: Target,
            mode: EnumValue,
            fog_id: Text = "fogId",
            user_id: Text = "userProvidedId",
        },
        1 => Delete { victim: Target, mode: EnumValue, user_id: Text = "userProvidedId" },
    }
}

pub fn run_fog(ctx: &mut CommandContext<'_>, args: FogArgs) -> CommandResult {
    let (victim, change) = match args {
        FogArgs::Push {
            victim,
            fog_id,
            user_id,
            ..
        } => (victim, FogChange::Push(fog_id.0, user_id.0)),
        FogArgs::Delete {
            victim,
            mode,
            user_id,
        } => {
            let change = if &*mode == "pop" {
                FogChange::Pop(user_id.0)
            } else {
                FogChange::Remove(user_id.0)
            };
            (victim, change)
        }
    };
    let players = ctx.players(Some(&victim))?;
    let mut output = CommandOutput::default();
    for player in players {
        let name = ctx.name(player);
        let mut stack = ctx
            .world
            .get::<FogStack>(player)
            .cloned()
            .unwrap_or_default();
        let (key, id) = match &change {
            FogChange::Push(fog, id) => {
                stack.0.push((fog.clone(), id.clone()));
                ("commands.fog.success.push", id)
            }
            FogChange::Pop(id) => {
                let Some(index) = stack.0.iter().rposition(|(_, user)| user == id) else {
                    output.add_failure("commands.fog.invalidUserId", vec![name, id.clone()]);
                    continue;
                };
                stack.0.remove(index);
                ("commands.fog.success.pop", id)
            }
            FogChange::Remove(id) => {
                if !stack.0.iter().any(|(_, user)| user == id) {
                    output.add_failure("commands.fog.invalidUserId", vec![name, id.clone()]);
                    continue;
                }
                stack.0.retain(|(_, user)| user != id);
                ("commands.fog.success.remove", id)
            }
        };
        ctx.send(
            player,
            PlayerFogPacket {
                fog_stack: stack.0.iter().map(|(fog, _)| fog.as_str().into()).collect(),
            },
        );
        ctx.world.entity_mut(player).insert(stack);
        output.add_success(key, vec![name, id.clone()]);
    }
    if output.success_count == 0 {
        return Err(output);
    }
    Ok(output)
}

enum FogChange {
    Push(String, String),
    Pop(String),
    Remove(String),
}

overloads! {
    pub enum PlayAnimationArgs for "playanimation" {
        0 => Play {
            entity: Target,
            animation: Text,
            next_state: Option<Text>,
            blend_out_time: Option<f32>,
            stop_expression: Option<Text>,
            controller: Option<Text>,
        },
    }
}

pub fn run_playanimation(ctx: &mut CommandContext<'_>, args: PlayAnimationArgs) -> CommandResult {
    let PlayAnimationArgs::Play {
        entity,
        animation,
        next_state,
        blend_out_time,
        stop_expression,
        controller,
    } = args;
    let targets = ctx.entities(Some(&entity))?;
    let runtime_ids = targets
        .iter()
        .filter_map(|&e| runtime_id(ctx.world, e))
        .map(|id| ActorRuntimeId {
            actor_runtime_id: id as u64,
        })
        .collect();
    ctx.broadcast(AnimateEntityPacket {
        m_animation: animation.0.as_str().into(),
        m_next_state: next_state.as_deref().unwrap_or("default").into(),
        m_stop_expression: stop_expression
            .as_deref()
            .unwrap_or("query.any_animation_finished")
            .into(),
        m_stop_expression_version: 1,
        m_controller: controller
            .as_deref()
            .unwrap_or("__runtime_controller")
            .into(),
        m_blend_out_time: blend_out_time.unwrap_or(0.0),
        m_runtime_ids: runtime_ids,
    });
    Ok(CommandOutput::success(
        "commands.playanimation.success",
        vec![],
    ))
}

/// The runtime id the client knows an entity by.
pub(super) fn runtime_id(world: &World, entity: Entity) -> Option<i64> {
    world
        .get::<RuntimeEntityId>(entity)
        .map(|id| id.0)
        .or_else(|| world.get::<RuntimeId>(entity).map(|id| id.0))
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::{Replay, assert_same_packets};

    #[test]
    fn particle_matches_the_bds_capture() {
        let steps = Replay::new().assert_segment("particle", &[]);
        assert_same_packets(&steps, &["SpawnParticleEffectPacket"], &[]);
    }

    #[test]
    fn camerashake_matches_the_bds_capture() {
        let steps = Replay::new().assert_segment("camerashake", &[]);
        assert_same_packets(&steps, &["CameraShakePacket"], &[]);
    }

    #[test]
    fn hud_matches_the_bds_capture() {
        let steps = Replay::new().assert_segment("hud", &[]);
        assert_same_packets(&steps, &["SetHudPacket"], &[]);
    }

    #[test]
    fn fog_matches_the_bds_capture() {
        let steps = Replay::new().assert_segment("fog", &[]);
        assert_same_packets(&steps, &["PlayerFogPacket"], &[]);
    }

    #[test]
    fn playanimation_matches_the_bds_capture() {
        let steps = Replay::new().assert_segment("playanimation", &[]);
        assert_same_packets(
            &steps,
            &["AnimateEntityPacket"],
            &["/playanimation @e[type=cow,r=16] animation.cow.x"],
        );
    }
}
