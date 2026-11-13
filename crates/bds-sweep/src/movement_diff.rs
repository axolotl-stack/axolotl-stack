//! The Rust side of the three-way movement harness (`tools/movement-diff`).
//!
//! ```text
//! movement-diff simulate --corpus CORPUS --out unastar.json
//! movement-diff extract  --corpus CORPUS --run DIR --out bds.json
//! movement-diff resolve  --corpus CORPUS --out resolved.json
//! ```
//!
//! `resolve` writes the corpus with every block state spelled out and each
//! block's collision boxes (BDS 1.26.51's states and shapes, from
//! `unastar-data`), for the bedsim and acacia runners.
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
    BlockSpec, Corpus, DenseVelocities, DenseVelocity, Exact, HarnessCheck, HarnessRun,
    HarnessRuns, Implementation, Scenario, ScenarioRun, ScenarioTrace, TickState, Traces, simulate,
};
use bds_sweep::validation::{RunValidation, validate};
use clap::{Parser, Subcommand};
use jolyne::valentine::{EnumsPlayerPositionModeComponentPositionMode, McpePacketData};
use std::collections::{BTreeMap, BTreeSet};
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
    /// Write each scenario's placed blocks as the simulation sees them (but
    /// plain full cubes of friction 0.6), to compare with the other runners'
    /// worlds (`compare.py --worlds`).
    World {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Write the corpus with each scenario again under `NAME@K`, K more idle
    /// inputs before it, for each K of `--shifts`: BDS corrects every sixth
    /// input from the scenario's start, so the shifted copies show its
    /// velocity on the other inputs (`movement-diff velocities`).
    Shift {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long, value_delimiter = ',', num_args = 1)]
        shifts: Vec<u32>,
        /// Only these scenarios (all when empty).
        #[arg(long, value_delimiter = ',', num_args = 1)]
        only: Vec<String>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Merge the velocities of a shifted run (`movement-diff shift`) into
    /// one velocity per input for every scenario whose shifted copies move
    /// exactly as the original (positions and ground contact bit for bit).
    Velocities {
        /// The committed traces.
        #[arg(long)]
        base: PathBuf,
        /// The shifted run's traces.
        #[arg(long)]
        shifted: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Write a corpus of random scenarios (`random_SEED`), one per seed of
    /// `--first..=--last` (`bds_sweep::random`).
    Random {
        #[arg(long)]
        first: u64,
        #[arg(long)]
        last: u64,
        #[arg(long)]
        out: PathBuf,
    },
    /// Write the corpus with every block's full state and collision boxes.
    Resolve {
        #[arg(long)]
        corpus: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Record another BDS run of the corpus (a harness check) as one digest
    /// per scenario, and say which scenarios differ from `--against`.
    Check {
        /// The run's traces (`movement-diff extract`).
        #[arg(long)]
        traces: PathBuf,
        /// The committed traces to compare with.
        #[arg(long)]
        against: PathBuf,
        #[arg(long, value_enum)]
        check: CheckKind,
        #[arg(long, value_delimiter = ',', num_args = 1, allow_hyphen_values = true)]
        offset: Vec<f32>,
        #[arg(long, default_value = "simulation")]
        flag_source: String,
        /// The checks file to add the run to.
        #[arg(long)]
        into: PathBuf,
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

/// [`HarnessCheck`] on the command line.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum CheckKind {
    Repeat,
    Offset,
    Flags,
}

impl From<CheckKind> for HarnessCheck {
    fn from(kind: CheckKind) -> Self {
        match kind {
            CheckKind::Repeat => HarnessCheck::Repeat,
            CheckKind::Offset => HarnessCheck::Offset,
            CheckKind::Flags => HarnessCheck::Flags,
        }
    }
}

fn read_traces(path: &Path) -> Result<Traces> {
    serde_json::from_str(
        &std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?,
    )
    .with_context(|| format!("parsing {}", path.display()))
}

fn main() -> Result<()> {
    match Args::parse().command {
        Command::Check {
            traces,
            against,
            check,
            offset,
            flag_source,
            into,
        } => {
            let run = read_traces(&traces)?;
            let committed = read_traces(&against)?;
            let digests: BTreeMap<String, String> = run
                .scenarios
                .iter()
                .map(|(name, trace)| (name.clone(), trace.digest()))
                .collect();
            let mut same = 0;
            for (name, digest) in &digests {
                match committed.scenarios.get(name) {
                    Some(theirs) if theirs.digest() == *digest => same += 1,
                    Some(_) => println!("{name}: differs from the committed run"),
                    None => println!("{name}: not in the committed run"),
                }
            }
            println!("{same} of {} scenarios bit-identical", digests.len());
            let mut checks: HarnessRuns = if into.exists() {
                serde_json::from_str(&std::fs::read_to_string(&into)?)?
            } else {
                HarnessRuns::default()
            };
            let name = run.source.clone();
            checks.runs.retain(|r| r.run != name);
            checks.runs.push(HarnessRun {
                check: check.into(),
                run: name,
                offset: offset
                    .as_slice()
                    .try_into()
                    .context("--offset takes x,y,z")?,
                flag_source,
                digests,
                velocities: run
                    .scenarios
                    .iter()
                    .map(|(name, trace)| (name.clone(), trace.velocities()))
                    .collect(),
                differs: BTreeMap::new(),
            });
            std::fs::write(
                &into,
                serde_json::to_string_pretty(&checks)?
                    + "
",
            )
            .with_context(|| format!("writing {}", into.display()))
        }
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
        Command::Shift {
            corpus,
            shifts,
            only,
            out,
        } => {
            let text = std::fs::read_to_string(&corpus)
                .with_context(|| format!("reading {}", corpus.display()))?;
            Corpus::parse(&text)?;
            let mut value: serde_json::Value = serde_json::from_str(&text)?;
            let scenarios = value["scenarios"]
                .as_array()
                .context("scenarios is not a list")?
                .clone();
            let mut shifted = Vec::new();
            for scenario in scenarios {
                let name = scenario["name"]
                    .as_str()
                    .context("a scenario without a name")?;
                if !only.is_empty() && !only.iter().any(|o| o == name) {
                    continue;
                }
                for &k in &shifts {
                    let mut copy = scenario.clone();
                    copy["name"] = format!("{name}@{k}").into();
                    let lead = &mut copy["inputs"][0]["ticks"];
                    *lead = (lead.as_u64().context("ticks")? + u64::from(k)).into();
                    let events = copy.get_mut("events").and_then(|e| e.as_array_mut());
                    for event in events.into_iter().flatten() {
                        let after = &mut event["after"];
                        *after = (after.as_u64().context("after")? + u64::from(k)).into();
                    }
                    shifted.push(copy);
                }
            }
            value["scenarios"] = shifted.into();
            let text = serde_json::to_string_pretty(&value)? + "\n";
            Corpus::parse(&text)?;
            std::fs::write(&out, text).with_context(|| format!("writing {}", out.display()))
        }
        Command::Velocities { base, shifted, out } => {
            let base = read_traces(&base)?;
            let shifted = read_traces(&shifted)?;
            let dense = dense_velocities(&base, &shifted);
            for (name, outcome) in &dense.scenarios {
                match outcome {
                    DenseVelocity::Known { velocities } => println!(
                        "{name}: velocity on {} of {} inputs",
                        velocities.iter().filter(|v| v.is_some()).count(),
                        velocities.len()
                    ),
                    DenseVelocity::Diverged { reason } => println!("{name}: {reason}"),
                }
            }
            std::fs::write(&out, serde_json::to_string_pretty(&dense)? + "\n")
                .with_context(|| format!("writing {}", out.display()))
        }
        Command::Random { first, last, out } => {
            let scenarios: Vec<serde_json::Value> =
                (first..=last).map(bds_sweep::random::scenario).collect();
            let text =
                serde_json::to_string_pretty(&serde_json::json!({ "scenarios": scenarios }))?
                    + "
";
            Corpus::parse(&text)?;
            std::fs::write(&out, text).with_context(|| format!("writing {}", out.display()))
        }
        Command::World { corpus, out } => {
            let corpus = read_corpus(&corpus)?;
            let mut scenarios = serde_json::Map::new();
            for scenario in &corpus.scenarios {
                scenarios.insert(scenario.name.clone(), world_cells(scenario)?);
            }
            let value = serde_json::json!({ "runner": "unastar", "scenarios": scenarios });
            std::fs::write(
                &out,
                serde_json::to_string_pretty(&value)?
                    + "
",
            )
            .with_context(|| format!("writing {}", out.display()))
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
                // The runners have one block layer: the formed blocks of
                // the first layer join the placed ones, the second layer's
                // water is left out.
                let formed = scenario
                    .as_object_mut()
                    .and_then(|s| s.remove("formed"))
                    .unwrap_or_default();
                let world = scenario["world"]
                    .as_array_mut()
                    .context("world is not a list")?;
                world.extend(
                    formed
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|fill| fill["layer"].as_u64().unwrap_or(0) == 0)
                        .cloned(),
                );
                for fill in world {
                    let spec: BlockSpec = serde_json::from_value(fill["block"].clone())?;
                    let state = spec.resolve()?;
                    fill["block"]["states"] = serde_json::to_value(state.states())?;
                    fill["block"]["collision"] = serde_json::to_value(state.collision_boxes()?)?;
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
            let (traces, validation) = extract(&corpus, &run)?;
            let checks = run.join("validation.json");
            std::fs::write(
                &checks,
                serde_json::to_string_pretty(&validation)?
                    + "
",
            )
            .with_context(|| format!("writing {}", checks.display()))?;
            let failures = validation.failures();
            if !failures.is_empty() {
                // Kept for diagnosis only: these traces are not BDS's
                // simulation of the corpus.
                write(&out.with_extension("invalid.json"), &traces)?;
            }
            ensure!(
                failures.is_empty(),
                "the run did not play the corpus exactly ({}): {}",
                checks.display(),
                failures.join(", ")
            );
            write(&out, &traces)
        }
    }
}

/// The velocity after each input, merged from shifted runs.
fn dense_velocities(base: &Traces, shifted: &Traces) -> DenseVelocities {
    let mut out = DenseVelocities::default();
    for (name, trace) in &base.scenarios {
        let mut velocities: Vec<Option<Exact<3>>> =
            trace.ticks.iter().map(|state| state.velocity).collect();
        let mut diverged = None;
        let copies = shifted.scenarios.iter().filter_map(|(copy, t)| {
            let (original, k) = copy.rsplit_once('@')?;
            (original == name).then(|| Some((k.parse::<usize>().ok()?, t)))?
        });
        let mut any = false;
        for (k, copy) in copies {
            any = true;
            for (index, state) in trace.ticks.iter().enumerate() {
                let Some(theirs) = copy.ticks.get(index + k) else {
                    diverged = Some(format!("@{k} is shorter"));
                    break;
                };
                let bits = |v: [f32; 3]| v.map(f32::to_bits);
                if bits(theirs.position.0) != bits(state.position.0)
                    || theirs.on_ground != state.on_ground
                {
                    diverged = Some(format!(
                        "@{k} moves differently from tick {} (the leading idle inputs change the start)",
                        state.tick
                    ));
                    break;
                }
                if let Some(velocity) = theirs.velocity {
                    match velocities[index] {
                        Some(known) if bits(known.0) != bits(velocity.0) => {
                            diverged =
                                Some(format!("@{k} has another velocity at tick {}", state.tick));
                            break;
                        }
                        _ => velocities[index] = Some(velocity),
                    }
                }
            }
            if diverged.is_some() {
                break;
            }
        }
        if !any {
            continue;
        }
        out.scenarios.insert(
            name.clone(),
            match diverged {
                Some(reason) => DenseVelocity::Diverged { reason },
                None => DenseVelocity::Known { velocities },
            },
        );
    }
    out
}

/// The scenario's placed blocks as the simulation sees them: collision
/// boxes in world coordinates and friction, but plain full cubes of friction
/// 0.6.
fn world_cells(scenario: &Scenario) -> Result<serde_json::Value> {
    let world = scenario.world()?;
    let mut cells = serde_json::Map::new();
    let positions: std::collections::BTreeSet<[i32; 3]> = scenario
        .placed_blocks()?
        .into_keys()
        .map(|(pos, _)| pos)
        .collect();
    for pos in positions {
        let block = world.block(pos).context("a placed block is not loaded")?;
        let motion = unastar_data::motion::block(block.state);
        let boxes: Vec<[f32; 6]> = block
            .boxes(pos)?
            .iter()
            .map(|b| [b.min[0], b.min[1], b.min[2], b.max[0], b.max[1], b.max[2]])
            .collect();
        let p = pos.map(|v| v as f32);
        let full = [p[0], p[1], p[2], p[0] + 1.0, p[1] + 1.0, p[2] + 1.0];
        if motion.liquid.is_none() && motion.friction == 0.6 && boxes == [full] {
            continue;
        }
        cells.insert(
            format!("{},{},{}", pos[0], pos[1], pos[2]),
            serde_json::json!({
                "block": block.identifier(),
                "boxes": boxes,
                "friction": motion.friction,
            }),
        );
    }
    Ok(serde_json::Value::Object(cells))
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

fn extract(corpus: &Corpus, run: &Path) -> Result<(Traces, RunValidation)> {
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
    let subject = capture::records(&run.join("subject").join("packets.jsonl"))?;
    let relays: BTreeSet<u64> = by_tick
        .iter()
        .filter(|(_, o)| o.relay.is_some())
        .map(|(tick, _)| *tick)
        .collect();
    let validation = validate(corpus, &runs, &subject, &relays)?;
    for record in subject {
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
    Ok((traces, validation))
}
