//! Scalar Bedrock runtime math, independent of ECS and vector libraries.

const SIN: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/rotation_sine.bin"));
const SAMPLES: usize = SIN.len() / 4;
const INDEX_SCALE: f32 = SAMPLES as f32 / std::f32::consts::TAU;

#[inline]
fn table_value(index: i32) -> f32 {
    let offset = (index as usize & (SAMPLES - 1)) * 4;
    f32::from_le_bytes([
        SIN[offset],
        SIN[offset + 1],
        SIN[offset + 2],
        SIN[offset + 3],
    ])
}

/// Direction used by native player Drop, from finite yaw/pitch in degrees.
///
/// Yaw zero points along +Z; positive pitch points down. Samples invert the
/// rounded f32 index scale before sin; neither f64 sampling nor an algebraic
/// TAU multiplication preserves the captured negative-angle impulses.
/// Keep this separate from Java's table and Molang's math functions. These
/// stationary-drop captures do not calibrate other actor look-vector paths.
/// The immutable table is emitted at build time: no lazy allocation or
/// transcendental work occurs while evaluating a direction.
#[inline]
pub fn player_throw_direction(yaw: f32, pitch: f32) -> [f32; 3] {
    let yaw_index = yaw.to_radians() * INDEX_SCALE;
    let pitch_index = pitch.to_radians() * INDEX_SCALE;
    let cos_pitch = table_value((pitch_index + SAMPLES as f32 / 4.0) as i32);
    [
        -table_value(yaw_index as i32) * cos_pitch,
        -table_value(pitch_index as i32),
        table_value((yaw_index + SAMPLES as f32 / 4.0) as i32) * cos_pitch,
    ]
}
