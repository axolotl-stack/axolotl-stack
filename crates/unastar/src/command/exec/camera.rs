//! `/camera`, `/aimassist` and `/controlscheme`: camera instructions to
//! players.
//!
//! Measured on BDS 1.26.51 (command capture, `camera` segment):
//!
//! - every form answers `commands.camera.success [names]` and sends one
//!   `CameraInstruction` per player; `set` names its preset by index in the
//!   join's `CameraPresets` ([`PRESETS`]), an unknown name answers
//!   `commands.camera.unknown.preset`; `pos`/`facing` positions resolve like
//!   `/tp`'s (integer x/z centred), `facing <entity>` looks at the middle of
//!   its collision box; `default` also sets "remove ignore starting values";
//! - `fade` times must be within 0-10 s (`commands.camera.invalid.time`) and
//!   colours 0-255 (`commands.camera.invalid.color`), sent as fractions of
//!   255; `fov_set` takes any value;
//! - `target_entity` and `play_spline` need the free camera
//!   (`commands.generic.wrongTargetCamera` otherwise); no splines exist, so a
//!   spline answers `commands.camera.invalid` (a name without namespace also
//!   logs `[Camera] Namespace required for category: NAME`);
//! - `clear` resets the camera, and the control scheme too when one was
//!   ever set;
//! - `/controlscheme set` works only on the free, fixed boom and follow orbit
//!   cameras; `clear` sends `LockedPlayerRelativeStrafe` once a scheme was
//!   ever set; otherwise both answer `commands.controlscheme.failed [names]`;
//! - `/aimassist clear` sends the default settings (30°, 45°, 5.7 blocks,
//!   angle mode); `set` validates the preset (`error.unknownPreset`), the
//!   angles (10-90°) and the distance (1-16), then answers the *successful*
//!   `commands.aimassist.unsupportedCameraType`: BDS supports aim assist for
//!   the camera the client reports, and the capture's client (like unastar,
//!   which has no such report) never leaves first person.

use crate::command::Args;
use crate::command::prelude::*;
use crate::entity::components::Player;
use jolyne::valentine::types::{
    CameraInstruction, CameraInstructionOptionsAttachToEntityInstruction,
    CameraInstructionOptionsFadeInstruction, CameraInstructionOptionsFadeInstructionColorOption,
    CameraInstructionOptionsFadeInstructionTimeOption, CameraInstructionOptionsFovInstruction,
    CameraInstructionOptionsSetInstruction, CameraInstructionOptionsSetInstructionEaseOption,
    CameraInstructionOptionsSetInstructionEntityOffsetOption,
    CameraInstructionOptionsSetInstructionFacingOption,
    CameraInstructionOptionsSetInstructionPosOption,
    CameraInstructionOptionsSetInstructionRotOption,
    CameraInstructionOptionsSetInstructionViewOffsetOption,
    CameraInstructionOptionsTargetInstruction, CameraPresets,
    EnumsCameraAimAssistPacketPayloadAction, EnumsCameraAimAssistPacketPayloadTargetMode,
    EnumsControlSchemeScheme, SharedTypesv12650CameraPreset, Vec2, Vec3,
};
use jolyne::valentine::{
    CameraAimAssistPacket, CameraInstructionPacket, CameraPresetsPacket, CameraSplinePacket,
    ClientboundControlSchemeSetPacket,
};

/// The camera presets BDS 1.26.51 sends at join, in index order.
pub const PRESETS: [&str; 6] = [
    "minecraft:first_person",
    "minecraft:fixed_boom",
    "minecraft:follow_orbit",
    "minecraft:free",
    "minecraft:third_person",
    "minecraft:third_person_front",
];

const FREE: usize = 3;
const FIXED_BOOM: usize = 1;
const FOLLOW_ORBIT: usize = 2;

/// Aim assist presets BDS 1.26.51 knows (`/aimassist … [preset id]`).
const AIM_ASSIST_PRESETS: [&str; 1] = ["minecraft:aim_assist_default"];

/// What a player's camera was last told: the preset `/camera set` chose (none
/// after `clear`) and whether a control scheme was ever set.
#[derive(bevy_ecs::prelude::Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CameraState {
    pub preset: Option<usize>,
    pub control_scheme: bool,
}

fn state(world: &World, player: Entity) -> CameraState {
    world
        .get::<CameraState>(player)
        .copied()
        .unwrap_or_default()
}

fn set_state(world: &mut World, player: Entity, state: CameraState) {
    world.entity_mut(player).insert(state);
}

/// `/camera`: its 41 overloads share parameter names, so it reads them by
/// name rather than through one variant per overload.
pub fn run_camera(ctx: &mut CommandContext<'_>, args: &Args) -> CommandResult {
    let target: Target = args.get("players").expect("players is required");
    let players = ctx.players(Some(&target))?;
    let mut instruction = CameraInstruction::default();
    let mut new_preset = None;
    if args.has("set") {
        let preset: SoftEnumValue = args.get("preset").expect("preset is required");
        let Some(index) = PRESETS.iter().position(|p| *p == &*preset) else {
            return failure!("commands.camera.unknown.preset");
        };
        new_preset = Some(index);
        instruction.set = Some(set_instruction(ctx, args, index)?);
    } else if args.has("clear") {
        instruction.clear = Some(true);
    } else if args.has("fade") {
        instruction.fade = Some(fade_instruction(args)?);
    } else if args.has("fov_set") || args.has("fov_clear") {
        let clear = args.has("fov_clear");
        instruction.field_of_view = Some(CameraInstructionOptionsFovInstruction {
            fieldof_view: if clear {
                0.0
            } else {
                args.get("fov_value").unwrap_or_default()
            },
            fov_ease_time: args.get("fovEaseTime").unwrap_or(0.0),
            fov_ease_type: args
                .get::<EnumValue>("fovEaseType")
                .map_or("linear", |easing| easing.0)
                .into(),
            fieldof_view_clear: clear,
        });
    } else if args.has("attach_to_entity") {
        let entity: Target = args.get("entity").expect("entity is required");
        let entity = ctx.entity(Some(&entity))?;
        instruction.attach_to_entity = Some(CameraInstructionOptionsAttachToEntityInstruction {
            entity_actor_id: unique_id(ctx.world, entity),
        });
    } else if args.has("detach_from_entity") {
        instruction.detach_from_entity = Some(true);
    } else if args.has("removeTarget") {
        instruction.remove_target = Some(true);
    } else if args.has("targetEntity") {
        require_free_camera(ctx, &players)?;
        let entity: Target = args.get("entity").expect("entity is required");
        let entity = ctx.entity(Some(&entity))?;
        let offset = args.has("targetCenterOffset").then(|| Vec3 {
            x: args.get("xTargetCenterOffset").unwrap_or_default(),
            y: args.get("yTargetCenterOffset").unwrap_or_default(),
            z: args.get("zTargetCenterOffset").unwrap_or_default(),
        });
        instruction.target = Some(CameraInstructionOptionsTargetInstruction {
            target_center_offset: offset,
            target_actor_id: unique_id(ctx.world, entity),
        });
    } else if args.has("play_spline") {
        require_free_camera(ctx, &players)?;
        // No splines are defined (the join's `CameraSpline` is empty).
        let name: Text = args.get("name").expect("name is required");
        if !name.contains(':') {
            ctx.console_note(
                true,
                format!("[Camera] Namespace required for category: {}\n\n", &*name),
            );
        }
        return failure!("commands.camera.invalid");
    } else {
        return failure!("commands.camera.invalid");
    }
    for &player in &players {
        let mut camera = state(ctx.world, player);
        ctx.send(
            player,
            CameraInstructionPacket {
                camera_instruction: instruction.clone(),
            },
        );
        if let Some(index) = new_preset {
            camera.preset = Some(index);
        } else if instruction.clear.is_some() {
            camera.preset = None;
            if camera.control_scheme {
                ctx.send(
                    player,
                    scheme_packet(EnumsControlSchemeScheme::LockedPlayerRelativeStrafe),
                );
            }
        }
        set_state(ctx.world, player, camera);
    }
    Ok(CommandOutput::success(
        "commands.camera.success",
        vec![ctx.names(&players)],
    ))
}

fn require_free_camera(ctx: &CommandContext<'_>, players: &[Entity]) -> Result<(), CommandOutput> {
    if players
        .iter()
        .all(|&player| state(ctx.world, player).preset == Some(FREE))
    {
        Ok(())
    } else {
        Err(CommandOutput::failure(
            "commands.generic.wrongTargetCamera",
            vec![],
        ))
    }
}

fn set_instruction(
    ctx: &mut CommandContext<'_>,
    args: &Args,
    preset: usize,
) -> Result<CameraInstructionOptionsSetInstruction, CommandOutput> {
    let default = args.has("default");
    let ease = args
        .has("ease")
        .then(|| CameraInstructionOptionsSetInstructionEaseOption {
            type_: easing_index(ctx, args.get::<EnumValue>("easeType")),
            time: args.get("easeTime").unwrap_or_default(),
        });
    let pos = args.get::<CommandPosition>("position").map(|position| {
        CameraInstructionOptionsSetInstructionPosOption {
            pos: vec3(ctx.position(Some(position))),
        }
    });
    let rot = args.has("rot").then(|| {
        let (yaw, pitch) = ctx.rotation(args.get("yRot"), args.get("xRot"));
        CameraInstructionOptionsSetInstructionRotOption { x: pitch, y: yaw }
    });
    let facing = if let Some(target) = args.get::<Target>("lookAtEntity") {
        let entity = ctx.entity(Some(&target))?;
        let feet = ctx.feet(entity).unwrap_or_default();
        Some(feet + DVec3::Y * collision_height(ctx.world, entity) / 2.0)
    } else {
        args.get::<CommandPosition>("lookAtPosition")
            .map(|position| ctx.position(Some(position)))
    };
    Ok(CameraInstructionOptionsSetInstruction {
        preset: preset as u32,
        ease,
        pos,
        rot,
        facing: facing
            .map(|at| CameraInstructionOptionsSetInstructionFacingOption { pos: vec3(at) }),
        view_offset: args.has("view_offset").then(|| {
            CameraInstructionOptionsSetInstructionViewOffsetOption {
                x: args.get("xViewOffset").unwrap_or_default(),
                y: args.get("yViewOffset").unwrap_or_default(),
            }
        }),
        entity_offset: args.has("entity_offset").then(|| {
            CameraInstructionOptionsSetInstructionEntityOffsetOption {
                entity_offset_x: args.get("xEntityOffset").unwrap_or_default(),
                entity_offset_y: args.get("yEntityOffset").unwrap_or_default(),
                entity_offset_z: args.get("zEntityOffset").unwrap_or_default(),
            }
        }),
        default: default.then_some(true),
        remove_ignore_starting_values_component: default,
    })
}

fn fade_instruction(args: &Args) -> Result<CameraInstructionOptionsFadeInstruction, CommandOutput> {
    let time = if args.has("time") {
        let times: [f32; 3] = [
            args.get("fadeInSeconds").unwrap_or_default(),
            args.get("holdSeconds").unwrap_or_default(),
            args.get("fadeOutSeconds").unwrap_or_default(),
        ];
        if times.iter().any(|t| !(0.0..=10.0).contains(t)) {
            return Err(CommandOutput::failure(
                "commands.camera.invalid.time",
                vec![],
            ));
        }
        Some(CameraInstructionOptionsFadeInstructionTimeOption {
            fade_in_time: times[0],
            hold_time: times[1],
            fade_out_time: times[2],
        })
    } else {
        None
    };
    let color = if args.has("color") {
        let rgb: [i32; 3] = [
            args.get("red").unwrap_or_default(),
            args.get("green").unwrap_or_default(),
            args.get("blue").unwrap_or_default(),
        ];
        if rgb.iter().any(|c| !(0..=255).contains(c)) {
            return Err(CommandOutput::failure(
                "commands.camera.invalid.color",
                vec![],
            ));
        }
        // BDS scales by the reciprocal (30 → 0.11764707, not 30 / 255).
        let [red, green, blue] = rgb.map(|c| c as f32 * (1.0 / 255.0));
        Some(CameraInstructionOptionsFadeInstructionColorOption { red, green, blue })
    } else {
        None
    };
    Ok(CameraInstructionOptionsFadeInstruction { time, color })
}

/// Index of an easing in the captured `Easing` enum (the wire's ease type).
fn easing_index(ctx: &CommandContext<'_>, easing: Option<EnumValue>) -> u8 {
    let Some(easing) = easing else { return 0 };
    ctx.schema
        .syntax()
        .enum_named("Easing")
        .and_then(|e| e.values.iter().position(|v| v == &*easing))
        .unwrap_or(0) as u8
}

fn vec3(v: DVec3) -> Vec3 {
    Vec3 {
        x: v.x as f32,
        y: v.y as f32,
        z: v.z as f32,
    }
}

/// Height of an entity's collision box (players 1.8).
fn collision_height(world: &World, entity: Entity) -> f64 {
    use jolyne::valentine::actor_data_i_ds as ids;
    use jolyne::valentine::types::DataItemEntryPayload;
    if world.get::<Player>(entity).is_some() {
        return 1.8;
    }
    world
        .get::<crate::entity::definition::metadata::EntityMetadata>(entity)
        .and_then(|metadata| match metadata.get(ids::RESERVED_054)? {
            DataItemEntryPayload::DataItemFloatPayload(height) => Some(f64::from(height.value)),
            _ => None,
        })
        .unwrap_or_default()
}

fn unique_id(world: &World, entity: Entity) -> i64 {
    super::visuals::runtime_id(world, entity).unwrap_or(-1)
}

fn scheme_packet(scheme: EnumsControlSchemeScheme) -> ClientboundControlSchemeSetPacket {
    ClientboundControlSchemeSetPacket {
        control_scheme: scheme,
    }
}

arg_enum! {
    /// Captured `controlscheme`.
    pub enum ControlScheme in "controlscheme" {
        CameraRelativeStrafe = "camera_relative_strafe",
        CameraRelative = "camera_relative",
        PlayerRelativeStrafe = "player_relative_strafe",
        PlayerRelative = "player_relative",
        LockedPlayerRelativeStrafe = "locked_player_relative_strafe",
    }
}

overloads! {
    pub enum ControlSchemeArgs for "controlscheme" {
        0 => Set { players: Target, set: EnumValue, scheme: ControlScheme = "control scheme" },
        1 => Clear { players: Target, clear: EnumValue },
    }
}

pub fn run_controlscheme(ctx: &mut CommandContext<'_>, args: ControlSchemeArgs) -> CommandResult {
    let (target, scheme) = match args {
        ControlSchemeArgs::Set {
            players, scheme, ..
        } => (players, Some(scheme)),
        ControlSchemeArgs::Clear { players, .. } => (players, None),
    };
    let players = ctx.players(Some(&target))?;
    let allowed = |camera: CameraState| match scheme {
        Some(_) => matches!(camera.preset, Some(FREE | FIXED_BOOM | FOLLOW_ORBIT)),
        None => camera.control_scheme,
    };
    if !players.iter().all(|&p| allowed(state(ctx.world, p))) {
        return failure!("commands.controlscheme.failed", ctx.names(&players));
    }
    let wire = match scheme {
        None | Some(ControlScheme::LockedPlayerRelativeStrafe) => {
            EnumsControlSchemeScheme::LockedPlayerRelativeStrafe
        }
        Some(ControlScheme::CameraRelative) => EnumsControlSchemeScheme::CameraRelative,
        Some(ControlScheme::CameraRelativeStrafe) => EnumsControlSchemeScheme::CameraRelativeStrafe,
        Some(ControlScheme::PlayerRelative) => EnumsControlSchemeScheme::PlayerRelative,
        Some(ControlScheme::PlayerRelativeStrafe) => EnumsControlSchemeScheme::PlayerRelativeStrafe,
    };
    for &player in &players {
        ctx.send(player, scheme_packet(wire));
        let mut camera = state(ctx.world, player);
        camera.control_scheme = true;
        set_state(ctx.world, player, camera);
    }
    success!("commands.controlscheme.success", ctx.names(&players))
}

arg_enum! {
    /// Captured `AimAssistTargetMode`.
    pub enum AimAssistTargetMode in "AimAssistTargetMode" {
        Distance = "distance",
        Angle = "angle",
    }
}

overloads! {
    pub enum AimAssistArgs for "aimassist" {
        0 => Set {
            players: Target,
            set: EnumValue,
            x_angle: Option<f32> = "x angle",
            y_angle: Option<f32> = "y angle",
            distance: Option<f32> = "max distance",
            mode: Option<AimAssistTargetMode> = "target mode",
            preset: Option<Text> = "preset id",
        },
        1 => Clear { players: Target, clear: EnumValue },
    }
}

pub fn run_aimassist(ctx: &mut CommandContext<'_>, args: AimAssistArgs) -> CommandResult {
    match args {
        AimAssistArgs::Clear { players, .. } => {
            let players = ctx.players(Some(&players))?;
            for &player in &players {
                ctx.send(
                    player,
                    CameraAimAssistPacket {
                        preset_id: "".into(),
                        view_angle: Vec2 { x: 30.0, y: 45.0 },
                        distance: 5.7,
                        target_mode: EnumsCameraAimAssistPacketPayloadTargetMode::Angle,
                        action: EnumsCameraAimAssistPacketPayloadAction::Clear,
                        show_debug_render: false,
                    },
                );
            }
            success!("commands.aimassist.success", ctx.names(&players))
        }
        AimAssistArgs::Set {
            players,
            x_angle,
            y_angle,
            distance,
            preset,
            ..
        } => {
            let players = ctx.players(Some(&players))?;
            if let Some(preset) = preset
                && !AIM_ASSIST_PRESETS.contains(&&*preset)
            {
                return failure!("commands.aimassist.error.unknownPreset", &*preset);
            }
            if x_angle.is_some_and(|x| !(10.0..=90.0).contains(&x)) {
                return failure!("commands.aimassist.invalidxangle");
            }
            if y_angle.is_some_and(|y| !(10.0..=90.0).contains(&y)) {
                return failure!("commands.aimassist.invalidyangle");
            }
            if distance.is_some_and(|d| !(1.0..=16.0).contains(&d)) {
                return failure!("commands.aimassist.invaliddistance");
            }
            // The client reports no camera type unastar could check: like
            // BDS for a client that never left first person.
            let _ = players;
            success!("commands.aimassist.unsupportedCameraType")
        }
    }
}

/// The join's camera definitions, as BDS 1.26.51 sends them after
/// `PlayerFog`: the presets [`PRESETS`] (indices of `/camera set`), then
/// (no) splines, with its aim assist definitions between them. The join
/// sends the vendored BDS payloads (`server::game::join_data`); this builder
/// pins the `/camera set` preset indices to the same capture.
pub fn join_packets() -> (CameraPresetsPacket, CameraSplinePacket) {
    let preset = |name: &str| SharedTypesv12650CameraPreset {
        name: name.into(),
        apply_inherited_starting_rotation: true,
        ..Default::default()
    };
    let offsets = |mut preset: SharedTypesv12650CameraPreset| {
        preset.view_offset = Some(Vec2 { x: 0.0, y: 0.0 });
        preset.entity_offset = Some(Vec3 {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        });
        preset
    };
    let mut follow_orbit = offsets(preset(PRESETS[FOLLOW_ORBIT]));
    follow_orbit.radius = Some(10.0);
    let mut free = preset(PRESETS[FREE]);
    free.pos_x = Some(0.0);
    free.pos_y = Some(0.0);
    free.pos_z = Some(0.0);
    free.rot_x = Some(0.0);
    free.rot_y = Some(0.0);
    let presets = CameraPresetsPacket {
        camera_presets: CameraPresets {
            presets: vec![
                preset(PRESETS[0]),
                offsets(preset(PRESETS[FIXED_BOOM])),
                follow_orbit,
                free,
                preset(PRESETS[4]),
                preset(PRESETS[5]),
            ],
        },
    };
    (presets, CameraSplinePacket::default())
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use super::*;
    use crate::command::exec::evidence::{Replay, assert_same_packets, decode_file};
    use jolyne::valentine::McpePacketData;

    /// Lines naming the segment's cow, whose unique id differs between servers.
    const COW_LINES: [&str; 3] = [
        "/camera @s target_entity @e[type=cow,r=16,c=1]",
        "/camera @s target_entity @e[type=cow,r=16,c=1] target_center_offset 0 1 0",
        "/camera @s attach_to_entity @e[type=cow,r=16,c=1]",
    ];

    #[test]
    fn camera_aimassist_and_controlscheme_match_the_bds_capture() {
        let steps = Replay::new().assert_segment("camera", &[]);
        assert_same_packets(
            &steps,
            &[
                "CameraInstructionPacket",
                "CameraAimAssistPacket",
                "ClientboundControlSchemeSetPacket",
            ],
            &COW_LINES,
        );
        // The cow lines carry the same instruction for unastar's cow.
        for step in steps
            .iter()
            .filter(|s| COW_LINES.contains(&s.captured.command.as_str()))
        {
            let ids = |packets: Vec<&McpePacketData>| -> Vec<CameraInstruction> {
                packets
                    .into_iter()
                    .filter_map(|p| match p {
                        McpePacketData::CameraInstructionPacket(p) => {
                            let mut instruction = p.camera_instruction.clone();
                            if let Some(target) = &mut instruction.target {
                                target.target_actor_id = 0;
                            }
                            if let Some(attach) = &mut instruction.attach_to_entity {
                                attach.entity_actor_id = 0;
                            }
                            Some(instruction)
                        }
                        _ => None,
                    })
                    .collect()
            };
            assert_eq!(
                ids(step.sent_named("CameraInstructionPacket")),
                ids(step.captured.named("CameraInstructionPacket")),
                "{}",
                step.captured.command
            );
        }
    }

    #[test]
    fn join_presets_match_the_bds_join() {
        let (presets, splines) = join_packets();
        assert_eq!(
            decode_file("member/join/00043-CameraPresetsPacket.bin"),
            McpePacketData::CameraPresetsPacket(presets)
        );
        assert_eq!(
            decode_file("member/join/00045-CameraSplinePacket.bin"),
            McpePacketData::CameraSplinePacket(splines)
        );
    }
}
