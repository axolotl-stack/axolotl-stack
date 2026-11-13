//! Checks that a BDS run of the movement corpus played exactly what the
//! corpus says (`tools/movement-diff/README.md`, "Harness validation"):
//!
//! - **Setup**: the world BDS sent the subject (its chunks and block
//!   updates, replayed from the subject's capture) is the scenario's world
//!   in the whole scenario area, both layers, when the scenario's first
//!   input goes out: every position holds the same block with the same
//!   collision boxes and liquid (a state that differs only in what does not
//!   move a player, such as a plant's age, is counted but passes); so do the
//!   effects BDS reports
//!   (`MobEffect`), the chest slot (an elytra or nothing) and the start
//!   teleport's position and rotation (`MovePlayer`).
//! - **Inputs**: every `PlayerAuthInput` the subject sent for a scenario
//!   input carries the corpus input: the move vector, the rotation and the
//!   held keys, one input per tick.
//! - **Relays**: the viewer saw exactly one relay of each input.
//!
//! It also keeps the input flags the subject sent, so runs with different
//! flag derivations can be compared.

use crate::capture::{Direction, Record};
use crate::scenario::{AREA_MAX, AREA_MIN, BlockState, Corpus, Scenario, ScenarioRun};
use crate::world::World;
use anyhow::{Context, Result};
use jolyne::valentine::types::EnumsPlayerAuthInputPacketPayloadInputData as Flag;
use jolyne::valentine::{EnumsMobEffectPacketPayloadEvent, McpePacketData, PlayerAuthInputPacket};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The armour container, and its chest slot.
const ARMOUR_CONTAINER: u32 = 120;
const CHEST_SLOT: usize = 1;
/// Differences listed per check before the rest are only counted.
const SHOWN: usize = 12;

/// What the checks found in one run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunValidation {
    pub scenarios: BTreeMap<String, ScenarioValidation>,
}

impl RunValidation {
    /// The scenarios with a failed check.
    pub fn failures(&self) -> Vec<&str> {
        self.scenarios
            .iter()
            .filter(|(_, v)| !v.passed())
            .map(|(name, _)| name.as_str())
            .collect()
    }
}

/// One scenario's checks.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScenarioValidation {
    /// Where BDS's setup differs from the corpus.
    pub setup: Vec<String>,
    /// Where a sent input differs from the corpus.
    pub inputs: Vec<String>,
    /// Relays of the scenario's inputs the viewer received (one per input).
    pub relays: u32,
    pub ticks: u32,
    /// The input flags sent on each input.
    pub flags: Vec<Vec<String>>,
}

impl ScenarioValidation {
    pub fn passed(&self) -> bool {
        self.setup.is_empty() && self.inputs.is_empty() && self.relays == self.ticks
    }
}

/// What the subject knew from BDS while it played.
#[derive(Default)]
struct Seen {
    world: World,
    /// Effect id to amplifier.
    effects: BTreeMap<i32, i32>,
    chest: Option<bool>,
    /// The last teleport of the subject: position, pitch, yaw.
    teleport: Option<([f32; 3], f32, f32)>,
    /// The input tick the last teleport was stamped with.
    teleport_tick: Option<u64>,
    /// Every teleport's input tick.
    teleports: Vec<u64>,
}

impl Seen {
    fn apply(&mut self, packet: McpePacketData, runtime_id: u64) -> Result<()> {
        match packet {
            McpePacketData::LevelChunkPacket(packet) => {
                self.world.level_chunk(&packet, [0.0; 3])?;
            }
            McpePacketData::SubChunkPacket(packet) => self.world.sub_chunk(&packet)?,
            McpePacketData::UpdateBlockPacket(packet) => {
                let pos = &packet.block_position;
                self.world.set_block(
                    [pos.x, pos.y, pos.z],
                    packet.block_runtime_id,
                    packet.layer as usize,
                )?;
            }
            McpePacketData::UpdateSubChunkBlocksPacket(packet) => {
                for (layer, entries) in [
                    packet.blocks_changed.blocks_changed_standards,
                    packet.blocks_changed.blocks_changed_extras,
                ]
                .into_iter()
                .enumerate()
                {
                    for entry in entries {
                        let pos = &entry.pos;
                        self.world
                            .set_block([pos.x, pos.y, pos.z], entry.runtime_id, layer)?;
                    }
                }
            }
            McpePacketData::MobEffectPacket(packet)
                if packet.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                if packet.event_id == EnumsMobEffectPacketPayloadEvent::Remove {
                    self.effects.remove(&packet.effect_id);
                } else {
                    self.effects
                        .insert(packet.effect_id, packet.effect_amplifier);
                }
            }
            McpePacketData::MovePlayerPacket(packet)
                if packet.player_runtime_id.actor_runtime_id == runtime_id =>
            {
                self.teleport = Some((
                    [packet.position.x, packet.position.y, packet.position.z],
                    packet.rotation.x,
                    packet.rotation.y,
                ));
                self.teleport_tick = Some(packet.tick.inputtick);
                self.teleports.push(packet.tick.inputtick);
            }
            McpePacketData::InventoryContentPacket(packet)
                if packet.container_id == ARMOUR_CONTAINER =>
            {
                self.chest = packet.slots.get(CHEST_SLOT).map(|item| item.id != 0);
            }
            McpePacketData::InventorySlotPacket(packet)
                if u32::from(packet.container_id) == ARMOUR_CONTAINER
                    && packet.slot as usize == CHEST_SLOT =>
            {
                self.chest = Some(packet.item.id != 0);
            }
            _ => {}
        }
        Ok(())
    }

    /// The setup's differences from `scenario`.
    fn check_setup(&self, scenario: &Scenario) -> Result<Vec<String>> {
        let mut out = Vec::new();
        let expected = scenario.placed_blocks()?;
        let air = scenario_air()?;
        let mut differ = Vec::new();
        let mut cosmetic = 0;
        for x in AREA_MIN[0]..=AREA_MAX[0] {
            for y in AREA_MIN[1]..=AREA_MAX[1] {
                for z in AREA_MIN[2]..=AREA_MAX[2] {
                    let pos = [x, y, z];
                    for layer in 0..2 {
                        let want = expected.get(&(pos, layer)).copied().unwrap_or(air);
                        let layer = usize::from(layer);
                        let got = self.world.layer(pos, layer).map(|b| b.state);
                        match got {
                            Some(got) if got == want => {}
                            Some(got) if same_to_movement(got, want) => cosmetic += 1,
                            _ => differ.push((pos, layer, want, got)),
                        }
                    }
                }
            }
        }
        if !differ.is_empty() {
            let name = |state: u32| {
                unastar_data::blocks::by_runtime_id(state).map_or_else(
                    || state.to_string(),
                    |data| {
                        let states = BlockState { state, data }.states();
                        if states.is_empty() {
                            data.identifier.to_owned()
                        } else {
                            format!("{} {}", data.identifier, serde_json::json!(states))
                        }
                    },
                )
            };
            let shown: Vec<String> = differ
                .iter()
                .take(SHOWN)
                .map(|(pos, layer, want, got)| {
                    format!(
                        "{pos:?} layer {layer}: {} instead of {}",
                        got.map_or_else(|| "not loaded".to_owned(), name),
                        name(*want)
                    )
                })
                .collect();
            out.push(format!(
                "{} blocks differ from the corpus: {}",
                differ.len(),
                shown.join("; ")
            ));
        }
        if cosmetic > 0 {
            tracing::info!(
                scenario = scenario.name,
                cosmetic,
                "block states that differ only in what does not move a player"
            );
        }
        let wanted: BTreeMap<i32, i32> = scenario
            .start
            .effects
            .iter()
            .map(|e| (e.effect.id(), i32::from(e.amplifier)))
            .collect();
        if wanted != self.effects {
            out.push(format!(
                "effects {:?} instead of {wanted:?} (id: amplifier)",
                self.effects
            ));
        }
        if self.chest != Some(scenario.start.elytra) {
            out.push(format!(
                "chest slot occupied {:?}, the corpus gives an elytra: {}",
                self.chest, scenario.start.elytra
            ));
        }
        let [x, y, z] = scenario.start.feet;
        let position = [x, y + crate::physics::NETWORK_OFFSET, z];
        match self.teleport {
            Some((at, pitch, yaw))
                if at.map(f32::to_bits) == position.map(f32::to_bits)
                    && pitch.to_bits() == scenario.start.pitch.to_bits()
                    && yaw.to_bits() == scenario.start.yaw.to_bits() => {}
            other => out.push(format!(
                "start teleport {other:?} instead of {position:?}, pitch {}, yaw {}",
                scenario.start.pitch, scenario.start.yaw
            )),
        }
        Ok(out)
    }
}

/// Whether two states of one block are the same to a moving player: the
/// same collision boxes, friction, movement family and liquid.
fn same_to_movement(a: u32, b: u32) -> bool {
    let block = unastar_data::blocks::by_runtime_id;
    let boxes = |state| unastar_data::shapes::collision(state).and_then(|s| s.boxes());
    block(a).is_some_and(|data| block(b).is_some_and(|other| data.identifier == other.identifier))
        && boxes(a) == boxes(b)
        && unastar_data::motion::block(a) == unastar_data::motion::block(b)
}

fn scenario_air() -> Result<u32> {
    Ok(unastar_data::blocks::get("minecraft:air")
        .context("air")?
        .min_state_id)
}

/// The differences between the input the subject sent and `tick` of the
/// corpus.
fn check_input(
    sent: &PlayerAuthInputPacket,
    expected: &crate::scenario::TickInput,
    number: u32,
) -> Vec<String> {
    let controls = &expected.controls;
    let has = |flag| sent.input_data.contains(&flag);
    let mut out = Vec::new();
    let bits = |v: &jolyne::valentine::types::Vec2| [v.x.to_bits(), v.y.to_bits()];
    let keys = [controls.strafe.to_bits(), controls.forward.to_bits()];
    if bits(&sent.move_vector) != keys || bits(&sent.raw_move_vector) != keys {
        out.push(format!(
            "tick {number}: move vector {:?} (raw {:?}) instead of strafe {} forward {}",
            sent.move_vector, sent.raw_move_vector, controls.strafe, controls.forward
        ));
    }
    if bits(&sent.player_rotation) != [controls.pitch.to_bits(), controls.yaw.to_bits()] {
        out.push(format!(
            "tick {number}: rotation {:?} instead of pitch {} yaw {}",
            sent.player_rotation, controls.pitch, controls.yaw
        ));
    }
    for (flag, held) in [
        (Flag::Jumpdown, controls.jump),
        (Flag::Sneakdown, controls.sneak),
        (Flag::Sprintdown, controls.sprint),
        (Flag::Up, controls.forward > 0.0),
        (Flag::Down, controls.forward < 0.0),
        (Flag::Left, controls.strafe > 0.0),
        (Flag::Right, controls.strafe < 0.0),
    ] {
        if has(flag) != held {
            out.push(format!(
                "tick {number}: {flag:?} is {} instead of {held}",
                has(flag)
            ));
        }
    }
    if has(Flag::Startgliding) && !expected.glide {
        out.push(format!("tick {number}: StartGliding without the glide key"));
    }
    out
}

/// Check every played scenario against the subject's capture
/// (`subject` records, in order) and the input ticks the viewer saw a relay
/// of (`relays`).
pub fn validate(
    corpus: &Corpus,
    runs: &[ScenarioRun],
    subject: &[Record],
    relays: &BTreeSet<u64>,
) -> Result<RunValidation> {
    let runtime_id = runs.first().map_or(0, |r| r.runtime_id);
    let mut out = RunValidation::default();
    let firsts: BTreeMap<u64, &ScenarioRun> = runs.iter().map(|r| (r.first_input, r)).collect();
    let mut inputs: BTreeMap<u64, Vec<PlayerAuthInputPacket>> = BTreeMap::new();
    let mut seen = Seen::default();
    for record in subject {
        let relevant = match record.dir {
            Direction::C2s => record.is("PlayerAuthInput"),
            Direction::S2c => [
                "LevelChunk",
                "SubChunk",
                "UpdateBlock",
                "UpdateSubChunkBlocks",
                "MobEffect",
                "MovePlayer",
                "InventoryContent",
                "InventorySlot",
            ]
            .iter()
            .any(|name| record.is(name)),
        };
        if !relevant {
            continue;
        }
        match record.decode()? {
            McpePacketData::PlayerAuthInputPacket(packet) => {
                let tick = packet.client_tick.inputtick;
                // The setup is complete once the start teleport arrived: the
                // scenario's first input is the second after it (an input
                // sent before the teleport arrived may carry the same tick).
                let started = seen
                    .teleport_tick
                    .and_then(|teleported| firsts.get(&(teleported + 2)))
                    .filter(|run| tick >= run.first_input);
                if let Some(run) = started
                    && !out.scenarios.contains_key(&run.name)
                {
                    let scenario = corpus
                        .get(&run.name)
                        .with_context(|| format!("{} is not in the corpus", run.name))?;
                    let setup = seen.check_setup(scenario)?;
                    out.scenarios.insert(
                        run.name.clone(),
                        ScenarioValidation {
                            setup,
                            ticks: run.ticks,
                            ..Default::default()
                        },
                    );
                }
                inputs.entry(tick).or_default().push(*packet);
            }
            other => seen.apply(other, runtime_id)?,
        }
    }
    for run in runs {
        let scenario = corpus
            .get(&run.name)
            .with_context(|| format!("{} is not in the corpus", run.name))?;
        let check = out
            .scenarios
            .entry(run.name.clone())
            .or_insert_with(|| ScenarioValidation {
                setup: vec!["the first input was never sent".to_owned()],
                ticks: run.ticks,
                ..Default::default()
            });
        let last = run.first_input + u64::from(run.ticks);
        if let Some(tick) = seen
            .teleports
            .iter()
            .find(|&&tick| tick >= run.first_input.saturating_sub(1) && tick < last)
        {
            check.setup.push(format!(
                "a teleport stamped with input {tick}, during the scenario"
            ));
        }
        for ((number, tick), expected) in
            (1u32..).zip(run.first_input..).zip(scenario.tick_inputs())
        {
            if relays.contains(&tick) {
                check.relays += 1;
            }
            match inputs.get(&tick).map(Vec::as_slice) {
                Some([sent]) => {
                    check.inputs.extend(check_input(sent, &expected, number));
                    check.flags.push(
                        sent.input_data
                            .iter()
                            .map(|flag| format!("{flag:?}"))
                            .collect(),
                    );
                }
                Some(many) => check
                    .inputs
                    .push(format!("tick {number}: {} inputs sent", many.len())),
                None => check.inputs.push(format!("tick {number}: no input sent")),
            }
        }
    }
    Ok(out)
}
