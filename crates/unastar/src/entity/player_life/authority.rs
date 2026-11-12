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
//! runs of the sweep client confirmed. Any other input is trusted (the copy
//! takes the client's position and velocity, as the server did before):
//! flight in a game mode that can fly, a spectator, movement effects, an
//! item in use, a sprint too hungry to sprint, movement enchantments where
//! they act, a riptide spin, crawling, and blocks whose movement the
//! simulation does not reproduce yet ([`MotionTable`]); see [`Trust`]. A
//! trusted input more than [`MAX_TRUSTED_MOVE`] blocks from the copy is
//! rejected whole, and a teleport is still enforced. A client must never be
//! corrected where BDS would not correct it.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::types::{
    EnumsPlayerAuthInputPacketPayloadInputData as Flag, EnumsRewindType, PlayerInputTick, Vec2,
    Vec3,
};
use jolyne::valentine::{CorrectPlayerMovePredictionPacket, PlayerAuthInputPacket};
use unastar_rules::movement::authority::{self, Authority, Verdict};
use unastar_rules::movement::{
    self as sim, Aabb, Block, BlockKind, Body, Controls, JUMP_DELAY, NETWORK_OFFSET, Pose,
    SPRINT_MULTIPLIER, Terrain, WALK_SPEED, WIDTH,
};

use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::{ArmourInventory, EffectType, Effects};
use crate::entity::systems::BlockSource;
use crate::entity::systems::physics::collect_block_boxes;
use crate::item::ItemStack;
use crate::item::enchant::{Enchantment, enchantment_level};
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
        Some(chunk_layer(chunk, pos, 0))
    }

    fn registry(&self) -> &BlockRegistry {
        self.registry
    }
}

/// Blocks with their second layer, where a waterlogged block keeps its
/// water (seagrass, kelp, waterlogged stairs).
pub trait LayeredBlocks: BlockSource {
    /// Runtime ID of the second-layer block at `pos`; `None` if the chunk
    /// is not loaded.
    fn second_layer_at(&self, pos: IVec3) -> Option<u32>;
}

impl LayeredBlocks for LoadedBlocks<'_, '_, '_> {
    fn second_layer_at(&self, pos: IVec3) -> Option<u32> {
        let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
        let chunk = self.chunks.get(self.manager.get_by_coords(cx, cz)?).ok()?;
        Some(chunk_layer(chunk, pos, 1))
    }
}

impl LayeredBlocks for World {
    fn second_layer_at(&self, pos: IVec3) -> Option<u32> {
        let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
        let entity = self.get_resource::<ChunkManager>()?.get_by_coords(cx, cz)?;
        Some(chunk_layer(self.get::<ChunkData>(entity)?, pos, 1))
    }
}

/// The block in `layer` at `pos` of a loaded chunk; air above and below
/// the world.
fn chunk_layer(chunk: &ChunkData, pos: IVec3, layer: u8) -> u32 {
    if !(MIN_Y..MIN_Y + i32::from(SUBCHUNK_COUNT) * 16).contains(&pos.y) {
        return *crate::world::chunk::blocks::AIR;
    }
    let (x, y, z) = world_to_local_coords(pos.x, pos.y, pos.z);
    chunk.inner.get_block_layer(x, y, z, layer)
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
/// cobwebs are simulated but not yet confirmed against BDS. Lava, flowing
/// water (whose current pushes) and blocks whose friction is not
/// [`CONFIRMED_FRICTION`] (ice) are not supported either.
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

/// The ground friction BDS runs of the sweep client confirmed (every block
/// but ice, honey and slime).
const CONFIRMED_FRICTION: f32 = 0.6;

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
                let liquid = registry.liquid_state(id);
                // Still water only: a current pushes, and lava has rules of
                // its own.
                let liquid_supported = liquid
                    .is_none_or(|liquid| liquid.kind == LiquidKind::Water && liquid.depth == 0);
                let friction_supported = kind == BlockKind::Air
                    || (entry.data.friction - CONFIRMED_FRICTION).abs() < 1.0e-6;
                let supported = !UNSUPPORTED.contains(&entry.string_id.as_str())
                    && liquid_supported
                    && friction_supported;
                blocks[index] = (
                    Block {
                        friction: entry.data.friction,
                        kind,
                        water: liquid.is_some_and(|liquid| liquid.kind == LiquidKind::Water),
                    },
                    supported,
                );
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
        !self.every_block(area, 0, |pos| !self.block(pos.to_array()).water)
    }

    /// Whether `test` holds for every block position of `area`, reaching
    /// `below` layers under it.
    fn every_block(&self, area: &Aabb, below: i32, test: impl Fn(IVec3) -> bool) -> bool {
        let (min, max) = area.block_range();
        (min[0]..=max[0]).all(|x| {
            (min[1] - below..=max[1]).all(|y| (min[2]..=max[2]).all(|z| test(IVec3::new(x, y, z))))
        })
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

impl<B: LayeredBlocks> Terrain for WorldTerrain<'_, B> {
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
        block.water |= self
            .blocks
            .second_layer_at(pos)
            .is_some_and(|id| self.table.get(id).0.water);
        block
    }
}

/// Why an input is trusted instead of simulated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust {
    /// The simulation judges the input.
    No,
    /// Flying in a game mode that can fly, or a spectator: flight is not
    /// simulated.
    Flight,
    /// A movement effect the simulation does not apply, or applies without
    /// a BDS run confirming it.
    Effect,
    /// An item in use (eating, drawing a bow) slows the player.
    UsingItem,
    /// Sprinting with food at 6 or below, where the sprint key does not
    /// sprint.
    Hungry,
    /// A movement enchantment where it acts (depth strider in water, swift
    /// sneak sneaking, soul speed), or a riptide trident's spin.
    Gear,
    /// The copy or the client in a space only a crawling player fits.
    Crawl,
    /// A block the simulation does not reproduce is near.
    Block,
    /// The first input after joining or respawning, close to the spawn
    /// point: the copy starts where the client does.
    Spawn,
}

/// How far from the spawn point the first input may put the copy.
const SPAWN_SETTLE: f32 = 2.0;

/// The farthest a trusted input may move the copy: farther, it is rejected
/// whole and the copy stays.
pub const MAX_TRUSTED_MOVE: f32 = 64.0;

/// A player's movement effects as the simulation reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MovementEffects {
    /// Jump boost level (amplifier + 1), 0 without.
    pub jump_boost: u8,
    pub slow_falling: bool,
    /// An effect the input is trusted under: speed, slowness, levitation
    /// and blindness (no sprint) are not simulated; jump boost and slow
    /// falling are, but no BDS run has confirmed them yet.
    pub unsupported: bool,
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
    /// An item is in use.
    pub using_item: bool,
    /// Food at 6 or below in a game mode that cannot fly.
    pub hungry: bool,
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
            EffectType::BLINDNESS,
            EffectType::JUMP_BOOST,
            EffectType::SLOW_FALLING,
        ]
        .into_iter()
        .any(|effect| effects.has(effect)),
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
    /// A riptide spin is under way (from `StartSpinAttack` with a riptide
    /// trident in hand until `StopSpinAttack`).
    pub spinning: bool,
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
            spinning: false,
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

    /// One input, in `situation`. `None` when the input is trusted but
    /// more than [`MAX_TRUSTED_MOVE`] from the copy: it is rejected whole
    /// and the copy does not change.
    pub fn input(
        &mut self,
        input: &PlayerAuthInputPacket,
        terrain: &WorldTerrain<'_, impl LayeredBlocks>,
        situation: &Situation,
    ) -> Option<Outcome> {
        let outcome = self.judge_input(input, terrain, situation)?;
        self.last = (outcome.verdict, outcome.trust);
        if outcome.trust != Trust::No {
            tracing::trace!(trust = ?outcome.trust, tick = input.client_tick.inputtick, "Trusted PlayerAuthInput");
        }
        Some(outcome)
    }

    /// Why `input` is trusted, if it is: the player's `situation`, the
    /// blocks around the copy and a crawl space at either end.
    fn trust(
        &self,
        input: &PlayerAuthInputPacket,
        claim: [f32; 3],
        terrain: &WorldTerrain<'_, impl LayeredBlocks>,
        situation: &Situation,
        spinning: bool,
    ) -> Trust {
        let has = |flag| input.input_data.contains(&flag);
        let search = self
            .body
            .aabb()
            .expand(self.body.velocity)
            .inflate([1.0, 1.0, 1.0]);
        let gear = situation.gear;
        let sneaking = has(Flag::Sneaking) || self.body.sneaking;
        let sprinting = has(Flag::Sprintdown) || self.body.sprinting;
        if situation.flight {
            Trust::Flight
        } else if situation.effects.unsupported {
            Trust::Effect
        } else if situation.using_item {
            Trust::UsingItem
        } else if situation.hungry && sprinting {
            Trust::Hungry
        } else if !terrain.supported(&search) {
            Trust::Block
        } else if spinning
            || gear.soul_speed
            || (gear.swift_sneak && sneaking)
            || (gear.depth_strider && terrain.water_near(&search))
        {
            Trust::Gear
        } else if [self.body.feet, claim]
            .into_iter()
            .any(|feet| terrain.crawl_space(feet) == Some(true))
        {
            Trust::Crawl
        } else {
            Trust::No
        }
    }

    fn judge_input(
        &mut self,
        input: &PlayerAuthInputPacket,
        terrain: &WorldTerrain<'_, impl LayeredBlocks>,
        situation: &Situation,
    ) -> Option<Outcome> {
        let tick = input.client_tick.inputtick;
        let claim = feet_of_reported(&input.position);
        let has = |flag| input.input_data.contains(&flag);
        let spinning = situation.gear.riptide
            && (self.spinning || has(Flag::Startspinattack))
            && !has(Flag::Stopspinattack);
        if std::mem::take(&mut self.settling) {
            // The spawn point is the server's choice (a configured location
            // can sit inside a block); a client that joined or respawned
            // there starts the copy where it is. Farther off, the input is
            // simulated from the spawn and judged, as BDS judges the first
            // input after a respawn.
            let d = [0, 1, 2].map(|axis| claim[axis] - self.body.feet[axis]);
            if d.iter().map(|v| v * v).sum::<f32>() <= SPAWN_SETTLE * SPAWN_SETTLE {
                self.spinning = spinning;
                return Some(self.trusted(input, claim, Trust::Spawn, situation.glide));
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
            self.spinning = spinning;
            return Some(Outcome {
                position: eyes(self.body.feet),
                on_ground: true,
                jumped: false,
                correction,
                verdict: Some(verdict),
                trust: Trust::No,
            });
        }
        let trust = self.trust(input, claim, terrain, situation, spinning);
        if trust != Trust::No {
            let d = [0, 1, 2].map(|axis| claim[axis] - self.body.feet[axis]);
            if d.iter().map(|v| v * v).sum::<f32>() > MAX_TRUSTED_MOVE * MAX_TRUSTED_MOVE {
                return None;
            }
            self.spinning = spinning;
            return Some(self.trusted(input, claim, trust, situation.glide));
        }
        self.spinning = spinning;
        let controls = controls(input);
        if let Some(push) = self.push.take() {
            self.body.velocity = push;
        }
        let pull = self.authority.take_pull();
        for (velocity, pull) in self.body.velocity.iter_mut().zip(pull) {
            *velocity += pull;
        }
        self.body.jump_boost = situation.effects.jump_boost;
        self.body.slow_falling = situation.effects.slow_falling;
        if situation.glide
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
        Some(Outcome {
            position: eyes(self.body.feet),
            on_ground: self.body.on_ground,
            jumped: changes.jumped,
            correction,
            verdict: Some(verdict),
            trust: Trust::No,
        })
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
        body.velocity = [input.pos_delta.x, input.pos_delta.y, input.pos_delta.z];
        // The landing a trusted client reports, as the server took it before
        // it simulated movement.
        body.on_ground = has(Flag::Verticalcollision);
        body.vertical_collision = has(Flag::Verticalcollision);
        body.horizontal_collision = has(Flag::Horizontalcollision);
        body.sneaking = has(Flag::Sneaking);
        // A sprint with its speed: the simulation sets the speed only when a
        // sprint starts.
        body.sprinting = has(Flag::Sprinting);
        body.moved_sprinting = body.sprinting;
        body.movement_speed = if body.sprinting {
            WALK_SPEED * SPRINT_MULTIPLIER
        } else {
            WALK_SPEED
        };
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
