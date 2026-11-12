//! A steady `PlayerAuthInput` stream (`--input`): one packet per client tick
//! (50 ms) at the player's last known position, as an idle client sends.
//!
//! BDS ignores some client packets from a player that sends no input (a
//! `BookEdit` changed nothing in the round 2 relay runs), so captures that
//! need a "live" player run with this stream on. The player stands still: the
//! position is the `StartGame` one until the server moves the player
//! (`MovePlayer` for its own runtime id) or corrects it
//! (`CorrectPlayerMovePrediction` for the player), and every packet repeats
//! it. BDS's `StartGame` puts a new player at a placeholder height (y 32769.62
//! in the 2026-11-12 `book` run) and corrects it onto the ground, so a stream
//! that ignored corrections would be corrected every few ticks.

use jolyne::valentine::{
    EnumsClientPlayMode, EnumsInputMode, EnumsNewInteractionModel, EnumsRewindType, McpePacket,
    McpePacketData, PlayerAuthInputPacket, PlayerInputTick, Vec2, Vec3,
};
use std::time::Duration;
use tokio::time::Instant;

/// How often a client sends `PlayerAuthInput`: once per 20 Hz tick.
pub const TICK: Duration = Duration::from_millis(50);

/// The state of the input stream.
#[derive(Debug, Clone)]
pub struct InputStream {
    runtime_id: u64,
    position: Vec3,
    rotation: Vec2,
    tick: u64,
    due: Instant,
}

impl InputStream {
    /// A stream for the player `runtime_id` standing at `position` (eye
    /// height, as `StartGame` and `MovePlayer` carry it), its first packet
    /// due at `now`.
    pub fn new(runtime_id: u64, position: Vec3, rotation: Vec2, now: Instant) -> Self {
        Self {
            runtime_id,
            position,
            rotation,
            tick: 0,
            due: now,
        }
    }

    /// When the next packet is due.
    pub fn due(&self) -> Instant {
        self.due
    }

    /// The next packet; the one after it is due a tick later.
    pub fn next_packet(&mut self) -> McpePacket {
        self.tick += 1;
        self.due += TICK;
        McpePacket::from(PlayerAuthInputPacket {
            player_rotation: self.rotation.clone(),
            player_head_rotation: self.rotation.y,
            interact_rotation: self.rotation.clone(),
            position: self.position.clone(),
            input_mode: EnumsInputMode::Mouse,
            play_mode: EnumsClientPlayMode::Normal,
            new_interaction_model: EnumsNewInteractionModel::Crosshair,
            client_tick: PlayerInputTick {
                inputtick: self.tick,
            },
            ..Default::default()
        })
    }

    /// Follows a server packet that moves or corrects the player.
    pub fn follow(&mut self, packet: &McpePacketData) {
        match packet {
            McpePacketData::MovePlayerPacket(moved)
                if moved.player_runtime_id.actor_runtime_id == self.runtime_id =>
            {
                self.position = moved.position.clone();
                self.rotation = moved.rotation.clone();
            }
            McpePacketData::CorrectPlayerMovePredictionPacket(corrected)
                if corrected.prediction_type == EnumsRewindType::Player =>
            {
                self.position = corrected.pos.clone();
                self.rotation = corrected.rotation.clone();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jolyne::valentine::{ActorRuntimeId, MovePlayerPacket};

    fn vec3(x: f32, y: f32, z: f32) -> Vec3 {
        Vec3 { x, y, z }
    }

    #[test]
    fn packets_count_ticks_and_follow_server_moves_of_the_player_only() {
        let start = Instant::now();
        let mut input = InputStream::new(7, vec3(0.5, 5.62, 0.5), Vec2::default(), start);
        let first = input.next_packet();
        assert_eq!(input.due(), start + TICK);
        let McpePacketData::PlayerAuthInputPacket(first) = first.data else {
            panic!("PlayerAuthInput expected");
        };
        assert_eq!(first.client_tick.inputtick, 1);
        assert_eq!(first.position, vec3(0.5, 5.62, 0.5));

        let moved = |runtime_id| {
            McpePacketData::MovePlayerPacket(MovePlayerPacket {
                player_runtime_id: ActorRuntimeId {
                    actor_runtime_id: runtime_id,
                },
                position: vec3(9.0, 9.0, 9.0),
                ..Default::default()
            })
        };
        input.follow(&moved(8));
        input.follow(&moved(7));
        let McpePacketData::PlayerAuthInputPacket(second) = input.next_packet().data else {
            panic!("PlayerAuthInput expected");
        };
        assert_eq!(second.client_tick.inputtick, 2);
        assert_eq!(second.position, vec3(9.0, 9.0, 9.0));

        input.follow(&McpePacketData::CorrectPlayerMovePredictionPacket(
            jolyne::valentine::CorrectPlayerMovePredictionPacket {
                prediction_type: EnumsRewindType::Player,
                pos: vec3(103.5, 137.62, -27.5),
                ..Default::default()
            },
        ));
        let McpePacketData::PlayerAuthInputPacket(third) = input.next_packet().data else {
            panic!("PlayerAuthInput expected");
        };
        assert_eq!(third.position, vec3(103.5, 137.62, -27.5));
    }
}
