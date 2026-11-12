//! Movement scenarios of the three-way movement harness
//! (`tools/movement-diff`): a small world, a start state and a list of
//! per-tick inputs, played the same way by BDS 1.26.51 (through the scripted
//! client), `unastar_rules::movement` ([`simulate`]) and bedsim (the Go
//! runner). Each implementation writes one [`Traces`] file: the state after
//! every input, with exact `f32` bit patterns.
//!
//! Positions in traces are wire positions (the feet plus
//! [`physics::NETWORK_OFFSET`], added in `f32`), because that is what BDS
//! sends; comparing them bit for bit compares the feet too.

use crate::physics::{self, Body, Controls};
use crate::world::World;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use unastar_data::blocks::{self, BlockData, StateValue};

/// Every scenario's blocks and start lie inside this box (inclusive), which
/// the BDS runner clears before building a scenario. It is high above the
/// superflat ground, so nothing else is near.
pub const AREA_MIN: [i32; 3] = [-8, 90, -8];
pub const AREA_MAX: [i32; 3] = [8, 120, 40];

/// Idle inputs every scenario starts with.
pub const LEADING_IDLE: u32 = 3;

/// The scenario corpus (`tools/movement-diff/corpus.json`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Corpus {
    pub scenarios: Vec<Scenario>,
}

impl Corpus {
    /// Parse and validate a corpus.
    pub fn parse(text: &str) -> Result<Self> {
        let corpus: Corpus = serde_json::from_str(text).context("parsing the corpus")?;
        let mut names = std::collections::HashSet::new();
        for scenario in &corpus.scenarios {
            ensure!(
                names.insert(scenario.name.as_str()),
                "scenario {} is listed twice",
                scenario.name
            );
            scenario
                .validate()
                .with_context(|| format!("scenario {}", scenario.name))?;
        }
        Ok(corpus)
    }

    pub fn get(&self, name: &str) -> Option<&Scenario> {
        self.scenarios.iter().find(|s| s.name == name)
    }
}

/// One scenario.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    pub description: String,
    /// Blocks placed in order into an empty (air) [`AREA_MIN`]..[`AREA_MAX`].
    pub world: Vec<Fill>,
    pub start: Start,
    /// The inputs, in runs of identical ticks.
    pub inputs: Vec<Segment>,
    #[serde(default)]
    pub events: Vec<Event>,
}

/// A box of one block, corners inclusive.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fill {
    pub from: [i32; 3],
    pub to: [i32; 3],
    pub block: BlockSpec,
}

/// A block state: the block's name and the states that differ from its
/// first canonical state.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockSpec {
    pub name: String,
    #[serde(default)]
    pub states: BTreeMap<String, StateJson>,
}

/// A state value as written in the corpus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StateJson {
    Bool(bool),
    Int(i32),
    Str(String),
}

impl StateJson {
    fn matches(&self, value: &StateValue) -> bool {
        match (self, value) {
            (StateJson::Bool(b), StateValue::Byte(v)) => u8::from(*b) == *v,
            (StateJson::Int(i), StateValue::Int(v)) => i == v,
            (StateJson::Int(i), StateValue::Byte(v)) => *i == i32::from(*v),
            (StateJson::Str(s), StateValue::Str(v)) => s == v,
            _ => false,
        }
    }
}

/// A resolved block state.
#[derive(Debug, Clone, Copy)]
pub struct BlockState {
    pub state: u32,
    pub data: &'static BlockData,
}

impl BlockSpec {
    /// The canonical state this names.
    pub fn resolve(&self) -> Result<BlockState> {
        let data =
            blocks::get(&self.name).with_context(|| format!("unknown block {}", self.name))?;
        let mut state = data.min_state_id;
        for (property, wanted) in &self.states {
            let row = data
                .properties
                .iter()
                .find(|p| p.name == property)
                .with_context(|| format!("{} has no state {property}", self.name))?;
            let value = row
                .values
                .iter()
                .find(|v| wanted.matches(v))
                .with_context(|| format!("{} {property} cannot be {wanted:?}", self.name))?;
            state = data
                .with_state_value(state, property, *value)
                .with_context(|| {
                    format!("{} has no state with {property} {wanted:?}", self.name)
                })?;
        }
        Ok(BlockState { state, data })
    }
}

impl BlockState {
    /// Every state of the block, as the corpus writes them (bytes as
    /// booleans).
    pub fn states(&self) -> BTreeMap<String, StateJson> {
        self.data
            .properties
            .iter()
            .filter_map(|p| {
                let value = match self.data.state_value(self.state, p.name)? {
                    StateValue::Byte(v) => StateJson::Bool(v != 0),
                    StateValue::Int(v) => StateJson::Int(v),
                    StateValue::Str(v) => StateJson::Str(v.to_owned()),
                };
                Some((p.name.to_owned(), value))
            })
            .collect()
    }

    /// The block as a command argument, every state spelled out so BDS
    /// places exactly this state.
    pub fn command_text(&self) -> String {
        let name = self
            .data
            .identifier
            .strip_prefix("minecraft:")
            .unwrap_or(self.data.identifier);
        if self.data.properties.is_empty() {
            return name.to_owned();
        }
        let states: Vec<String> = self
            .data
            .properties
            .iter()
            .filter_map(|p| {
                let value = self.data.state_value(self.state, p.name)?;
                Some(match value {
                    StateValue::Byte(v) => format!("\"{}\"={}", p.name, v != 0),
                    StateValue::Int(v) => format!("\"{}\"={v}", p.name),
                    StateValue::Str(v) => format!("\"{}\"=\"{v}\"", p.name),
                })
            })
            .collect();
        format!("{name} [{}]", states.join(","))
    }
}

/// Where and how the player starts: at rest at `feet`, as a teleport leaves
/// it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Start {
    pub feet: [f32; 3],
    #[serde(default)]
    pub yaw: f32,
    #[serde(default)]
    pub pitch: f32,
    #[serde(default)]
    pub effects: Vec<EffectSpec>,
    /// An elytra in the chest slot.
    #[serde(default)]
    pub elytra: bool,
}

/// A status effect active through the scenario.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectSpec {
    pub effect: Effect,
    /// Zero-based, as `/effect` takes it (0 is level I).
    pub amplifier: u8,
}

/// The status effects that change movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Speed,
    Slowness,
    JumpBoost,
    SlowFalling,
    Levitation,
    /// No movement effect: keeps a player in lava alive.
    FireResistance,
}

impl Effect {
    /// The `/effect` name.
    pub fn command_name(self) -> &'static str {
        match self {
            Effect::Speed => "speed",
            Effect::Slowness => "slowness",
            Effect::JumpBoost => "jump_boost",
            Effect::SlowFalling => "slow_falling",
            Effect::Levitation => "levitation",
            Effect::FireResistance => "fire_resistance",
        }
    }
}

/// `ticks` inputs with the same keys. Yaw and pitch carry over from the
/// previous segment unless set, then turn by `turn` and `tilt` each tick.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Segment {
    pub ticks: u32,
    /// Forward (W positive) and strafe (A positive), each in -1..=1.
    pub forward: f32,
    pub strafe: f32,
    pub jump: bool,
    pub sneak: bool,
    pub sprint: bool,
    /// Open the elytra on this segment's first tick.
    pub glide: bool,
    pub yaw: Option<f32>,
    pub pitch: Option<f32>,
    /// Degrees per tick.
    pub turn: f32,
    pub tilt: f32,
}

/// Something the server does between two inputs.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    /// After this tick's input (1-based).
    pub after: u32,
    /// The command the BDS runner sends.
    #[serde(default)]
    pub command: Option<String>,
    /// The velocity the server gives the player (a knockback), taken by the
    /// next input's move. For BDS it is what `command` causes, recorded from
    /// its `SetActorMotion`.
    #[serde(default)]
    pub velocity: Option<[f32; 3]>,
}

/// One tick's input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TickInput {
    pub controls: Controls,
    /// Open the elytra this tick.
    pub glide: bool,
}

impl Scenario {
    fn validate(&self) -> Result<()> {
        let inside = |p: [i32; 3]| (0..3).all(|a| p[a] >= AREA_MIN[a] && p[a] <= AREA_MAX[a]);
        for fill in &self.world {
            ensure!(
                inside(fill.from) && inside(fill.to),
                "fill {:?}..{:?} leaves the scenario area",
                fill.from,
                fill.to
            );
            let volume: i64 = (0..3)
                .map(|a| i64::from((fill.to[a] - fill.from[a]).abs() + 1))
                .product();
            ensure!(volume <= 32768, "fill of {volume} blocks (BDS takes 32768)");
            fill.block.resolve()?;
        }
        ensure!(
            inside(self.start.feet.map(|v| v.floor() as i32)),
            "start outside the scenario area"
        );
        ensure!(!self.inputs.is_empty(), "no inputs");
        // The BDS runner may send the first inputs before it sees the
        // teleport to the start; they go out idle.
        let lead = &self.inputs[0];
        ensure!(
            lead.ticks >= LEADING_IDLE
                && lead.forward == 0.0
                && lead.strafe == 0.0
                && !(lead.jump || lead.sneak || lead.sprint || lead.glide)
                && lead.turn == 0.0
                && lead.tilt == 0.0
                && lead.yaw.is_none()
                && lead.pitch.is_none(),
            "a scenario starts with at least {LEADING_IDLE} idle ticks"
        );
        let ticks = self.ticks();
        for event in &self.events {
            ensure!(
                event.after >= 1 && event.after < ticks,
                "event after tick {} of {ticks}",
                event.after
            );
            ensure!(
                event.command.is_some() || event.velocity.is_some(),
                "an event without a command or velocity"
            );
        }
        for segment in &self.inputs {
            ensure!(segment.ticks > 0, "empty segment");
            ensure!(
                (-1.0..=1.0).contains(&segment.forward) && (-1.0..=1.0).contains(&segment.strafe),
                "move vector outside -1..=1"
            );
        }
        Ok(())
    }

    /// The number of inputs.
    pub fn ticks(&self) -> u32 {
        self.inputs.iter().map(|s| s.ticks).sum()
    }

    /// The inputs, one per tick.
    pub fn tick_inputs(&self) -> Vec<TickInput> {
        let (mut yaw, mut pitch) = (self.start.yaw, self.start.pitch);
        let mut out = Vec::new();
        for segment in &self.inputs {
            yaw = segment.yaw.unwrap_or(yaw);
            pitch = segment.pitch.unwrap_or(pitch);
            for tick in 0..segment.ticks {
                yaw += segment.turn;
                pitch += segment.tilt;
                let glide = segment.glide && tick == 0;
                out.push(TickInput {
                    controls: Controls {
                        forward: segment.forward,
                        strafe: segment.strafe,
                        yaw,
                        pitch,
                        // A vanilla client opens the elytra on a jump press.
                        jump: segment.jump || glide,
                        sneak: segment.sneak,
                        sprint: segment.sprint,
                    },
                    glide,
                });
            }
        }
        out
    }

    /// The velocity the server sets after input `tick`, if any.
    pub fn velocity_after(&self, tick: u32) -> Option<[f32; 3]> {
        self.events
            .iter()
            .filter(|e| e.after == tick)
            .find_map(|e| e.velocity)
    }

    /// The commands to send after input `tick`.
    pub fn commands_after(&self, tick: u32) -> impl Iterator<Item = &str> {
        self.events
            .iter()
            .filter(move |e| e.after == tick)
            .filter_map(|e| e.command.as_deref())
    }

    /// The scenario's world for the simulation.
    pub fn world(&self) -> Result<World> {
        let mut placed = BTreeMap::new();
        for fill in &self.world {
            let state = fill.block.resolve()?.state;
            let (lo, hi) = corners(fill);
            for x in lo[0]..=hi[0] {
                for y in lo[1]..=hi[1] {
                    for z in lo[2]..=hi[2] {
                        placed.insert([x, y, z], state);
                    }
                }
            }
        }
        // An air block marks the area so the columns around it load even
        // for a scenario without blocks.
        let air = blocks::get("minecraft:air").context("air")?.min_state_id;
        placed.entry(AREA_MIN).or_insert(air);
        placed.entry(AREA_MAX).or_insert(air);
        World::with_blocks(placed.into_iter().map(|(pos, state)| (pos, state, 0)), 2)
    }

    /// The commands that build the scenario on BDS and give the player its
    /// effects and equipment, before the teleport to the start.
    pub fn setup_commands(&self) -> Result<Vec<String>> {
        let mut out = vec![format!(
            "/fill {} {} {} {} {} {} air",
            AREA_MIN[0], AREA_MIN[1], AREA_MIN[2], AREA_MAX[0], AREA_MAX[1], AREA_MAX[2]
        )];
        for fill in &self.world {
            let (lo, hi) = corners(fill);
            out.push(format!(
                "/fill {} {} {} {} {} {} {}",
                lo[0],
                lo[1],
                lo[2],
                hi[0],
                hi[1],
                hi[2],
                fill.block.resolve()?.command_text()
            ));
        }
        out.push("/effect @s clear".to_owned());
        for effect in &self.start.effects {
            out.push(format!(
                "/effect @s {} 100000 {} true",
                effect.effect.command_name(),
                effect.amplifier
            ));
        }
        out.push(format!(
            "/replaceitem entity @s slot.armor.chest 0 {}",
            if self.start.elytra { "elytra" } else { "air" }
        ));
        Ok(out)
    }

    /// The teleport to the start.
    pub fn start_command(&self) -> String {
        let [x, y, z] = self.start.feet;
        let mut text = String::new();
        let _ = write!(
            text,
            "/tp @s {x} {y} {z} {} {}",
            self.start.yaw, self.start.pitch
        );
        text
    }

    /// The level of `effect`, 1-based (0 when absent).
    pub fn effect_level(&self, effect: Effect) -> u8 {
        self.start
            .effects
            .iter()
            .filter(|e| e.effect == effect)
            .map(|e| e.amplifier + 1)
            .max()
            .unwrap_or(0)
    }

    /// The body at the start, at rest and on the ground: BDS's `/tp` leaves
    /// its copy on the ground (the teleport's `MovePlayer` says so too), even
    /// in the air, until the first move.
    pub fn start_body(&self) -> Body {
        let mut body = Body::at(self.start.feet);
        body.on_ground = true;
        body.jump_boost = self.effect_level(Effect::JumpBoost);
        body.slow_falling = self.effect_level(Effect::SlowFalling) > 0;
        body.levitation = self.effect_level(Effect::Levitation);
        body.set_walk_speed(physics::walk_speed(
            self.effect_level(Effect::Speed),
            self.effect_level(Effect::Slowness),
        ));
        body
    }
}

fn corners(fill: &Fill) -> ([i32; 3], [i32; 3]) {
    let lo = [0, 1, 2].map(|a| fill.from[a].min(fill.to[a]));
    let hi = [0, 1, 2].map(|a| fill.from[a].max(fill.to[a]));
    (lo, hi)
}

/// Who produced a trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Implementation {
    /// BDS 1.26.51, measured.
    Bds,
    /// `unastar_rules::movement`.
    Unastar,
    /// oomph-ac/bedsim.
    Bedsim,
}

/// `f32` values with their exact bit patterns: serialized as the shortest
/// decimal (for reading) and the hex bits (authoritative).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Exact<const N: usize>(pub [f32; N]);

#[derive(Serialize, Deserialize)]
struct ExactRepr {
    value: Vec<f32>,
    bits: Vec<String>,
}

impl<const N: usize> Serialize for Exact<N> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        ExactRepr {
            value: self.0.to_vec(),
            bits: self
                .0
                .iter()
                .map(|v| format!("{:08x}", v.to_bits()))
                .collect(),
        }
        .serialize(serializer)
    }
}

impl<'de, const N: usize> Deserialize<'de> for Exact<N> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let repr = ExactRepr::deserialize(deserializer)?;
        let bits: Vec<f32> = repr
            .bits
            .iter()
            .map(|b| u32::from_str_radix(b, 16).map(f32::from_bits))
            .collect::<Result<_, _>>()
            .map_err(D::Error::custom)?;
        let values: [f32; N] = bits
            .try_into()
            .map_err(|_| D::Error::custom(format!("expected {N} values")))?;
        if repr.value.len() != N
            || repr
                .value
                .iter()
                .zip(values)
                .any(|(v, b)| v.to_bits() != b.to_bits() && !(v.is_nan() && b.is_nan()))
        {
            return Err(D::Error::custom("value and bits disagree"));
        }
        Ok(Exact(values))
    }
}

/// The state after one input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TickState {
    /// 1-based input number in the scenario.
    pub tick: u32,
    /// The wire position (feet plus 1.62001).
    pub position: Exact<3>,
    /// Velocity carried into the next tick. BDS's is known only on the ticks
    /// it sent a correction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity: Option<Exact<3>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_ground: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizontal_collision: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_collision: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fall_distance: Option<Exact<1>>,
}

impl From<(u32, &Body)> for TickState {
    fn from((tick, body): (u32, &Body)) -> Self {
        TickState {
            tick,
            position: Exact(body.network_position()),
            velocity: Some(Exact(body.velocity)),
            on_ground: Some(body.on_ground),
            horizontal_collision: Some(body.horizontal_collision),
            vertical_collision: Some(body.vertical_collision),
            fall_distance: None,
        }
    }
}

/// One implementation's states for every scenario it ran.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Traces {
    pub implementation: Implementation,
    /// What produced it (version, pinned commit, capture).
    pub source: String,
    pub scenarios: BTreeMap<String, ScenarioTrace>,
}

/// One scenario's states, and what an implementation noted about the run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScenarioTrace {
    pub ticks: Vec<TickState>,
    /// Velocities the server set (BDS: its `SetActorMotion`), by the tick
    /// they follow.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub velocity_events: BTreeMap<u32, Exact<3>>,
    /// The `minecraft:movement` values BDS sent during the scenario.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub movement_speeds: Vec<Exact<1>>,
}

/// Where the BDS runner played a scenario: the subject's runtime id and the
/// input tick of the scenario's first input (the inputs after it follow one
/// tick apart). The scripted client writes these (`--scenario-log`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenarioRun {
    pub name: String,
    pub runtime_id: u64,
    pub first_input: u64,
    pub ticks: u32,
}

/// Play `scenario` through `unastar_rules::movement`.
pub fn simulate(scenario: &Scenario) -> Result<ScenarioTrace> {
    let world = scenario.world()?;
    let mut body = scenario.start_body();
    let mut trace = ScenarioTrace::default();
    for (tick, input) in (1u32..).zip(scenario.tick_inputs()) {
        if let Some(velocity) = scenario.velocity_after(tick - 1) {
            body.velocity = velocity;
        }
        if input.glide && !body.on_ground && !body.gliding && !body.in_water {
            body.gliding = true;
        }
        let changes = physics::step(&mut body, &input.controls, &world);
        if changes.frozen {
            bail!("tick {tick}: the scenario world is not loaded around the player");
        }
        trace.ticks.push(TickState::from((tick, &body)));
    }
    for event in &scenario.events {
        if let Some(velocity) = event.velocity {
            trace.velocity_events.insert(event.after, Exact(velocity));
        }
    }
    Ok(trace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_specs_resolve_and_spell_every_state() {
        let slab = BlockSpec {
            name: "minecraft:smooth_stone_slab".into(),
            states: [(
                "minecraft:vertical_half".to_owned(),
                StateJson::Str("top".into()),
            )]
            .into(),
        }
        .resolve()
        .unwrap();
        assert_eq!(
            slab.command_text(),
            "smooth_stone_slab [\"minecraft:vertical_half\"=\"top\"]"
        );
        let stone = BlockSpec {
            name: "minecraft:stone".into(),
            states: BTreeMap::new(),
        };
        assert_eq!(stone.resolve().unwrap().command_text(), "stone");
    }

    #[test]
    fn exact_values_round_trip_through_their_bits() {
        let exact = Exact([0.1f32, -0.0, 1.620_01]);
        let text = serde_json::to_string(&exact).unwrap();
        let back: Exact<3> = serde_json::from_str(&text).unwrap();
        assert_eq!(back.0.map(f32::to_bits), exact.0.map(f32::to_bits));
    }
}
