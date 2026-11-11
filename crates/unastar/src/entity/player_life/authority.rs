//! The server's own copy of each player's movement, as BDS 1.26.51 keeps it.
//!
//! BDS does not take a client's word for where its player is: it simulates
//! every `PlayerAuthInput` itself (`unastar_rules::movement`, the simulation
//! the measuring sweep client runs too) and judges the position the client
//! reports against its copy (`unastar_rules::movement::authority`): close
//! positions are taken, others pull the copy towards them, and far ones,
//! or ones it cannot reach, are answered with `CorrectPlayerMovePrediction`
//! carrying the copy's state after that input. The copy is the player's
//! position for everything else: its viewers, its falls, its `MOVING` flag
//! and its exhaustion.
//!
//! A server teleport (command, plugin, ender pearl, chorus fruit) leaves
//! the copy at rest at the destination; the next input is not simulated
//! and a client still reporting its old position from far away is
//! corrected there, so no input sent before the client saw the teleport can
//! undo it. A respawn places the copy at the spawn point and simulates the
//! next input from there (a client close to the spawn point is taken where
//! it is, as on joining: a configured spawn can sit inside a block). A knockback is the copy's velocity from the next
//! input on.
//!
//! **Fail safe.** The copy only judges what the simulation covers. An input
//! is trusted instead (the copy takes the client's position and velocity,
//! as the server did before) when the player flies, is a spectator, has a
//! movement effect the simulation does not apply, or touches a block whose
//! movement it does not reproduce yet ([`MotionTable`]); a teleport is
//! still enforced then. A client must never be corrected where BDS would
//! not correct it.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::types::{
    EnumsPlayerAuthInputPacketPayloadInputData as Flag, EnumsRewindType, PlayerInputTick, Vec2,
    Vec3,
};
use jolyne::valentine::{CorrectPlayerMovePredictionPacket, PlayerAuthInputPacket};
use unastar_rules::movement::authority::{self, Authority, Verdict};
use unastar_rules::movement::{
    self as sim, Aabb, Block, BlockKind, Body, Controls, NETWORK_OFFSET, Terrain,
};

use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::{EffectType, Effects};
use crate::entity::systems::BlockSource;
use crate::entity::systems::physics::collect_block_boxes;
use crate::registry::BlockRegistry;
use crate::registry::block::LiquidKind;
use crate::registry::collision;
use crate::world::ChunkManager;
use crate::world::chunk::{MIN_Y, SUBCHUNK_COUNT};
use crate::world::ecs::{ChunkData, world_to_chunk_coords, world_to_local_coords};

/// The loaded chunks' blocks, for the movement system.
pub struct LoadedBlocks<'a, 'w, 's> {
    pub manager: &'a ChunkManager,
    pub chunks: &'a Query<'w, 's, &'static ChunkData>,
    pub registry: &'a BlockRegistry,
}

impl BlockSource for LoadedBlocks<'_, '_, '_> {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
        let chunk = self.chunks.get(self.manager.get_by_coords(cx, cz)?).ok()?;
        if !(MIN_Y..MIN_Y + i32::from(SUBCHUNK_COUNT) * 16).contains(&pos.y) {
            return Some(*crate::world::chunk::blocks::AIR);
        }
        let (x, y, z) = world_to_local_coords(pos.x, pos.y, pos.z);
        Some(chunk.inner.get_block(x, y, z))
    }

    fn registry(&self) -> &BlockRegistry {
        self.registry
    }
}

/// What a block is to the simulation, per runtime id, and whether the
/// simulation reproduces it.
#[derive(Resource)]
pub struct MotionTable {
    blocks: Vec<(Block, bool)>,
}

/// Blocks whose movement the simulation does not reproduce yet: a player
/// touching one is trusted. Climbing (ladders, vines, scaffolding), sticky
/// or slowing blocks (honey, soul sand, sweet berries), lava and bubble
/// columns have rules of their own; slime and bed bounces, powder snow and
/// cobwebs are simulated but not yet confirmed against BDS.
const UNSUPPORTED: &[&str] = &[
    "minecraft:ladder",
    "minecraft:vine",
    "minecraft:twisting_vines",
    "minecraft:weeping_vines",
    "minecraft:cave_vines",
    "minecraft:cave_vines_body_with_berries",
    "minecraft:cave_vines_head_with_berries",
    "minecraft:scaffolding",
    "minecraft:honey_block",
    "minecraft:soul_sand",
    "minecraft:sweet_berry_bush",
    "minecraft:bubble_column",
    "minecraft:slime",
    "minecraft:bed",
    "minecraft:powder_snow",
    "minecraft:web",
];

impl MotionTable {
    pub fn new(registry: &BlockRegistry) -> Self {
        let mut blocks = Vec::new();
        for entry in registry.iter() {
            let kind = match entry.string_id.as_str() {
                "minecraft:air" => BlockKind::Air,
                "minecraft:slime" => BlockKind::Slime,
                "minecraft:bed" => BlockKind::Bed,
                "minecraft:web" => BlockKind::Cobweb,
                "minecraft:powder_snow" => BlockKind::PowderSnow,
                _ => BlockKind::Plain,
            };
            for id in entry.min_state_id..=entry.max_state_id {
                let index = id as usize;
                if blocks.len() <= index {
                    blocks.resize(index + 1, (Block::AIR, true));
                }
                let liquid = registry.liquid_state(id).map(|liquid| liquid.kind);
                let supported = !UNSUPPORTED.contains(&entry.string_id.as_str())
                    && liquid != Some(LiquidKind::Lava);
                blocks[index] = (
                    Block {
                        friction: entry.data.friction,
                        kind,
                        water: liquid == Some(LiquidKind::Water),
                    },
                    supported,
                );
            }
        }
        Self { blocks }
    }

    fn get(&self, runtime_id: u32) -> (Block, bool) {
        self.blocks
            .get(runtime_id as usize)
            .copied()
            .unwrap_or((Block::AIR, false))
    }
}

/// The world as the simulation reads it: the loaded chunks' blocks.
pub struct WorldTerrain<'a, B: BlockSource> {
    pub blocks: &'a B,
    pub table: &'a MotionTable,
}

impl<B: BlockSource> WorldTerrain<'_, B> {
    /// Whether every block around `area` is one the simulation reproduces.
    fn supported(&self, area: &Aabb) -> bool {
        let (min, max) = area.block_range();
        (min[0]..=max[0]).all(|x| {
            (min[1] - 1..=max[1]).all(|y| {
                (min[2]..=max[2]).all(|z| {
                    self.blocks
                        .block_at(IVec3::new(x, y, z))
                        .is_none_or(|id| self.table.get(id).1)
                })
            })
        })
    }
}

impl<B: BlockSource> Terrain for WorldTerrain<'_, B> {
    fn collisions(&self, area: &Aabb, out: &mut Vec<Aabb>) {
        let area = collision::Aabb::new(
            DVec3::new(
                f64::from(area.min[0]),
                f64::from(area.min[1]),
                f64::from(area.min[2]),
            ),
            DVec3::new(
                f64::from(area.max[0]),
                f64::from(area.max[1]),
                f64::from(area.max[2]),
            ),
        );
        let mut boxes = Vec::new();
        collect_block_boxes(self.blocks, area, &mut boxes);
        out.extend(boxes.into_iter().map(|b| Aabb {
            min: [b.min.x as f32, b.min.y as f32, b.min.z as f32],
            max: [b.max.x as f32, b.max.y as f32, b.max.z as f32],
        }));
    }

    fn loaded(&self, area: &Aabb) -> bool {
        // Chunks load whole columns.
        let (min, max) = area.block_range();
        (min[0]..=max[0]).all(|x| {
            (min[2]..=max[2]).all(|z| self.blocks.block_at(IVec3::new(x, min[1], z)).is_some())
        })
    }

    fn block(&self, pos: [i32; 3]) -> Block {
        self.blocks
            .block_at(IVec3::from_array(pos))
            .map_or(Block::AIR, |id| self.table.get(id).0)
    }
}

/// Why an input is trusted instead of simulated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// The simulation judges the input.
    No,
    /// Flying or a spectator: flight is not simulated.
    Flight,
    /// A movement effect the simulation does not apply.
    Effect,
    /// A block the simulation does not reproduce is near.
    Block,
    /// The first input after joining or respawning, close to the spawn
    /// point: the copy starts where the client does.
    Spawn,
}

/// How far from the spawn point the first input may put the copy.
const SPAWN_SETTLE: f32 = 2.0;

/// A player's movement effects as the simulation reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MovementEffects {
    /// Jump boost level (amplifier + 1), 0 without.
    pub jump_boost: u8,
    pub slow_falling: bool,
    /// Speed, slowness or levitation: not simulated yet.
    pub unsupported: bool,
}

/// A player's movement effects as the simulation reads them (the one place
/// the authority reads the effect state).
pub fn movement_effects_of(effects: Option<&Effects>) -> MovementEffects {
    let Some(effects) = effects else {
        return MovementEffects::default();
    };
    MovementEffects {
        // Levels are amplifiers: 0 is level I.
        jump_boost: effects
            .get(EffectType::JUMP_BOOST)
            .map_or(0, |effect| effect.level.saturating_add(1)),
        slow_falling: effects.has(EffectType::SLOW_FALLING),
        unsupported: [
            EffectType::SPEED,
            EffectType::SLOWNESS,
            EffectType::LEVITATION,
        ]
        .into_iter()
        .any(|effect| effects.has(effect)),
    }
}

/// The server's copy of a player's movement.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct PlayerMotion {
    pub body: Body,
    pub authority: Authority,
    /// A velocity the server gave the player (a knockback): the copy's
    /// velocity from the next input on.
    pub push: Option<[f32; 3]>,
    /// The movement effects the next inputs use.
    pub effects: MovementEffects,
    /// Corrections sent.
    pub corrections: u32,
    /// How the last input was taken: its verdict (none when trusted) and
    /// why it was trusted.
    pub last: (Option<Verdict>, Trust),
    /// No input since joining or respawning yet.
    settling: bool,
}

/// What one input did to the copy.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    /// The eye position after the input.
    pub position: DVec3,
    /// The copy's ground contact after it.
    pub on_ground: bool,
    /// The copy jumped.
    pub jumped: bool,
    /// The correction to send.
    pub correction: Option<CorrectPlayerMovePredictionPacket>,
    pub verdict: Option<Verdict>,
    pub trust: Trust,
}

impl Outcome {
    /// An input of a player without a copy (no world to simulate in): the
    /// reported position and ground contact, trusted.
    pub fn reported(input: &PlayerAuthInputPacket) -> Self {
        let has = |flag| input.input_data.contains(&flag);
        Self {
            position: DVec3::new(
                f64::from(input.position.x),
                f64::from(input.position.y),
                f64::from(input.position.z),
            ),
            on_ground: has(Flag::Verticalcollision),
            jumped: has(Flag::Startjumping),
            correction: None,
            verdict: None,
            trust: Trust::Flight,
        }
    }
}

/// The server's `Position` (the eyes, [`PLAYER_EYE_HEIGHT`] above the feet)
/// of feet `feet`.
fn eyes(feet: [f32; 3]) -> DVec3 {
    DVec3::new(
        f64::from(feet[0]),
        f64::from(feet[1]) + PLAYER_EYE_HEIGHT,
        f64::from(feet[2]),
    )
}

/// The feet of a server `Position`.
fn feet_of(eye: DVec3) -> [f32; 3] {
    [
        eye.x as f32,
        (eye.y - PLAYER_EYE_HEIGHT) as f32,
        eye.z as f32,
    ]
}

/// The feet of a position a client reports: its wire position, 1.62001
/// above the feet in every pose.
fn feet_of_reported(position: &Vec3) -> [f32; 3] {
    [position.x, position.y - NETWORK_OFFSET, position.z]
}

/// The controls an input reports: move vector as pressed (x strafe, y
/// forward), rotation, and the jump, sneak and sprint keys.
pub fn controls(input: &PlayerAuthInputPacket) -> Controls {
    let has = |flag| input.input_data.contains(&flag);
    Controls {
        forward: input.move_vector.y,
        strafe: input.move_vector.x,
        yaw: input.player_rotation.y,
        pitch: input.player_rotation.x,
        jump: has(Flag::Jumping),
        sneak: has(Flag::Sneaking),
        sprint: has(Flag::Sprintdown),
    }
}

impl PlayerMotion {
    /// A player that joined at `eye`, at rest.
    pub fn spawned(eye: DVec3) -> Self {
        let mut body = Body::at(feet_of(eye));
        body.on_ground = true;
        Self {
            body,
            authority: Authority::default(),
            push: None,
            effects: MovementEffects::default(),
            corrections: 0,
            last: (None, Trust::No),
            settling: true,
        }
    }

    /// The server moved the player to `eye` (a teleport): the next input
    /// rests there and is judged against it.
    pub fn teleported(&mut self, eye: DVec3) {
        self.place(eye);
        self.settling = false;
        self.authority.teleported();
    }

    /// The player is put at `eye` at rest without a teleport: the next input
    /// is simulated from there.
    pub fn placed(&mut self, eye: DVec3) {
        self.place(eye);
        self.settling = false;
    }

    /// The player respawned at `eye`: placed there at rest without a
    /// teleport, the next input simulated from there as BDS does, unless
    /// the client is within [`SPAWN_SETTLE`] of it (as after joining).
    pub fn respawned(&mut self, eye: DVec3) {
        self.place(eye);
        self.settling = true;
    }

    fn place(&mut self, eye: DVec3) {
        self.body.feet = feet_of(eye);
        self.body.velocity = [0.0; 3];
        self.body.on_ground = true;
        self.body.gliding = false;
        self.body.swimming = false;
        self.push = None;
    }

    /// The server pushed the player (a knockback).
    pub fn pushed(&mut self, velocity: DVec3) {
        self.push = Some([velocity.x as f32, velocity.y as f32, velocity.z as f32]);
    }

    /// One input. `trust` says whether the simulation may judge it, from
    /// the player's state; `glide` whether a glide the input starts is
    /// allowed (an elytra worn). The blocks around the copy can still
    /// make it trusted.
    pub fn input(
        &mut self,
        input: &PlayerAuthInputPacket,
        terrain: &WorldTerrain<'_, impl BlockSource>,
        trust: Trust,
        glide: bool,
    ) -> Outcome {
        let outcome = self.judge_input(input, terrain, trust, glide);
        self.last = (outcome.verdict, outcome.trust);
        outcome
    }

    fn judge_input(
        &mut self,
        input: &PlayerAuthInputPacket,
        terrain: &WorldTerrain<'_, impl BlockSource>,
        trust: Trust,
        glide: bool,
    ) -> Outcome {
        let tick = input.client_tick.inputtick;
        let claim = feet_of_reported(&input.position);
        if std::mem::take(&mut self.settling) {
            // The spawn point is the server's choice (a configured location
            // can sit inside a block); a client that joined or respawned
            // there starts the copy where it is. Farther off, the input is
            // simulated from the spawn and judged, as BDS judges the first
            // input after a respawn.
            let d = [0, 1, 2].map(|axis| claim[axis] - self.body.feet[axis]);
            if d.iter().map(|v| v * v).sum::<f32>() <= SPAWN_SETTLE * SPAWN_SETTLE {
                return self.trusted(input, claim, Trust::Spawn);
            }
        }
        if self.authority.resting() {
            // The first input after a teleport: the copy stays at rest at
            // the destination, whatever the input says.
            let verdict = self.authority.judge_rest(self.body.feet, claim);
            let correction = (verdict == Verdict::Correct).then(|| {
                self.corrections += 1;
                correction_packet(&self.body, [0.0; 3], tick)
            });
            return Outcome {
                position: eyes(self.body.feet),
                on_ground: true,
                jumped: false,
                correction,
                verdict: Some(verdict),
                trust: Trust::No,
            };
        }
        let controls = controls(input);
        let search = self
            .body
            .aabb()
            .expand(self.body.velocity)
            .inflate([1.0, 1.0, 1.0]);
        let trust = match trust {
            Trust::No if self.effects.unsupported => Trust::Effect,
            Trust::No if !terrain.supported(&search) => Trust::Block,
            trust => trust,
        };
        if trust != Trust::No {
            return self.trusted(input, claim, trust);
        }
        if let Some(push) = self.push.take() {
            self.body.velocity = push;
        }
        let pull = self.authority.take_pull();
        for (velocity, pull) in self.body.velocity.iter_mut().zip(pull) {
            *velocity += pull;
        }
        self.body.jump_boost = self.effects.jump_boost;
        self.body.slow_falling = self.effects.slow_falling;
        if glide
            && input.input_data.contains(&Flag::Startgliding)
            && !self.body.on_ground
            && !self.body.gliding
            && !self.body.in_water
        {
            self.body.gliding = true;
        }
        let before = self.body.feet;
        let changes = sim::step(&mut self.body, &controls, terrain);
        let moved = self.body.feet[0] != before[0] || self.body.feet[2] != before[2];
        let free = !authority::inside_blocks(terrain, claim, self.body.pose().height());
        let verdict = self.authority.judge(&mut self.body, claim, moved, free);
        let correction = (verdict == Verdict::Correct).then(|| {
            self.corrections += 1;
            correction_packet(&self.body, self.body.velocity, tick)
        });
        Outcome {
            position: eyes(self.body.feet),
            on_ground: self.body.on_ground,
            jumped: changes.jumped,
            correction,
            verdict: Some(verdict),
            trust: Trust::No,
        }
    }

    /// Take the client's position and velocity (the fail-safe).
    fn trusted(&mut self, input: &PlayerAuthInputPacket, claim: [f32; 3], trust: Trust) -> Outcome {
        let has = |flag| input.input_data.contains(&flag);
        self.body.feet = claim;
        self.body.velocity = [input.pos_delta.x, input.pos_delta.y, input.pos_delta.z];
        // The landing a trusted client reports, as the server took it before
        // it simulated movement.
        self.body.on_ground = has(Flag::Verticalcollision);
        self.body.sneaking = has(Flag::Sneaking);
        self.body.sprinting = has(Flag::Sprinting);
        self.push = None;
        Outcome {
            position: eyes(claim),
            on_ground: self.body.on_ground,
            jumped: has(Flag::Startjumping),
            correction: None,
            verdict: None,
            trust,
        }
    }
}

/// BDS's correction: the copy's eye position, its velocity after the input
/// and its ground contact, for input `tick`.
fn correction_packet(
    body: &Body,
    velocity: [f32; 3],
    tick: u64,
) -> CorrectPlayerMovePredictionPacket {
    let [x, y, z] = body.network_position();
    CorrectPlayerMovePredictionPacket {
        prediction_type: EnumsRewindType::Player,
        pos: Vec3 { x, y, z },
        pos_delta: Vec3 {
            x: velocity[0],
            y: velocity[1],
            z: velocity[2],
        },
        rotation: Vec2 { x: 0.0, y: 0.0 },
        vehicle_angular_velocity: None,
        on_ground: body.on_ground,
        tick: PlayerInputTick { inputtick: tick },
    }
}
