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
//! it is, as on joining: a configured spawn can sit inside a block). A
//! knockback is the copy's velocity from the next input on.
//!
//! **Fail safe.** The copy only judges what the simulation covers and BDS
//! runs of the sweep client confirmed; any other input is trusted, the copy
//! taking the client's position and velocity ([`Trust`]). How far a trusted
//! input may move the copy depends on who brings the trust about: the
//! server grants flight and effects (up to [`MAX_TRUSTED_MOVE`], beyond it
//! the input is rejected whole), while the blocks, gear, crawl spaces and
//! riptide spins a client can seek out only cover about one tick of vanilla
//! movement (beyond it the copy judges the input). A client that is only
//! slower than the copy's own simulation of an input (an item in use, a
//! sprint its food level forbids: slowdowns the simulation does not model)
//! is taken too, except while a knockback must play out. A teleport is
//! enforced whatever the trust. A client must never be corrected where BDS
//! would not correct it. The movement differential (`tools/movement-diff`)
//! decides what the simulation covers: what it matches BDS on bit for bit is
//! judged.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::types::{
    EnumsPlayerAuthInputPacketPayloadInputData as Flag, EnumsRewindType, PlayerInputTick, Vec2,
    Vec3,
};
use jolyne::valentine::{CorrectPlayerMovePredictionPacket, PlayerAuthInputPacket};
use unastar_rules::movement::authority::{self, Authority, TINY, Verdict};
use unastar_rules::movement::{
    self as sim, Aabb, Block, BlockKind, Body, Controls, JUMP_DELAY, NETWORK_OFFSET, Pose, Terrain,
    WIDTH, walk_speed,
};

use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::{ArmourInventory, EffectType, Effects};
use crate::entity::systems::physics::collect_block_boxes;
use crate::entity::systems::{BlockSource, LayeredBlocks, chunk_layer};
use crate::item::ItemStack;
use crate::item::enchant::{Enchantment, enchantment_level};
use crate::registry::BlockRegistry;
use crate::registry::block::LiquidKind;
use crate::registry::collision;
use crate::world::ChunkManager;
use crate::world::ecs::{ChunkData, world_to_chunk_coords};

/// The loaded chunks' blocks, for the movement system.
pub struct LoadedBlocks<'a, 'w, 's> {
    pub manager: &'a ChunkManager,
    pub chunks: &'a Query<'w, 's, &'static ChunkData>,
    pub registry: &'a BlockRegistry,
}

impl LoadedBlocks<'_, '_, '_> {
    /// The loaded chunk holding `pos`.
    fn chunk(&self, pos: IVec3) -> Option<&ChunkData> {
        let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
        self.chunks.get(self.manager.get_by_coords(cx, cz)?).ok()
    }
}

impl BlockSource for LoadedBlocks<'_, '_, '_> {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        Some(chunk_layer(self.chunk(pos)?, pos, 0))
    }

    fn registry(&self) -> &BlockRegistry {
        self.registry
    }
}

impl LayeredBlocks for LoadedBlocks<'_, '_, '_> {
    fn second_layer_at(&self, pos: IVec3) -> Option<u32> {
        Some(chunk_layer(self.chunk(pos)?, pos, 1))
    }
}

/// What a block is to the simulation, per runtime id, and whether the
/// simulation reproduces it.
#[derive(Resource)]
pub struct MotionTable {
    blocks: Vec<(Block, bool)>,
}

/// Blocks with movement rules of their own the simulation does not
/// reproduce yet: a player near one is trusted. The climbable plants but
/// ladders and vines, scaffolding (climbing it is not yet BDS's), sweet
/// berries and bubble columns. Families the simulation has but no BDS run
/// has confirmed ([`BlockKind::confirmed`]: beds, powder snow), lava and
/// flowing water (whose current pushes) are not supported either.
/// Ladders, vines, honey, soul sand, slime, cobwebs and ice are: the
/// movement differential (`tools/movement-diff`) matches BDS on them bit for
/// bit.
const UNSUPPORTED: &[&str] = &[
    "minecraft:twisting_vines",
    "minecraft:weeping_vines",
    "minecraft:cave_vines",
    "minecraft:cave_vines_body_with_berries",
    "minecraft:cave_vines_head_with_berries",
    "minecraft:scaffolding",
    "minecraft:sweet_berry_bush",
    "minecraft:bubble_column",
];

impl MotionTable {
    pub fn new(registry: &BlockRegistry) -> Self {
        let mut blocks = Vec::new();
        for entry in registry.iter() {
            let kind = BlockKind::of(&entry.string_id);
            for id in entry.min_state_id..=entry.max_state_id {
                let index = id as usize;
                if blocks.len() <= index {
                    // Ids the registry does not name are not supported.
                    blocks.resize(index + 1, (Block::AIR, false));
                }
                let block = unastar_data::motion::block(id);
                // Still water only: a current pushes, and lava has rules of
                // its own.
                let liquid_supported = block
                    .liquid
                    .is_none_or(|liquid| liquid.kind == LiquidKind::Water && liquid.depth == 0);
                let supported = kind.confirmed()
                    && !UNSUPPORTED.contains(&entry.string_id.as_str())
                    && liquid_supported;
                blocks[index] = (block, supported);
            }
        }
        Self { blocks }
    }

    /// Whether the simulation reproduces the block `runtime_id`.
    pub fn reproduces(&self, runtime_id: u32) -> bool {
        self.get(runtime_id).1
    }

    fn get(&self, runtime_id: u32) -> (Block, bool) {
        self.blocks
            .get(runtime_id as usize)
            .copied()
            .unwrap_or((Block::AIR, false))
    }
}

/// The world as the simulation reads it: the loaded chunks' blocks, both
/// layers.
pub struct WorldTerrain<'a, B: LayeredBlocks> {
    pub blocks: &'a B,
    pub table: &'a MotionTable,
}

impl<B: LayeredBlocks> WorldTerrain<'_, B> {
    /// Whether every block around `area` (and the layer under it), in both
    /// layers, is one the simulation reproduces.
    fn supported(&self, area: &Aabb) -> bool {
        self.every_block(area, 1, |pos| {
            [self.blocks.block_at(pos), self.blocks.second_layer_at(pos)]
                .into_iter()
                .flatten()
                .all(|id| self.table.get(id).1)
        })
    }

    /// Whether water is in or around `area`.
    fn water_near(&self, area: &Aabb) -> bool {
        !self.every_block(area, 0, |pos| !self.block(pos.to_array()).water())
    }

    /// Whether a block whose family passes `test` is in or around `area`
    /// (and the layer under it).
    fn kind_near(&self, area: &Aabb, test: impl Fn(BlockKind) -> bool) -> bool {
        !self.every_block(area, 1, |pos| !test(self.block(pos.to_array()).kind))
    }

    /// Whether `test` holds for every block position of `area`, reaching
    /// `below` layers under it.
    fn every_block(&self, area: &Aabb, below: i32, test: impl Fn(IVec3) -> bool) -> bool {
        let (min, max) = area.block_range();
        (min[0]..=max[0]).all(|x| {
            (min[1] - below..=max[1]).all(|y| (min[2]..=max[2]).all(|z| test(IVec3::new(x, y, z))))
        })
    }

    /// Whether a player box `height` tall moved from `from` to `to` passes
    /// through no block the simulation reproduces (the blocks it does not,
    /// such as ladders, scaffolding and cobwebs, a client moves through as
    /// vanilla) and no unloaded one.
    fn clear_path(&self, from: [f32; 3], to: [f32; 3], height: f32) -> bool {
        let swept = collision_box(
            &Aabb::from_feet(from, WIDTH, height)
                .expand(offset(from, to))
                .inflate([-1.0e-5; 3]),
        );
        let mut boxes = Vec::new();
        collect_block_boxes(&Reproduced(self), swept, &mut boxes);
        !boxes.iter().any(|b| b.intersects(&swept))
    }

    /// Whether a player at `feet` crawls: a block is in its standing box but
    /// not in its low one (BDS 1.26.51: stone, glass and oak leaves at eye
    /// height). `None` while the blocks there are not loaded.
    pub fn crawl_space(&self, feet: [f32; 3]) -> Option<bool> {
        let standing = Aabb::from_feet(feet, WIDTH, Pose::Standing.height());
        if !self.loaded(&standing) {
            return None;
        }
        Some(
            authority::inside_blocks(self, feet, Pose::Standing.height())
                && !authority::inside_blocks(self, feet, Pose::Low.height()),
        )
    }
}

/// The blocks of a terrain the simulation reproduces; the others read as
/// air.
struct Reproduced<'t, 'a, B: LayeredBlocks>(&'t WorldTerrain<'a, B>);

impl<B: LayeredBlocks> BlockSource for Reproduced<'_, '_, B> {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        let id = self.0.blocks.block_at(pos)?;
        Some(if self.0.table.get(id).1 {
            id
        } else {
            *crate::world::chunk::blocks::AIR
        })
    }

    fn registry(&self) -> &BlockRegistry {
        self.0.blocks.registry()
    }
}

/// The server's collision box of the simulation's `area`.
fn collision_box(area: &Aabb) -> collision::Aabb {
    collision::Aabb::new(
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
    )
}

impl<B: LayeredBlocks> Terrain for WorldTerrain<'_, B> {
    fn collisions(&self, area: &Aabb, out: &mut Vec<Aabb>) {
        let area = collision_box(area);
        let mut boxes = Vec::new();
        collect_block_boxes(self.blocks, area, &mut boxes);
        // Blocks that are not loaded add nothing (the simulation does not
        // move a body whose surroundings are not loaded).
        boxes.retain(|b| self.blocks.block_at(b.min.floor().as_ivec3()).is_some());
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
        let pos = IVec3::from_array(pos);
        let Some(id) = self.blocks.block_at(pos) else {
            return Block::AIR;
        };
        let mut block = self.table.get(id).0;
        // A waterlogged block's water is in the second layer.
        if block.liquid.is_none() {
            block.liquid = self
                .blocks
                .second_layer_at(pos)
                .and_then(|id| self.table.get(id).0.liquid);
        }
        block
    }
}

/// Why an input was not judged: the copy took the client's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// Flying in a game mode that can fly, or a spectator: flight is not
    /// simulated.
    Flight,
    /// Slowness or levitation (see [`MovementEffects::unsupported`]).
    Effect,
    /// A riptide trident's spin, started in water, launches the player.
    Riptide,
    /// A block the simulation does not reproduce is near.
    Block,
    /// A movement enchantment where it acts: depth strider near water,
    /// swift sneak sneaking, soul speed near soul sand or soil.
    Gear,
    /// The copy is in a space only a crawling player fits.
    Crawl,
    /// Only slower than the copy's own simulation of the input, which does
    /// not model every slowdown (an item in use, a sprint the food level
    /// forbids): no farther, level with it, so never an advantage.
    Slowed,
    /// The first input after joining or respawning, close to the spawn
    /// point: the copy starts where the client does.
    Spawn,
    /// The player has no copy (no world to simulate in).
    Unsimulated,
}

/// How far a trusted input may move the copy.
enum Reach {
    /// Up to [`MAX_TRUSTED_MOVE`]; beyond it the input is rejected whole.
    Far,
    /// Each axis within these bounds; beyond them the copy judges the input.
    Near([f32; 3]),
}

impl Trust {
    /// How far the client may be from the copy for this trust to hold: the
    /// server grants flight and effects, while a client can seek out the
    /// rest, which only cover about one tick of vanilla movement.
    fn reach(self) -> Reach {
        match self {
            Trust::Riptide => Reach::Near(RIPTIDE_REACH),
            Trust::Block | Trust::Gear | Trust::Crawl => Reach::Near(NEAR_REACH),
            // Slowed, Spawn and Unsimulated are bounded where they are
            // decided.
            Trust::Flight | Trust::Effect | Trust::Slowed | Trust::Spawn | Trust::Unsimulated => {
                Reach::Far
            }
        }
    }
}

/// How the copy took an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Judgement {
    /// Simulated and judged.
    Judged(Verdict),
    /// The client's state was taken.
    Trusted(Trust),
    /// Trusted, but farther from the copy than the trust reaches: rejected
    /// whole.
    Refused(Trust),
}

/// How far from the spawn point the first input may put the copy.
const SPAWN_SETTLE: f32 = 2.0;

/// The farthest a trust the server grants (flight, effects) may move the
/// copy: farther, the input is rejected whole and the copy stays.
pub const MAX_TRUSTED_MOVE: f32 = 64.0;

/// The per-axis bounds (x, y, z) of a trust a client can seek out (blocks,
/// gear, crawl spaces): about one tick of the fastest vanilla movement
/// there, a fall at the terminal 3.92 a tick or a sprint-jump on ice. Not a
/// BDS measurement: the limit of what this server takes unjudged.
const NEAR_REACH: [f32; 3] = [1.5, 4.0, 1.5];

/// The same for a riptide spin, which launches the player.
const RIPTIDE_REACH: [f32; 3] = [4.0, 4.0, 4.0];

/// The inputs a riptide spin is trusted for at most.
const RIPTIDE_INPUTS: u8 = 20;

/// The fastest velocity, per axis, a trusted input gives the copy (falling
/// tops out at 3.92 a tick).
const MAX_TRUSTED_SPEED: f32 = 4.0;

/// The inputs after a knockback during which a client slower than the
/// copy is judged, not taken: the knockback must play out as BDS's.
const PUSH_SETTLE: u8 = 20;

/// A player's movement effects as the simulation reads them: levels are
/// amplifier + 1, 0 without the effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MovementEffects {
    pub jump_boost: u8,
    pub slow_falling: bool,
    pub speed: u8,
    pub slowness: u8,
    pub levitation: u8,
}

impl MovementEffects {
    /// Whether an input is trusted under these effects. Speed, jump boost
    /// and slow falling are simulated as BDS moves them. Levitation is
    /// simulated too, but a levitating player's first ground contact is
    /// not yet BDS's. Slowness is simulated as BDS moves it, but this
    /// server does not send the movement attribute with the effect's
    /// modifier, so whether a vanilla client slows down is not known: a
    /// copy slowed where the client is not would correct it. (A client that
    /// does not speed up is only slower than the copy, and taken.)
    pub fn unsupported(&self) -> bool {
        self.slowness > 0 || self.levitation > 0
    }
}

/// Movement enchantments a player wears, and a riptide trident in hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MovementGear {
    /// Depth strider boots: faster in water.
    pub depth_strider: bool,
    /// Soul speed boots: faster on soul sand and soil.
    pub soul_speed: bool,
    /// Swift sneak leggings: faster sneaking.
    pub swift_sneak: bool,
    /// A riptide trident held: its spin launches the player.
    pub riptide: bool,
}

/// What the server knows about a player besides its movement, for one
/// input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Situation {
    /// Flying in a game mode that can fly, or a spectator.
    pub flight: bool,
    /// An elytra is worn: the input may start a glide.
    pub glide: bool,
    pub effects: MovementEffects,
    pub gear: MovementGear,
}

/// A player's movement effects as the simulation reads them (the one place
/// the authority reads the effect state).
pub fn movement_effects_of(effects: Option<&Effects>) -> MovementEffects {
    let Some(effects) = effects else {
        return MovementEffects::default();
    };
    // Levels are amplifiers: 0 is level I.
    let level = |effect| {
        effects
            .get(effect)
            .map_or(0, |effect| effect.level.saturating_add(1))
    };
    MovementEffects {
        jump_boost: level(EffectType::JUMP_BOOST),
        slow_falling: effects.has(EffectType::SLOW_FALLING),
        speed: level(EffectType::SPEED),
        slowness: level(EffectType::SLOWNESS),
        levitation: level(EffectType::LEVITATION),
    }
}

/// The movement enchantments in `armour` and a riptide trident in `hand`.
pub fn movement_gear_of(
    armour: Option<&ArmourInventory>,
    hand: Option<&ItemStack>,
) -> MovementGear {
    let worn = |slot: fn(&ArmourInventory) -> &ItemStack, enchantment| {
        armour.is_some_and(|armour| enchantment_level(slot(armour), enchantment) > 0)
    };
    MovementGear {
        depth_strider: worn(ArmourInventory::boots, Enchantment::DepthStrider),
        soul_speed: worn(ArmourInventory::boots, Enchantment::SoulSpeed),
        swift_sneak: worn(ArmourInventory::leggings, Enchantment::SwiftSneak),
        riptide: hand.is_some_and(|stack| {
            stack.item_id() == "minecraft:trident"
                && enchantment_level(stack, Enchantment::Riptide) > 0
        }),
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
    /// Corrections sent.
    pub corrections: u32,
    /// How the last input was taken.
    pub last: Option<Judgement>,
    /// Inputs left after a knockback in which a slower client is judged.
    pushed: u8,
    /// Inputs left of a riptide spin.
    spin: u8,
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
    pub judgement: Judgement,
}

impl Outcome {
    /// An input of a player without a copy (no world to simulate in): the
    /// reported position and ground contact.
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
            judgement: Judgement::Trusted(Trust::Unsimulated),
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

/// `to - from`, per axis.
fn offset(from: [f32; 3], to: [f32; 3]) -> [f32; 3] {
    [0, 1, 2].map(|axis| to[axis] - from[axis])
}

/// The horizontal distance between `a` and `b`.
fn horizontal(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

/// The controls an input reports: move vector as pressed (x strafe, y
/// forward), rotation, and the jump, sneak and sprint keys. A sprint the
/// client starts without the sprint key (a double tap) counts as pressed:
/// whether BDS starts sprints so is unmeasured, and sprinting where the copy
/// would not is the lenient way to be wrong.
pub fn controls(input: &PlayerAuthInputPacket) -> Controls {
    let has = |flag| input.input_data.contains(&flag);
    Controls {
        forward: input.move_vector.y,
        strafe: input.move_vector.x,
        yaw: input.player_rotation.y,
        pitch: input.player_rotation.x,
        jump: has(Flag::Jumping),
        sneak: has(Flag::Sneaking),
        sprint: has(Flag::Sprintdown) || has(Flag::Startsprinting),
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
            corrections: 0,
            last: None,
            pushed: 0,
            spin: 0,
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
        self.pushed = 0;
        self.spin = 0;
    }

    /// The server pushed the player (a knockback).
    pub fn pushed(&mut self, velocity: DVec3) {
        self.push = Some([velocity.x as f32, velocity.y as f32, velocity.z as f32]);
    }

    /// One input, in `situation`. `None` when the input is trusted but
    /// farther from the copy than the trust reaches: it is rejected whole
    /// and the copy does not change.
    pub fn input(
        &mut self,
        input: &PlayerAuthInputPacket,
        terrain: &WorldTerrain<'_, impl LayeredBlocks>,
        situation: &Situation,
    ) -> Option<Outcome> {
        let judged = self.judge_input(input, terrain, situation);
        let judgement = match &judged {
            Ok(outcome) => outcome.judgement,
            Err(trust) => Judgement::Refused(*trust),
        };
        self.last = Some(judgement);
        if let Judgement::Trusted(trust) = judgement {
            tracing::trace!(
                trust = ?trust,
                tick = input.client_tick.inputtick,
                "Trusted PlayerAuthInput"
            );
        }
        judged.ok()
    }

    /// The blocks this input's move can reach: the copy's box stretched by
    /// its velocity and grown by a block.
    fn search_box(&self) -> Aabb {
        self.body
            .aabb()
            .expand(self.body.velocity)
            .inflate([1.0, 1.0, 1.0])
    }

    /// Why `input` is trusted, if it is: the player's `situation`, the
    /// blocks around the copy, a riptide spin or a crawl space.
    fn trust(
        &self,
        input: &PlayerAuthInputPacket,
        terrain: &WorldTerrain<'_, impl LayeredBlocks>,
        situation: &Situation,
    ) -> Option<Trust> {
        let search = self.search_box();
        let gear = situation.gear;
        let sneaking = input.input_data.contains(&Flag::Sneaking) || self.body.sneaking;
        if situation.flight {
            Some(Trust::Flight)
        } else if situation.effects.unsupported() {
            Some(Trust::Effect)
        } else if self.spin > 0 {
            Some(Trust::Riptide)
        } else if !terrain.supported(&search) {
            Some(Trust::Block)
        } else if (gear.swift_sneak && sneaking)
            || (gear.depth_strider && terrain.water_near(&search))
            || (gear.soul_speed
                && terrain.kind_near(&search, |kind| {
                    matches!(kind, BlockKind::SoulSand | BlockKind::SoulSoil)
                }))
        {
            Some(Trust::Gear)
        } else if terrain.crawl_space(self.body.feet) == Some(true) {
            Some(Trust::Crawl)
        } else {
            None
        }
    }

    /// Follow the riptide spin `input` starts, goes on with or ends: it
    /// starts in water (this server has no rain) with a riptide trident in
    /// hand and lasts at most [`RIPTIDE_INPUTS`] inputs.
    fn follow_spin(
        &mut self,
        input: &PlayerAuthInputPacket,
        terrain: &WorldTerrain<'_, impl LayeredBlocks>,
        situation: &Situation,
    ) {
        let has = |flag| input.input_data.contains(&flag);
        self.spin = if !situation.gear.riptide || has(Flag::Stopspinattack) {
            0
        } else if has(Flag::Startspinattack) && terrain.water_near(&self.search_box()) {
            RIPTIDE_INPUTS
        } else {
            self.spin.saturating_sub(1)
        };
    }

    /// Carry the player's movement effects on the copy: the walk speed of
    /// speed and slowness, jump boost, slow falling and levitation.
    fn follow_effects(&mut self, effects: &MovementEffects) {
        let body = &mut self.body;
        body.jump_boost = effects.jump_boost;
        body.slow_falling = effects.slow_falling;
        body.levitation = effects.levitation;
        body.set_walk_speed(walk_speed(effects.speed, effects.slowness));
    }

    /// The input, judged, trusted, or `Err` with the trust it was refused
    /// under.
    fn judge_input(
        &mut self,
        input: &PlayerAuthInputPacket,
        terrain: &WorldTerrain<'_, impl LayeredBlocks>,
        situation: &Situation,
    ) -> Result<Outcome, Trust> {
        let tick = input.client_tick.inputtick;
        let claim = feet_of_reported(&input.position);
        self.follow_spin(input, terrain, situation);
        self.follow_effects(&situation.effects);
        if std::mem::take(&mut self.settling) {
            // The spawn point is the server's choice (a configured location
            // can sit inside a block); a client that joined or respawned
            // there starts the copy where it is. Farther off, the input is
            // simulated from the spawn and judged, as BDS judges the first
            // input after a respawn.
            let d = offset(self.body.feet, claim);
            if d.iter().map(|v| v * v).sum::<f32>() <= SPAWN_SETTLE * SPAWN_SETTLE {
                return Ok(self.trusted(input, claim, Trust::Spawn, situation.glide));
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
            return Ok(Outcome {
                position: eyes(self.body.feet),
                on_ground: true,
                jumped: false,
                correction,
                judgement: Judgement::Judged(verdict),
            });
        }
        if let Some(trust) = self.trust(input, terrain, situation) {
            let d = offset(self.body.feet, claim);
            let height = if trust == Trust::Crawl {
                Pose::Low.height()
            } else {
                self.body.pose().height()
            };
            match trust.reach() {
                Reach::Far
                    if d.iter().map(|v| v * v).sum::<f32>()
                        > MAX_TRUSTED_MOVE * MAX_TRUSTED_MOVE =>
                {
                    return Err(trust);
                }
                // Out of a near trust's reach, or through a block the
                // simulation reproduces: the copy judges the input.
                Reach::Near(bounds)
                    if d.iter().zip(bounds).any(|(d, bound)| d.abs() > bound)
                        || !terrain.clear_path(self.body.feet, claim, height) => {}
                _ => return Ok(self.trusted(input, claim, trust, situation.glide)),
            }
        }
        Ok(self.simulate(input, claim, terrain, situation))
    }

    /// Simulate `input` on the copy and judge `claim` against it.
    fn simulate(
        &mut self,
        input: &PlayerAuthInputPacket,
        claim: [f32; 3],
        terrain: &WorldTerrain<'_, impl LayeredBlocks>,
        situation: &Situation,
    ) -> Outcome {
        let controls = controls(input);
        if let Some(push) = self.push.take() {
            self.body.velocity = push;
            self.pushed = PUSH_SETTLE;
        } else {
            self.pushed = self.pushed.saturating_sub(1);
        }
        let pull = self.authority.take_pull();
        for (velocity, pull) in self.body.velocity.iter_mut().zip(pull) {
            *velocity += pull;
        }
        // Only a move the keys drive can be slowed: a pull, or momentum
        // after the keys are let go, moves the copy as BDS's.
        let driven = pull == [0.0; 3] && (controls.forward != 0.0 || controls.strafe != 0.0);
        if situation.glide
            && input.input_data.contains(&Flag::Startgliding)
            && !self.body.on_ground
            && !self.body.gliding
            && !self.body.in_water
        {
            self.body.gliding = true;
        }
        let start = self.body.feet;
        let changes = sim::step(&mut self.body, &controls, terrain);
        let moved = self.body.feet[0] != start[0] || self.body.feet[2] != start[2];
        let height = self.body.pose().height();
        let free = |feet| !authority::inside_blocks(terrain, feet, height);
        if driven && free(claim) && self.slowed(start, claim) {
            return self.trusted(input, claim, Trust::Slowed, situation.glide);
        }
        let verdict = self.authority.judge(&mut self.body, claim, moved, free);
        let correction = (verdict == Verdict::Correct).then(|| {
            self.corrections += 1;
            correction_packet(&self.body, self.body.velocity, input.client_tick.inputtick)
        });
        Outcome {
            position: eyes(self.body.feet),
            on_ground: self.body.on_ground,
            jumped: changes.jumped,
            correction,
            judgement: Judgement::Judged(verdict),
        }
    }

    /// Whether `claim` is only slower than the copy's simulation of the
    /// input from `start`: apart from it, but no farther horizontally and
    /// level with it. Not while a knockback plays out, nor after a teleport
    /// until a correction was sent.
    fn slowed(&self, start: [f32; 3], claim: [f32; 3]) -> bool {
        let simulated = self.body.feet;
        let reach = horizontal(simulated, start);
        self.pushed == 0
            && self.authority.trusting()
            && reach > TINY
            && horizontal(claim, simulated) > TINY
            && horizontal(claim, start) <= reach
            && (claim[1] - simulated[1]).abs() <= TINY
    }

    /// Take the client's position and velocity (the fail-safe), and the
    /// state the simulation carries from one input to the next as the
    /// client's flags report it, so the copy picks up where the client is
    /// when it judges again.
    fn trusted(
        &mut self,
        input: &PlayerAuthInputPacket,
        claim: [f32; 3],
        trust: Trust,
        glide: bool,
    ) -> Outcome {
        let has = |flag| input.input_data.contains(&flag);
        let body = &mut self.body;
        body.feet = claim;
        body.velocity = [input.pos_delta.x, input.pos_delta.y, input.pos_delta.z]
            .map(|v| v.clamp(-MAX_TRUSTED_SPEED, MAX_TRUSTED_SPEED));
        // The landing a trusted client reports, as the server took it before
        // it simulated movement.
        body.on_ground = has(Flag::Verticalcollision);
        body.vertical_collision = has(Flag::Verticalcollision);
        body.horizontal_collision = has(Flag::Horizontalcollision);
        body.sneaking = has(Flag::Sneaking);
        // A sprint with its speed: the simulation sets the speed only when a
        // sprint starts. A stopping sprint moved this tick as a sprint.
        body.sprinting = has(Flag::Sprinting);
        body.moved_sprinting = body.sprinting || has(Flag::Stopsprinting);
        body.set_walk_speed(body.walk_speed);
        if has(Flag::Startswimming) {
            body.swimming = true;
        }
        if has(Flag::Stopswimming) {
            body.swimming = false;
        }
        if has(Flag::Startgliding) && glide {
            body.gliding = true;
        }
        if has(Flag::Stopgliding) {
            body.gliding = false;
        }
        // The held jump key's delay as the simulation counts it: a jump
        // leaves it one tick down from the full delay.
        body.jump_delay = if !has(Flag::Jumping) {
            0
        } else if has(Flag::Startjumping) {
            JUMP_DELAY - 1
        } else {
            body.jump_delay.saturating_sub(1)
        };
        self.push = None;
        self.authority.synced();
        Outcome {
            position: eyes(claim),
            on_ground: self.body.on_ground,
            jumped: has(Flag::Startjumping),
            correction: None,
            judgement: Judgement::Trusted(trust),
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
