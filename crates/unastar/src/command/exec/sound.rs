//! `/playsound` and `/stopsound`.
//!
//! Measured on BDS 1.26.51:
//! - `PlaySound` carries the position times 8, truncated; without a position
//!   the sound plays at the target's eyes, `~ ~ ~` resolves from the
//!   source's feet; the answer is `commands.playsound.success.single
//!   [sound, name]`.
//! - A target farther than the sound carries (16 blocks, times the volume
//!   when it is above 1) hears it only with a minimum volume: then the sound
//!   moves 2 blocks from the target towards it and plays at the minimum
//!   volume; with none it is skipped, and when every target is skipped the
//!   answer is `commands.playsound.failed`.
//! - `/stopsound` sends `StopSound` (all sounds when no name is given) and
//!   answers `commands.stopsound.success [sound]` or
//!   `commands.stopsound.success.all`.
//! - Non-player targets fail with `commands.generic.targetNotPlayer`.

use crate::command::prelude::*;
use jolyne::valentine::types::BlockPos;
use jolyne::valentine::{PlaySoundPacket, StopSoundPacket};

overloads! {
    pub enum PlaySoundArgs for "playsound" {
        0 => Play {
            sound: Text,
            player: Option<Target>,
            position: Option<CommandPosition>,
            volume: Option<f32>,
            pitch: Option<f32>,
            minimum_volume: Option<f32> = "minimumVolume",
        },
    }
}

overloads! {
    pub enum StopSoundArgs for "stopsound" {
        0 => Stop { player: Target, sound: Option<Text> },
    }
}

/// How far a sound of volume 1 carries.
const AUDIBLE_DISTANCE: f64 = 16.0;

pub fn run_playsound(ctx: &mut CommandContext<'_>, args: PlaySoundArgs) -> CommandResult {
    let PlaySoundArgs::Play {
        sound,
        player,
        position,
        volume,
        pitch,
        minimum_volume,
    } = args;
    let players = ctx.players(player.as_ref())?;
    let position = position.map(|p| ctx.position(Some(p)));
    let volume = volume.unwrap_or(1.0);
    let pitch = pitch.unwrap_or(1.0);
    let minimum_volume = minimum_volume.unwrap_or(0.0);
    let reach = AUDIBLE_DISTANCE * f64::from(volume.max(1.0));
    let mut heard = Vec::new();
    for player in players {
        let Some(ear) = ctx.eye(player) else {
            continue;
        };
        let at = position.unwrap_or(ear);
        let (at, volume) = if at.distance_squared(ear) > reach * reach {
            if minimum_volume <= 0.0 {
                continue;
            }
            (ear + (at - ear).normalize() * 2.0, minimum_volume)
        } else {
            (at, volume)
        };
        ctx.send(player, play_sound(&sound, at, volume, pitch));
        heard.push(player);
    }
    match heard.as_slice() {
        [] => failure!("commands.playsound.failed"),
        [player] => success!(
            "commands.playsound.success.single",
            &*sound,
            ctx.name(*player)
        ),
        // [INFERENCE] Several listeners: the lang's plural line.
        _ => Ok(CommandOutput::success(
            "commands.playsound.success",
            vec![sound.to_string(), ctx.names(&heard)],
        )
        .with_count(heard.len())),
    }
}

fn play_sound(name: &str, at: DVec3, volume: f32, pitch: f32) -> PlaySoundPacket {
    // Eighths of a block, truncated towards zero.
    let eighths = |v: f64| (v * 8.0) as i32;
    PlaySoundPacket {
        name: name.into(),
        position: BlockPos {
            x: eighths(at.x),
            y: eighths(at.y),
            z: eighths(at.z),
        },
        volume,
        pitch,
        loop_count: 0,
        bypass_listener_range_check: false,
        server_sound_handle: None,
        playback_position_seconds: None,
    }
}

pub fn run_stopsound(ctx: &mut CommandContext<'_>, args: StopSoundArgs) -> CommandResult {
    let StopSoundArgs::Stop { player, sound } = args;
    let players = ctx.players(Some(&player))?;
    let packet = StopSoundPacket {
        sound_name: sound.as_deref().unwrap_or_default().into(),
        stop_all_sounds: sound.is_none(),
        stop_music_legacy: false,
    };
    for &player in &players {
        ctx.send(player, packet.clone());
    }
    let output = match &sound {
        Some(sound) => {
            CommandOutput::success("commands.stopsound.success", vec![sound.to_string()])
        }
        None => CommandOutput::success("commands.stopsound.success.all", vec![]),
    };
    Ok(output.with_count(players.len()))
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::{Replay, assert_same_packets};

    #[test]
    fn playsound_and_stopsound_match_the_bds_capture() {
        let steps = Replay::new().assert_segment("sound", &[]);
        assert_same_packets(&steps, &["PlaySoundPacket", "StopSoundPacket"], &[]);
    }
}
