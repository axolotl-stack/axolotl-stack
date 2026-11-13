//! `/music play|queue|stop|volume`: drives the client's music player.
//!
//! Measured on BDS 1.26.51: each action is a `LevelEventGeneric` whose data
//! is an NBT compound with sorted keys — play (1901) and queue (1900) carry
//! `fadeSeconds` (float), `repeatMode` (byte: 0 play once, 1 loop),
//! `trackName` and `volume` (float); stop (1902) `fadeSeconds`; volume
//! (1903) `volume`. Defaults: volume 1, no fade, play once. Answers:
//! `commands.music.success.playAction|queueAction [track]`,
//! `…stopAction`, `…volumeAction [volume with two decimals]`. Bounds: play
//! and queue volume 0.01-1, the volume action 0-1, fade at least 0, else
//! `commands.generic.num.tooSmall|tooBig` with two decimals.

use super::visuals::float_in_range;
use crate::command::prelude::*;
use jolyne::valentine::LevelEventGenericPacket;
use jolyne::valentine::bedrock::codec::NbtCompoundBody;

const EVENT_QUEUE: i32 = 1900;
const EVENT_PLAY: i32 = 1901;
const EVENT_STOP: i32 = 1902;
const EVENT_VOLUME: i32 = 1903;

const MIN_TRACK_VOLUME: f32 = 0.01;
const MAX_VOLUME: f32 = 1.0;

arg_enum! {
    /// Captured `MusicRepeatMode`.
    pub enum RepeatMode in "MusicRepeatMode" {
        PlayOnce = "play_once",
        Loop = "loop",
    }
}

overloads! {
    pub enum MusicArgs for "music" {
        0 => Queue {
            action: EnumValue,
            track: Text = "trackName",
            volume: Option<f32>,
            fade: Option<f32> = "fadeSeconds",
            repeat: Option<RepeatMode> = "repeatMode",
        },
        1 => Play {
            action: EnumValue,
            track: Text = "trackName",
            volume: Option<f32>,
            fade: Option<f32> = "fadeSeconds",
            repeat: Option<RepeatMode> = "repeatMode",
        },
        2 => Stop { action: EnumValue, fade: Option<f32> = "fadeSeconds" },
        3 => Volume { action: EnumValue, volume: f32 },
    }
}

pub fn run(ctx: &mut CommandContext<'_>, args: MusicArgs) -> CommandResult {
    let (event, data, output) = match args {
        MusicArgs::Queue {
            track,
            volume,
            fade,
            repeat,
            ..
        } => {
            let data = track_data(&track, volume, fade, repeat)?;
            (
                EVENT_QUEUE,
                data,
                CommandOutput::success("commands.music.success.queueAction", vec![track.0]),
            )
        }
        MusicArgs::Play {
            track,
            volume,
            fade,
            repeat,
            ..
        } => {
            let data = track_data(&track, volume, fade, repeat)?;
            (
                EVENT_PLAY,
                data,
                CommandOutput::success("commands.music.success.playAction", vec![track.0]),
            )
        }
        MusicArgs::Stop { fade, .. } => {
            let fade = fade.unwrap_or(0.0);
            float_in_range(fade, 0.0, None)?;
            let mut data = Compound::default();
            data.float("fadeSeconds", fade);
            (
                EVENT_STOP,
                data,
                CommandOutput::success("commands.music.success.stopAction", vec![]),
            )
        }
        MusicArgs::Volume { volume, .. } => {
            float_in_range(volume, 0.0, Some(MAX_VOLUME))?;
            let mut data = Compound::default();
            data.float("volume", volume);
            (
                EVENT_VOLUME,
                data,
                CommandOutput::success(
                    "commands.music.success.volumeAction",
                    vec![format::decimal(f64::from(volume))],
                ),
            )
        }
    };
    ctx.broadcast(LevelEventGenericPacket {
        event_id: event,
        __ctd__: data.finish(),
    });
    Ok(output)
}

fn track_data(
    track: &str,
    volume: Option<f32>,
    fade: Option<f32>,
    repeat: Option<RepeatMode>,
) -> Result<Compound, CommandOutput> {
    let volume = volume.unwrap_or(MAX_VOLUME);
    float_in_range(volume, MIN_TRACK_VOLUME, Some(MAX_VOLUME))?;
    let fade = fade.unwrap_or(0.0);
    float_in_range(fade, 0.0, None)?;
    let mut data = Compound::default();
    data.float("fadeSeconds", fade);
    data.byte("repeatMode", u8::from(repeat == Some(RepeatMode::Loop)));
    data.string("trackName", track);
    data.float("volume", volume);
    Ok(data)
}

/// The children of a network little-endian NBT compound, written in the
/// order given (callers give BDS's sorted order).
#[derive(Default)]
struct Compound(Vec<u8>);

impl Compound {
    const BYTE: u8 = 1;
    const FLOAT: u8 = 5;
    const STRING: u8 = 8;

    fn name(&mut self, tag: u8, name: &str) {
        self.0.push(tag);
        self.var_len(name.len());
        self.0.extend_from_slice(name.as_bytes());
    }

    fn var_len(&mut self, mut len: usize) {
        while len >= 0x80 {
            self.0.push(len as u8 | 0x80);
            len >>= 7;
        }
        self.0.push(len as u8);
    }

    fn byte(&mut self, name: &str, value: u8) {
        self.name(Self::BYTE, name);
        self.0.push(value);
    }

    fn float(&mut self, name: &str, value: f32) {
        self.name(Self::FLOAT, name);
        self.0.extend_from_slice(&value.to_le_bytes());
    }

    fn string(&mut self, name: &str, value: &str) {
        self.name(Self::STRING, name);
        self.var_len(value.len());
        self.0.extend_from_slice(value.as_bytes());
    }

    fn finish(mut self) -> NbtCompoundBody {
        self.0.push(0);
        NbtCompoundBody(self.0.into())
    }
}

/// Replays of the committed BDS 1.26.51 command capture
/// (`crates/bds-extractor/captures/1.26.51/operator-commands`).
#[cfg(test)]
mod bds_capture {
    use crate::command::exec::evidence::{Replay, assert_same_packets};

    #[test]
    fn music_matches_the_bds_capture() {
        let steps = Replay::new().assert_segment("music", &[]);
        assert_same_packets(&steps, &["LevelEventGenericPacket"], &[]);
    }
}
