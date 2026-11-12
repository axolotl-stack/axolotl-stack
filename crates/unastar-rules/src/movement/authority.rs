//! What BDS 1.26.51 does with the position a client reports once it has
//! simulated the client's input itself (`player-position-acceptance-threshold`
//! 0.5, the default; measured with the scripted sweep client, which reported
//! positions offset from its own simulation, drifting, ignoring corrections
//! and teleports, and a second client watching BDS's copy of it):
//!
//! - **Far off:** a reported position whose squared distance from BDS's copy
//!   exceeds the threshold (0.5, so about 0.71 blocks) is corrected:
//!   `CorrectPlayerMovePrediction` with BDS's state after that input. At most
//!   one correction every [`COOLDOWN`] inputs; the copy is never moved
//!   towards such a position.
//! - **Close:** when every axis is within [`ACCEPT`] of the copy (and the
//!   distance is more than [`TINY`]), the copy takes the client's position,
//!   keeping its own velocity. Not into a block, not right after such a take
//!   while the copy moves horizontally, and not after a teleport until a
//!   correction was sent.
//! - **In between, and tiny differences:** the copy is pulled towards the
//!   client: the difference, at most [`PULL`] long, is added to its velocity
//!   before the next input's move. A disagreement of more than [`TINY`]
//!   counts; the eleventh in a row (ten pulls that did not bring the copy
//!   close enough) is corrected.
//! - After any correction the next [`QUIET`] inputs pull nothing.
//!
//! A teleport ([`Authority::teleported`]) is not acknowledged by any input
//! flag: BDS keeps the player at rest at the destination for the next input
//! without simulating it and judges that input's position against the
//! destination ([`Authority::judge_rest`]).
//!
//! Positions here are the feet; every pose puts the reported position the
//! same height above them, so the distances are those of the wire.

use super::{Aabb, Body, Terrain};

/// Each axis this close to the copy lets the copy take the client's
/// position.
pub const ACCEPT: f32 = 0.1;
/// Differences up to this are only pulled and never counted.
pub const TINY: f32 = 0.005;
/// The longest pull per input.
pub const PULL: f32 = 0.005;
/// The disagreement that is corrected when the copy is still not close
/// (counting the first one, so ten pulls).
pub const CORRECT_AFTER: u8 = 11;
/// Inputs after a correction that pull nothing.
pub const QUIET: u8 = 6;
/// Inputs between two threshold corrections.
pub const COOLDOWN: u8 = 6;

/// BDS's default `player-position-acceptance-threshold`, compared with the
/// squared distance.
pub const DEFAULT_THRESHOLD: f32 = 0.5;

/// How an input's reported position was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The client agrees with the copy.
    InSync,
    /// The copy took the client's position.
    Accepted,
    /// The copy is pulled towards the client from the next input on.
    Pulled,
    /// Send a correction with the copy's state.
    Correct,
    /// Too far, but a correction was sent less than [`COOLDOWN`] inputs ago.
    Rejected,
}

/// The per-player state of the judgement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Authority {
    /// `player-position-acceptance-threshold`.
    pub threshold: f32,
    pull: [f32; 3],
    count: u8,
    quiet: u8,
    cooldown: u8,
    accepted: bool,
    distrust: bool,
    rest: bool,
}

impl Default for Authority {
    fn default() -> Self {
        Self::new(DEFAULT_THRESHOLD)
    }
}

impl Authority {
    pub fn new(threshold: f32) -> Self {
        Self {
            threshold,
            pull: [0.0; 3],
            count: 0,
            quiet: 0,
            cooldown: 0,
            accepted: false,
            distrust: false,
            rest: false,
        }
    }

    /// The server moved the player (a teleport): the next input rests at
    /// the destination, and the client's position is not taken until a
    /// correction was sent.
    pub fn teleported(&mut self) {
        self.pull = [0.0; 3];
        self.accepted = false;
        self.distrust = true;
        self.rest = true;
    }

    /// Whether the next input rests (the first after a teleport).
    pub fn resting(&self) -> bool {
        self.rest
    }

    /// Whether the client's position may be taken: not after a teleport
    /// until a correction was sent.
    pub fn trusting(&self) -> bool {
        !self.distrust
    }

    /// An input that was not judged: the copy took the client's state, so
    /// nothing is left to pull and no disagreement is counted. The inputs
    /// still count towards the quiet and cooldown periods.
    pub fn synced(&mut self) {
        self.pull = [0.0; 3];
        self.count = 0;
        self.accepted = false;
        self.quiet = self.quiet.saturating_sub(1);
        self.cooldown = self.cooldown.saturating_sub(1);
    }

    /// The pull to add to the copy's velocity before this input's move;
    /// nothing during the inputs after a correction.
    pub fn take_pull(&mut self) -> [f32; 3] {
        let pull = std::mem::take(&mut self.pull);
        if self.quiet > 0 {
            self.quiet -= 1;
            return [0.0; 3];
        }
        pull
    }

    /// The first input after a teleport: the copy stays at the destination
    /// (`copy`); the reported position `claim` is judged against it.
    pub fn judge_rest(&mut self, copy: [f32; 3], claim: [f32; 3]) -> Verdict {
        self.rest = false;
        self.cooldown = self.cooldown.saturating_sub(1);
        let d = sub(claim, copy);
        if length_sq(d) > self.threshold {
            return self.correct();
        }
        if length_sq(d) > 0.0 {
            self.schedule(d);
            if length(d) > TINY {
                self.count += 1;
            }
            return Verdict::Pulled;
        }
        self.count = 0;
        Verdict::InSync
    }

    /// Judge `claim` (the client's feet) against `copy`, BDS's body after
    /// simulating the input; `moved` is whether the copy moved horizontally
    /// this input, `free` whether the client's box at `claim` is clear of
    /// blocks. An accepted position is written to `copy`, keeping its
    /// velocity.
    pub fn judge(&mut self, copy: &mut Body, claim: [f32; 3], moved: bool, free: bool) -> Verdict {
        self.cooldown = self.cooldown.saturating_sub(1);
        let d = sub(claim, copy.feet);
        let distance_sq = length_sq(d);
        if distance_sq > self.threshold {
            self.count = 0;
            self.accepted = false;
            if self.cooldown == 0 {
                return self.correct();
            }
            return Verdict::Rejected;
        }
        if distance_sq == 0.0 {
            self.count = 0;
            self.accepted = false;
            return Verdict::InSync;
        }
        let distance = distance_sq.sqrt();
        let close = d.iter().all(|axis| axis.abs() < ACCEPT) && distance > TINY && free;
        let take = close && !self.distrust && !(self.accepted && moved);
        self.accepted = take;
        if take {
            copy.feet = claim;
            self.count = 0;
            return Verdict::Accepted;
        }
        self.schedule(d);
        if distance > TINY && self.quiet == 0 {
            self.count += 1;
            if self.count >= CORRECT_AFTER {
                return self.correct();
            }
        }
        Verdict::Pulled
    }

    fn schedule(&mut self, d: [f32; 3]) {
        let length = length(d);
        if length <= 0.0 {
            return;
        }
        let scale = length.min(PULL) / length;
        self.pull = d.map(|v| v * scale);
    }

    fn correct(&mut self) -> Verdict {
        self.count = 0;
        self.quiet = QUIET;
        self.cooldown = COOLDOWN;
        self.pull = [0.0; 3];
        self.accepted = false;
        self.distrust = false;
        Verdict::Correct
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn length_sq(d: [f32; 3]) -> f32 {
    d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
}

fn length(d: [f32; 3]) -> f32 {
    length_sq(d).sqrt()
}

/// Whether a client standing at `feet` in a box `height` tall would be
/// inside a block (a reported position BDS never takes).
pub fn inside_blocks(terrain: &impl Terrain, feet: [f32; 3], height: f32) -> bool {
    let bb = Aabb::from_feet(feet, super::WIDTH, height).inflate([-1.0e-5; 3]);
    super::collides(terrain, &bb)
}
