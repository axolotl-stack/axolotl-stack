//! Plays the movement-diff corpus through acacia-physics (MIT, pinned in
//! `Cargo.toml`) and writes its traces in the harness's format.
//!
//! ```text
//! cargo run --release -- --corpus resolved.json --out acacia.json [--world-out acacia-world.json]
//! ```
//!
//! A scenario with a block acacia has no physics for is skipped (listed on
//! stderr); `--world-out` writes each scenario's blocks as the runner builds
//! them (boxes and friction) for `compare.py --worlds`.
//!
//! The corpus must be `movement-diff resolve`'s: every block state spelled out
//! and each block's collision boxes (BDS 1.26.51's shapes). The block physics
//! (friction, climbable, honey, bounce, liquids...) are acacia's own: its
//! `TestBlock` definitions where it has one for the block, its
//! `BlockPhysics` constructors otherwise. Edge flags (start and stop of
//! sprint, sneak, jump, swim, glide) are acacia's: `tick` derives them from
//! the held controls and its state.

use acacia_physics::test_world::TestBlock;
use acacia_physics::{
    Aabb, BlockPhysics, BlockPos, BubbleColumn, Input, Liquid, LiquidKind, PlayerState, WorldView,
    tick, touching_water,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::process::ExitCode;

/// The wire position's height above the feet.
const NETWORK_OFFSET: f32 = 1.620_01;
const PINNED: &str = "acacia-rs/acacia acacia-physics @ 97ec4c7ce23e5abb7cde1e66026363d3c2e6fbf0";

#[derive(Deserialize)]
struct Corpus {
    scenarios: Vec<Scenario>,
}

#[derive(Deserialize)]
struct Scenario {
    name: String,
    world: Vec<Fill>,
    start: Start,
    inputs: Vec<Segment>,
    #[serde(default)]
    events: Vec<Event>,
}

#[derive(Deserialize)]
struct Fill {
    from: [i32; 3],
    to: [i32; 3],
    block: Block,
}

#[derive(Deserialize)]
struct Block {
    name: String,
    #[serde(default)]
    states: BTreeMap<String, serde_json::Value>,
    collision: Vec<[f32; 6]>,
}

#[derive(Deserialize)]
struct Start {
    feet: [f32; 3],
    #[serde(default)]
    yaw: f32,
    #[serde(default)]
    pitch: f32,
    #[serde(default)]
    effects: Vec<EffectSpec>,
    #[serde(default)]
    elytra: bool,
}

#[derive(Deserialize)]
struct EffectSpec {
    effect: String,
    amplifier: i32,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Segment {
    ticks: u32,
    forward: f32,
    strafe: f32,
    jump: bool,
    sneak: bool,
    sprint: bool,
    glide: bool,
    yaw: Option<f32>,
    pitch: Option<f32>,
    turn: f32,
    tilt: f32,
}

#[derive(Deserialize)]
struct Event {
    after: u32,
    #[serde(default)]
    velocity: Option<[f32; 3]>,
}

/// The harness's `f32` encoding: the value and its hex bits.
#[derive(Serialize)]
struct Exact {
    value: Vec<f32>,
    bits: Vec<String>,
}

impl From<&[f32]> for Exact {
    fn from(values: &[f32]) -> Self {
        Exact {
            value: values.to_vec(),
            bits: values
                .iter()
                .map(|v| format!("{:08x}", v.to_bits()))
                .collect(),
        }
    }
}

#[derive(Serialize)]
struct TickState {
    tick: u32,
    position: Exact,
    velocity: Exact,
    on_ground: bool,
    horizontal_collision: bool,
    vertical_collision: bool,
    fall_distance: Exact,
}

#[derive(Serialize)]
struct ScenarioTrace {
    ticks: Vec<TickState>,
}

#[derive(Serialize)]
struct Traces {
    implementation: &'static str,
    source: &'static str,
    scenarios: BTreeMap<String, ScenarioTrace>,
}

/// A cell of the scenario's world.
#[derive(Clone)]
struct Cell {
    name: String,
    boxes: Vec<Aabb>,
    physics: BlockPhysics,
}

#[derive(Default)]
struct World {
    cells: HashMap<BlockPos, Cell>,
}

impl WorldView for World {
    fn block_collisions(&self, pos: BlockPos, out: &mut Vec<Aabb>) {
        if let Some(cell) = self.cells.get(&pos) {
            out.extend_from_slice(&cell.boxes);
        }
    }

    fn block(&self, pos: BlockPos) -> BlockPhysics {
        self.cells
            .get(&pos)
            .map_or(BlockPhysics::AIR, |c| c.physics)
    }
}

impl World {
    /// The cells as the runner built them, but plain full cubes of friction
    /// 0.6: world-space boxes and acacia's friction.
    fn dump(&self) -> serde_json::Value {
        let mut out = serde_json::Map::new();
        for (pos, cell) in &self.cells {
            let p = pos.map(|v| v as f32);
            let boxes: Vec<[f32; 6]> = cell
                .boxes
                .iter()
                .map(|b| {
                    [
                        b.min[0] + p[0],
                        b.min[1] + p[1],
                        b.min[2] + p[2],
                        b.max[0] + p[0],
                        b.max[1] + p[1],
                        b.max[2] + p[2],
                    ]
                })
                .collect();
            let full = [p[0], p[1], p[2], p[0] + 1.0, p[1] + 1.0, p[2] + 1.0];
            if cell.physics.liquid.is_none() && cell.physics.friction == 0.6 && boxes == [full] {
                continue;
            }
            out.insert(
                format!("{},{},{}", pos[0], pos[1], pos[2]),
                serde_json::json!({ "block": cell.name, "boxes": boxes, "friction": cell.physics.friction }),
            );
        }
        serde_json::Value::Object(out)
    }
}

/// acacia's physics of a BDS block, or why it has none.
fn physics(block: &Block) -> Result<BlockPhysics, String> {
    let name = block.name.strip_prefix("minecraft:").unwrap_or(&block.name);
    let flag = |state: &str| block.states.get(state).and_then(serde_json::Value::as_bool);
    let kind = match name {
        "stone" | "glass" | "dirt" => "stone",
        "smooth_stone_slab" => "slab",
        "oak_stairs" => "stair",
        "honey_block" => "honey",
        "web" => "web",
        "water" | "lava" | "ladder" | "vine" | "ice" | "packed_ice" | "blue_ice" | "soul_sand"
        | "slime" | "scaffolding" | "powder_snow" | "bed" => name,
        "sweet_berry_bush" => "berry_bush",
        "twisting_vines" | "weeping_vines" | "cave_vines" => "vine",
        // Plain solid blocks to acacia: their shapes are BDS's.
        "magma" | "white_carpet" | "snow_layer" | "trapdoor" | "wooden_door" | "iron_chain"
        | "iron_bars" | "glass_pane" | "frosted_ice" => {
            return Ok(BlockPhysics {
                friction: if name == "frosted_ice" { 0.98 } else { 0.6 },
                ..BlockPhysics::block()
            });
        }
        "fence_gate" | "cobblestone_wall" => {
            return Ok(BlockPhysics {
                fence_or_wall: true,
                ..BlockPhysics::block()
            });
        }
        "oak_fence" => {
            return Ok(BlockPhysics {
                fence_or_wall: true,
                ..BlockPhysics::block()
            });
        }
        "bubble_column" => {
            return Ok(BlockPhysics {
                bubble_column: Some(BubbleColumn {
                    downward: flag("drag_down").unwrap_or(false),
                    surface: None,
                }),
                ..BlockPhysics::liquid(Liquid::source(LiquidKind::Water))
            });
        }
        other => return Err(format!("no acacia physics for {other}")),
    };
    TestBlock::from_kind(kind)
        .map(|b| b.physics())
        .ok_or_else(|| format!("acacia has no test block {kind}"))
}

fn build(scenario: &Scenario) -> Result<World, String> {
    let mut world = World::default();
    for fill in &scenario.world {
        let cell = Cell {
            name: fill.block.name.clone(),
            boxes: fill
                .block
                .collision
                .iter()
                .map(|b| Aabb {
                    min: [b[0], b[1], b[2]],
                    max: [b[3], b[4], b[5]],
                })
                .collect(),
            physics: physics(&fill.block)?,
        };
        let lo = [0, 1, 2].map(|a| fill.from[a].min(fill.to[a]));
        let hi = [0, 1, 2].map(|a| fill.from[a].max(fill.to[a]));
        for x in lo[0]..=hi[0] {
            for y in lo[1]..=hi[1] {
                for z in lo[2]..=hi[2] {
                    world.cells.insert([x, y, z], cell.clone());
                }
            }
        }
    }
    Ok(world)
}

/// `minecraft:movement` without the sprint: 0.1 with the speed and
/// slowness effects (BDS sends 0.14 for speed II, 0.07 for slowness II).
fn movement_attribute(scenario: &Scenario) -> f32 {
    let level = |name: &str| {
        scenario
            .start
            .effects
            .iter()
            .filter(|e| e.effect == name)
            .map(|e| e.amplifier + 1)
            .max()
            .unwrap_or(0) as f32
    };
    0.1 * (1.0 + 0.2 * level("speed")) * (1.0 - 0.15 * level("slowness"))
}

fn run(scenario: &Scenario) -> Result<ScenarioTrace, String> {
    let world = build(scenario)?;
    let mut state = PlayerState::new(scenario.start.feet);
    // BDS's teleport leaves the player on the ground until the first move.
    state.on_ground = true;
    state.yaw = scenario.start.yaw;
    state.pitch = scenario.start.pitch;
    state.prev_pitch = scenario.start.pitch;
    for effect in &scenario.start.effects {
        match effect.effect.as_str() {
            "jump_boost" => state.effects.jump_boost = Some(effect.amplifier),
            "levitation" => state.effects.levitation = Some(effect.amplifier),
            "slow_falling" => state.effects.slow_falling = true,
            _ => {}
        }
    }
    state.equipment.elytra = scenario.start.elytra;
    state.set_movement_attribute(movement_attribute(scenario));
    let velocities: HashMap<u32, [f32; 3]> = scenario
        .events
        .iter()
        .filter_map(|e| Some((e.after, e.velocity?)))
        .collect();
    let (mut yaw, mut pitch) = (scenario.start.yaw, scenario.start.pitch);
    let mut gliding = false;
    let mut ticks = Vec::new();
    let mut number = 0u32;
    for segment in &scenario.inputs {
        yaw = segment.yaw.unwrap_or(yaw);
        pitch = segment.pitch.unwrap_or(pitch);
        for i in 0..segment.ticks {
            number += 1;
            yaw += segment.turn;
            pitch += segment.tilt;
            if let Some(velocity) = velocities.get(&(number - 1)) {
                state.queue_knockback(*velocity);
            }
            let glide = segment.glide && i == 0;
            gliding |= glide;
            let input = Input {
                move_vector: [segment.strafe, segment.forward],
                yaw,
                pitch,
                // A vanilla client opens the elytra on a jump press.
                jump: segment.jump || glide,
                sneak: segment.sneak,
                // The vanilla client sends WantDown with every sneak.
                want_down: segment.sneak,
                sprint: segment.sprint,
                // Vanilla: a sprint while touching water asks to swim.
                swim: segment.sprint && touching_water(&state, &world),
                glide: gliding,
                using_item: false,
            };
            let out = tick(&mut state, &input, &world);
            let feet = out.position;
            let position = [feet[0], feet[1] + NETWORK_OFFSET, feet[2]];
            ticks.push(TickState {
                tick: number,
                position: Exact::from(&position[..]),
                velocity: Exact::from(&out.delta[..]),
                on_ground: out.on_ground,
                horizontal_collision: out.horizontal_collision,
                vertical_collision: out.vertical_collision,
                fall_distance: Exact::from(&[state.fall_distance][..]),
            });
        }
    }
    Ok(ScenarioTrace { ticks })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let arg = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let world_out = arg("--world-out");
    let (Some(corpus), Some(out)) = (arg("--corpus"), arg("--out")) else {
        eprintln!("usage: acacia-runner --corpus RESOLVED.json --out ACACIA.json [--world-out WORLD.json]");
        return ExitCode::from(2);
    };
    let result = (|| -> Result<(), String> {
        let text = std::fs::read_to_string(&corpus).map_err(|e| format!("{corpus}: {e}"))?;
        let corpus: Corpus = serde_json::from_str(&text).map_err(|e| format!("{corpus}: {e}"))?;
        let mut traces = Traces {
            implementation: "acacia",
            source: PINNED,
            scenarios: BTreeMap::new(),
        };
        let mut worlds = serde_json::Map::new();
        for scenario in &corpus.scenarios {
            let world = match build(scenario) {
                Ok(world) => world,
                Err(error) => {
                    eprintln!("acacia-runner: skipping {}: {error}", scenario.name);
                    continue;
                }
            };
            worlds.insert(scenario.name.clone(), world.dump());
            let trace = run(scenario).map_err(|e| format!("scenario {}: {e}", scenario.name))?;
            traces.scenarios.insert(scenario.name.clone(), trace);
        }
        if let Some(path) = &world_out {
            let value = serde_json::json!({ "runner": "acacia", "scenarios": worlds });
            let text = serde_json::to_string_pretty(&value).map_err(|e| e.to_string())? + "\n";
            std::fs::write(path, text).map_err(|e| format!("{path}: {e}"))?;
        }
        let text = serde_json::to_string_pretty(&traces).map_err(|e| e.to_string())? + "\n";
        std::fs::write(&out, text).map_err(|e| format!("{out}: {e}"))
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("acacia-runner: {error}");
            ExitCode::FAILURE
        }
    }
}
