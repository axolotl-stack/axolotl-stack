//! The Rust side of the three-way movement harness (`tools/movement-diff`).
//!
//! ```text
//! movement-diff simulate --corpus CORPUS --out unastar.json
//! movement-diff extract  --corpus CORPUS --run DIR --out bds.json
//! movement-diff resolve  --corpus CORPUS --out resolved.json
//! ```
//!
//! `resolve` writes the corpus with every block state spelled out (BDS
//! 1.26.51's states, from `unastar-data`), for the bedsim runner.
//! `simulate` plays every scenario through `unastar_rules::movement`.
//! `extract` reads a BDS run (`tools/movement-diff/bds_run.py`): the
//! viewer's capture holds BDS's copy of the subject after every input (one
//! `MovePlayer` relay per input: position and ground contact), the subject's
//! capture the corrections BDS sent it (position, velocity and ground
//! contact, about one every six inputs because the subject reports a
//! position 3 blocks off on every axis) and the `SetActorMotion` and movement-speed
//! updates. Each correction is checked against the relay of its input.

use anyhow::{Context, Result, bail, ensure};
use bds_sweep::capture::{self, Direction};
use bds_sweep::scenario::{
    BlockSpec, Corpus, Exact, Implementation, ScenarioRun, ScenarioTrace, TickState, Traces,
    simulate,
};
use clap::{Parser, Subcommand};
use jolyne::valentine::{EnumsPlayerPositionModeComponentPositionMode, McpePacketData};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(about = "Movement scenarios through unastar's simulation, and BDS runs extracted")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Play the corpus through `unastar_rules::movement`.
    Simulate {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Write the corpus with every block's full state.
    Resolve {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Turn a BDS run's captures into traces.
    Extract {
        #[arg(long)]
        corpus: PathBuf,
        /// The run directory: `scenarios.json`, `subject/packets.jsonl`,
        /// `viewer/packets.jsonl`.
        #[arg(long)]
        run: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

fn main() -> Result<()> {
    match Args::parse().command {
        Command::Simulate { corpus, out } => {
            let corpus = read_corpus(&corpus)?;
            let mut traces = Traces {
                implementation: Implementation::Unastar,
                source: "unastar_rules::movement".to_owned(),
                scenarios: BTreeMap::new(),
            };
            for scenario in &corpus.scenarios {
                let trace =
                    simulate(scenario).with_context(|| format!("scenario {}", scenario.name))?;
                traces.scenarios.insert(scenario.name.clone(), trace);
            }
            write(&out, &traces)
        }
        Command::Resolve { corpus, out } => {
            let text = std::fs::read_to_string(&corpus)
                .with_context(|| format!("reading {}", corpus.display()))?;
            Corpus::parse(&text)?;
            let mut value: serde_json::Value = serde_json::from_str(&text)?;
            for scenario in value["scenarios"]
                .as_array_mut()
                .context("scenarios is not a list")?
            {
                for fill in scenario["world"]
                    .as_array_mut()
                    .context("world is not a list")?
                {
                    let spec: BlockSpec = serde_json::from_value(fill["block"].clone())?;
                    fill["block"]["states"] = serde_json::to_value(spec.resolve()?.states())?;
                }
            }
            std::fs::write(
                &out,
                serde_json::to_string_pretty(&value)?
                    + "
",
            )
            .with_context(|| format!("writing {}", out.display()))
        }
        Command::Extract { corpus, run, out } => {
            let corpus = read_corpus(&corpus)?;
            let traces = extract(&corpus, &run)?;
            write(&out, &traces)
        }
    }
}

fn read_corpus(path: &Path) -> Result<Corpus> {
    Corpus::parse(
        &std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
    )
}

fn write(path: &Path, traces: &Traces) -> Result<()> {
    std::fs::write(path, serde_json::to_string_pretty(traces)? + "\n")
        .with_context(|| format!("writing {}", path.display()))
}

/// What BDS said about the subject after one input.
#[derive(Debug, Default)]
struct Observed {
    relay: Option<([f32; 3], bool)>,
    correction: Option<([f32; 3], [f32; 3], bool)>,
}

fn extract(corpus: &Corpus, run: &Path) -> Result<Traces> {
    let runs: Vec<ScenarioRun> = serde_json::from_str(
        &std::fs::read_to_string(run.join("scenarios.json")).context("reading scenarios.json")?,
    )?;
    let runtime_id = runs.first().context("no scenario was played")?.runtime_id;
    let mut by_tick: BTreeMap<u64, Observed> = BTreeMap::new();
    let mut motions: BTreeMap<u64, [f32; 3]> = BTreeMap::new();
    let mut speeds: Vec<(u64, f32)> = Vec::new();
    for record in capture::records(&run.join("viewer").join("packets.jsonl"))? {
        if record.dir != Direction::S2c || !record.is("MovePlayer") {
            continue;
        }
        let McpePacketData::MovePlayerPacket(packet) = record.decode()? else {
            bail!("record {} is not a MovePlayer", record.seq);
        };
        if packet.player_runtime_id.actor_runtime_id != runtime_id
            || packet.position_mode != EnumsPlayerPositionModeComponentPositionMode::Normal
        {
            continue;
        }
        let tick = packet.tick.inputtick;
        let position = [packet.position.x, packet.position.y, packet.position.z];
        let observed = by_tick.entry(tick).or_default();
        ensure!(
            observed.relay.is_none(),
            "two relays of input {tick} (record {})",
            record.seq
        );
        observed.relay = Some((position, packet.on_ground));
    }
    for record in capture::records(&run.join("subject").join("packets.jsonl"))? {
        if record.dir != Direction::S2c {
            continue;
        }
        let wanted = [
            "CorrectPlayerMovePrediction",
            "SetActorMotion",
            "UpdateAttributes",
        ];
        if !wanted.iter().any(|name| record.is(name)) {
            continue;
        }
        match record.decode()? {
            McpePacketData::CorrectPlayerMovePredictionPacket(packet) => {
                let tick = packet.tick.inputtick;
                let observed = by_tick.entry(tick).or_default();
                observed.correction = Some((
                    [packet.pos.x, packet.pos.y, packet.pos.z],
                    [packet.pos_delta.x, packet.pos_delta.y, packet.pos_delta.z],
                    packet.on_ground,
                ));
            }
            McpePacketData::SetActorMotionPacket(packet)
                if packet.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                motions.insert(
                    packet.tick.inputtick,
                    [packet.motion.x, packet.motion.y, packet.motion.z],
                );
            }
            McpePacketData::UpdateAttributesPacket(packet)
                if packet.target_runtime_id.actor_runtime_id == runtime_id =>
            {
                for attribute in &packet.attribute_list {
                    if attribute.name == "minecraft:movement" {
                        speeds.push((packet.tick.inputtick, attribute.current_value));
                    }
                }
            }
            _ => {}
        }
    }
    let mut traces = Traces {
        implementation: Implementation::Bds,
        source: format!(
            "BDS 1.26.51, run {}",
            run.file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        ),
        scenarios: BTreeMap::new(),
    };
    for played in &runs {
        let scenario = corpus
            .get(&played.name)
            .with_context(|| format!("{} is not in the corpus", played.name))?;
        ensure!(
            played.ticks == scenario.ticks(),
            "{} was played with {} inputs, the corpus has {}",
            played.name,
            played.ticks,
            scenario.ticks()
        );
        let mut trace = ScenarioTrace::default();
        let first = played.first_input;
        let last = first + u64::from(played.ticks) - 1;
        for (number, tick) in (1u32..).zip(first..=last) {
            let observed = by_tick.get(&tick);
            let Some((position, on_ground)) = observed.and_then(|o| o.relay) else {
                bail!("{}: no relay of input {tick} (tick {number})", played.name);
            };
            let correction = observed.and_then(|o| o.correction);
            if let Some((corrected, _, corrected_ground)) = correction {
                ensure!(
                    corrected.map(f32::to_bits) == position.map(f32::to_bits)
                        && corrected_ground == on_ground,
                    "{} tick {number}: the correction {corrected:?} {corrected_ground} \
                     and the relay {position:?} {on_ground} of input {tick} disagree",
                    played.name
                );
            }
            trace.ticks.push(TickState {
                tick: number,
                position: Exact(position),
                velocity: correction.map(|(_, velocity, _)| Exact(velocity)),
                on_ground: Some(on_ground),
                horizontal_collision: None,
                vertical_collision: None,
                fall_distance: None,
            });
        }
        for (tick, motion) in motions.range(first..=last) {
            trace
                .velocity_events
                .insert(u32::try_from(tick - first + 1)?, Exact(*motion));
        }
        trace.movement_speeds = speeds
            .iter()
            .filter(|(tick, _)| (first..=last).contains(tick))
            .map(|(_, speed)| Exact([*speed]))
            .collect();
        traces.scenarios.insert(played.name.clone(), trace);
    }
    Ok(traces)
}
