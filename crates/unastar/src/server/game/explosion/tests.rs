//! Explosions compared with BDS 1.26.51 (`data/measured/explosions/`):
//! the blocks TNT breaks by distribution (`tnt_blast.json`, 64 blasts per
//! material against seeded replays), the drops, and the damage mobs held
//! still at known spots took (`tnt_hits.json`).

use std::collections::HashMap;

use glam::{DVec3, IVec3};
use rand::SeedableRng;
use rand::rngs::StdRng;
use serde_json::Value;

use super::{blast_centre, blasted_blocks, damage, distance, exposure, impact};
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::collision::Aabb;

/// TNT's explosion power and its actor's height.
const TNT_POWER: f32 = 4.0;
const TNT_HEIGHT: f64 = 0.98;

fn registry() -> &'static BlockRegistry {
    BlockRegistry::vanilla()
}

fn id(name: &str) -> u32 {
    registry()
        .default_state_id_by_name(&format!("minecraft:{name}"))
        .unwrap_or_else(|| panic!("{name}"))
}

/// A sweep cell: the superflat floor (grass at y = -1, dirt, bedrock at -4,
/// void below) and the blocks set in it.
struct Cell(HashMap<IVec3, u32>);

impl Cell {
    fn new() -> Self {
        Self(HashMap::new())
    }

    fn set(&mut self, pos: IVec3, name: &str) {
        self.0.insert(pos, id(name));
    }
}

impl BlockSource for Cell {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        Some(self.0.get(&pos).copied().unwrap_or_else(|| match pos.y {
            -1 => id("grass_block"),
            -3..=-2 => id("dirt"),
            -4 => id("bedrock"),
            _ => id("air"),
        }))
    }

    fn registry(&self) -> &BlockRegistry {
        registry()
    }
}

fn rows(family: &str) -> Vec<Value> {
    let path = format!(
        "{}/../unastar-data/data/measured/explosions/{family}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut value: Value = serde_json::from_str(&text).expect(&path);
    match value["rows"].take() {
        Value::Array(rows) => rows,
        other => panic!("{path}: rows {other:?}"),
    }
}

fn offset(value: &Value) -> IVec3 {
    let v = value.as_array().expect("offset");
    IVec3::new(
        v[0].as_i64().unwrap() as i32,
        v[1].as_i64().unwrap() as i32,
        v[2].as_i64().unwrap() as i32,
    )
}

/// The blast cell of `material`: the cube with the TNT gone (it is the
/// primed actor now) and the redstone block beside it.
fn blast_cell(material: &str) -> Cell {
    let mut cell = Cell::new();
    for x in -4..=4 {
        for y in -3..=5 {
            for z in -4..=4 {
                cell.set(IVec3::new(x, y, z), material);
            }
        }
    }
    cell.set(IVec3::new(0, 1, 0), "air");
    cell.set(IVec3::new(1, 1, 0), "redstone_block");
    cell
}

/// The primed TNT's feet in the blast cell: the bottom centre of the TNT's
/// block (it cannot move in the cube).
const BLAST_FEET: DVec3 = DVec3::new(0.5, 1.0, 0.5);

/// Per cube position, how often it was destroyed over `samples`.
fn frequencies(samples: &[Vec<IVec3>]) -> HashMap<IVec3, f64> {
    let mut out: HashMap<IVec3, f64> = HashMap::new();
    for sample in samples {
        for pos in sample {
            *out.entry(*pos).or_default() += 1.0;
        }
    }
    for value in out.values_mut() {
        *value /= samples.len() as f64;
    }
    out
}

fn mean_and_sd(counts: &[f64]) -> (f64, f64) {
    let n = counts.len() as f64;
    let mean = counts.iter().sum::<f64>() / n;
    let var = counts.iter().map(|c| (c - mean).powi(2)).sum::<f64>() / (n - 1.0);
    (mean, var.sqrt())
}

/// Every material's blasts: the number of blocks destroyed has BDS's mean
/// (within four standard errors of the difference) and every cube position
/// is destroyed about as often (within 0.25 of BDS's 64-sample rate).
#[test]
fn tnt_breaks_blocks_like_bds_by_distribution() {
    let rows = rows("tnt_blast");
    let mut by_material: HashMap<String, Vec<Vec<IVec3>>> = HashMap::new();
    for row in &rows {
        let material = row[0].as_str().unwrap().to_string();
        let destroyed = row[3].as_array().unwrap().iter().map(offset).collect();
        by_material.entry(material).or_default().push(destroyed);
    }
    let mut failures = Vec::new();
    for (material, bds) in &by_material {
        let cell = blast_cell(material);
        let mut rng = StdRng::seed_from_u64(0x7e57);
        let ours: Vec<Vec<IVec3>> = (0..256)
            .map(|_| {
                let mut blasted = blasted_blocks(
                    &cell,
                    blast_centre(BLAST_FEET, TNT_HEIGHT),
                    TNT_POWER,
                    f32::MAX,
                    &mut rng,
                );
                // The TNT's own (now air) block and the redstone block are
                // air afterwards too, as in the sweep's reads.
                blasted.retain(|pos| *pos != IVec3::new(0, 1, 0));
                blasted.push(IVec3::new(0, 1, 0));
                blasted
                    .into_iter()
                    .filter(|pos| pos.abs().max_element() <= 4 && (-3..=5).contains(&pos.y))
                    .collect()
            })
            .collect();
        let count = |samples: &[Vec<IVec3>]| -> Vec<f64> {
            samples.iter().map(|s| s.len() as f64).collect()
        };
        let (bds_mean, bds_sd) = mean_and_sd(&count(bds));
        let (our_mean, our_sd) = mean_and_sd(&count(&ours));
        let se = (bds_sd.powi(2) / bds.len() as f64 + our_sd.powi(2) / ours.len() as f64).sqrt();
        if (bds_mean - our_mean).abs() > 4.0 * se.max(0.25) {
            failures.push(format!(
                "{material}: BDS destroys {bds_mean:.1} +- {bds_sd:.1}, unastar {our_mean:.1} +- {our_sd:.1}"
            ));
        }
        let (theirs, mine) = (frequencies(bds), frequencies(&ours));
        let mut worst = (0.0, IVec3::ZERO);
        for pos in theirs.keys().chain(mine.keys()) {
            let diff = (theirs.get(pos).unwrap_or(&0.0) - mine.get(pos).unwrap_or(&0.0)).abs();
            if diff > worst.0 {
                worst = (diff, *pos);
            }
        }
        if worst.0 > 0.25 {
            failures.push(format!(
                "{material}: at {} BDS destroys {:.2} of the time, unastar {:.2}",
                worst.1,
                theirs.get(&worst.1).unwrap_or(&0.0),
                mine.get(&worst.1).unwrap_or(&0.0)
            ));
        }
    }
    assert_eq!(by_material.len(), 4, "materials");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// TNT drops every block it breaks, as by hand without the correct tool
/// (cobblestone drops, glass does not): BDS's item counts equal the blocks
/// destroyed, less the TNT's position (and the redstone block, which drops
/// itself).
#[test]
fn tnt_drops_every_block_it_breaks_like_bds() {
    for row in rows("tnt_blast") {
        let material = row[0].as_str().unwrap();
        let destroyed = row[3].as_array().unwrap().len() as u64;
        let dropped = |item: &str| row[4][item].as_u64().unwrap_or(0);
        assert_eq!(dropped("redstone_block"), 1, "{row}");
        let expected = if material == "glass" {
            0
        } else {
            destroyed - 2
        };
        assert_eq!(dropped(material), expected, "{material}: {row}");
    }
}

/// One measured hit: the TNT's and the mob's positions at the explosion
/// and the health the mob lost.
struct Hit {
    name: Value,
    tnt: DVec3,
    mob: DVec3,
    kind: String,
    health: (f64, f64),
}

fn hits() -> Vec<Hit> {
    let mut out = Vec::new();
    for row in rows("tnt_hits") {
        let actors = row[5].as_array().unwrap();
        let entry_of = |kind: &str, seen: &Value| {
            seen.as_array()
                .unwrap()
                .iter()
                .find(|a| a[1] == kind)
                .cloned()
        };
        let pos = |a: &Value| {
            DVec3::new(
                a[2].as_f64().unwrap(),
                a[3].as_f64().unwrap(),
                a[4].as_f64().unwrap(),
            )
        };
        let kind = row[0][0].as_str().unwrap().to_string();
        let mut last_tnt = None;
        let mut boom = None;
        let mut before = None;
        let mut after = None;
        for entry in actors {
            let (t, seen) = (entry[0].as_u64().unwrap(), &entry[1]);
            match entry_of("tnt", seen) {
                Some(tnt) => last_tnt = Some(pos(&tnt)),
                None if last_tnt.is_some() && boom.is_none() => boom = Some(t),
                None => {}
            }
            let mob = entry_of(&kind, seen);
            match (boom, mob) {
                (None, Some(mob)) => before = Some(mob),
                (Some(_), mob) if after.is_none() => {
                    after = Some(mob.map_or(0.0, |m| m[8].as_f64().unwrap_or(0.0)))
                }
                _ => {}
            }
        }
        let (Some(tnt), Some(before), Some(after)) = (last_tnt, before, after) else {
            continue;
        };
        out.push(Hit {
            name: row[0].clone(),
            tnt,
            mob: pos(&before),
            health: (before[8].as_f64().unwrap(), after),
            kind,
        });
    }
    out
}

/// The cell of a hit row: the TNT gone, the redstone block north of it, a
/// stone under a raised mob, and the wall (`glass` or `stone` four high,
/// `low_stone` one block) a block before the mob.
fn hit_cell(name: &Value) -> Cell {
    let mut cell = Cell::new();
    cell.set(IVec3::new(0, 0, -1), "redstone_block");
    let spot = name[1].as_array().unwrap();
    let dx = spot[0].as_i64().unwrap() as i32;
    let dz = spot[1].as_i64().unwrap() as i32;
    if spot.len() > 2 {
        cell.set(IVec3::new(dx, 0, dz), "stone");
    }
    match name[2].as_str() {
        Some("low_stone") => cell.set(IVec3::new(dx - 1, 0, 0), "stone"),
        Some(wall) => {
            for y in 0..4 {
                cell.set(IVec3::new(dx - 1, y, 0), wall);
            }
        }
        None => {}
    }
    cell
}

/// Mobs held still around a TNT lose what the explosion's damage formula
/// gives from their measured spot (BDS shows health as a whole number, so
/// the loss is compared within one point; at most four hits within 2.5):
/// iron golems up to 6 blocks away,
/// in the open, on the diagonal, raised and behind walls; pigs either die
/// or, fully behind a wall, take the 1 the formula always deals.
#[test]
fn explosions_hurt_mobs_like_bds() {
    // Exposure sampling and the distance to a mob's box are not exact: a
    // few hits (golems a block or two away, a pig half behind a wall on a
    // raised block) come out up to 2.5 points off.
    const NEAR: f64 = 2.5;
    const MOST_NEAR: usize = 4;
    let mut failures = Vec::new();
    let mut near = Vec::new();
    let mut compared = 0;
    for hit in hits() {
        let (width, height) = match hit.kind.as_str() {
            "iron_golem" => (1.4, 2.9),
            "pig" => (0.9, 0.9),
            other => panic!("{other}"),
        };
        let cell = hit_cell(&hit.name);
        let aabb = Aabb::from_feet(hit.mob, width, height);
        let centre = blast_centre(hit.tnt, TNT_HEIGHT);
        let seen = exposure(&cell, centre, &aabb);
        let impact = impact(distance(centre, hit.mob, width), TNT_POWER, seen);
        let dealt = f64::from(damage(impact, TNT_POWER));
        let (before, after) = hit.health;
        let lost = before - after;
        compared += 1;
        let off = if after <= 0.0 {
            (before - dealt).max(0.0)
        } else {
            (dealt - lost).abs()
        };
        if off >= 1.0 {
            near.push(format!("{}: off by {off:.2}", hit.name));
        }
        if off >= NEAR {
            failures.push(format!(
                "{}: BDS lost {lost} ({before} -> {after}), unastar deals {dealt:.2} (exposure {seen:.3})",
                hit.name
            ));
        }
    }
    assert!(compared >= 50, "only {compared} hits compared");
    assert!(
        near.len() <= MOST_NEAR,
        "{} of {compared} hits are a point or more off:
{}",
        near.len(),
        near.join(
            "
"
        )
    );
    assert!(
        failures.is_empty(),
        "{} of {compared} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// TNT in a running server: power primes it in the circuit update (the
/// block becomes air and a primed TNT stands at its bottom centre, moving
/// off at 0.02 sideways and 0.2 up), and the primed TNT explodes 81 ticks
/// later, breaking the dirt beside it (BDS 1.26.51,
/// `tnt_prime.json`: the block goes at read 7 and the explosion shows at
/// read 88).
#[test]
fn powered_tnt_primes_and_explodes_81_ticks_later_like_bds() {
    use crate::entity::mob::MobKind;
    use crate::server::game::blocks::{block_at, set_block};
    use crate::world::chunk::Chunk;
    use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition};

    let mut server = crate::GameServer::with_config(crate::ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..crate::ServerConfig::default()
    });
    let (tx, mut rx) = tokio::sync::mpsc::channel(1 << 16);
    {
        let world = server.ecs.world_mut();
        world.insert_resource(crate::entity::EntityRandom::seeded(9));
        // A creative watcher far enough away not to be hurt.
        let watcher = world
            .spawn((
                crate::entity::components::Player,
                crate::entity::components::Position(DVec3::new(-12.0, 65.62, -12.0)),
                crate::entity::components::GameMode::Creative,
                crate::entity::components::PlayerSession::new(1, "watcher".into(), None, None, tx),
            ))
            .id();
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut column = Chunk::new(cx, cz);
                for x in 0..16 {
                    for z in 0..16 {
                        column.set_block(x, 63, z, id("grass_block"));
                    }
                }
                let mut viewers = crate::world::ecs::ChunkViewers::default();
                viewers.insert(watcher);
                let chunk = world.spawn((ChunkData::new(column), viewers)).id();
                world
                    .resource_mut::<ChunkManager>()
                    .insert(ChunkPosition::new(cx, cz), chunk);
            }
        }
        assert!(
            world
                .resource::<crate::entity::mob::MobRegistry>()
                .unsupported("minecraft:tnt")
                .is_none(),
            "primed TNT is spawnable"
        );
    }
    let tnt = IVec3::new(4, 64, 4);
    let dirt = IVec3::new(4, 64, 5);
    {
        let world = server.ecs.world_mut();
        set_block(world, tnt, id("tnt"), false);
        set_block(world, dirt, id("dirt"), false);
    }
    for _ in 0..4 {
        server.tick();
    }
    set_block(
        server.ecs.world_mut(),
        tnt + IVec3::X,
        id("redstone_block"),
        false,
    );
    let at = |world: &bevy_ecs::world::World, pos: IVec3| block_at(world, (pos.x, pos.y, pos.z));
    let primed = |world: &mut bevy_ecs::world::World| {
        world
            .query::<(bevy_ecs::entity::Entity, &MobKind)>()
            .iter(world)
            .find(|(_, kind)| kind.0.identifier.as_ref() == "minecraft:tnt")
            .map(|(entity, _)| entity)
    };
    let mut lit = None;
    for tick in 0..10 {
        server.tick();
        if at(server.ecs.world_mut(), tnt) == Some(id("air")) {
            lit = Some(tick);
            break;
        }
    }
    let lit = lit.expect("the redstone block primes the TNT");
    let world = server.ecs.world_mut();
    let actor = primed(world).expect("a primed TNT");
    let feet = world
        .get::<crate::entity::components::Position>(actor)
        .unwrap()
        .0;
    assert!(
        (feet - DVec3::new(4.5, 64.0, 4.5)).length() < 0.25,
        "{feet}"
    );
    let mut exploded = None;
    for tick in 1..=100 {
        server.tick();
        if primed(server.ecs.world_mut()).is_none() {
            exploded = Some(tick);
            break;
        }
    }
    assert_eq!(
        exploded,
        Some(81),
        "the fuse burns 81 ticks (primed at tick {lit})"
    );
    assert_eq!(
        at(server.ecs.world_mut(), dirt),
        Some(id("air")),
        "the dirt beside it breaks"
    );
    // What the watcher saw, as in the BDS capture of a TNT lit by a lever
    // (`testdata/bds-placements-tnt.json`): the fuse sound at the bottom
    // centre with data 0, the explosion's particles with the power as data
    // and its sound, both at the fitted centre, and the block particles.
    use jolyne::valentine::McpePacketData;
    let mut fuse = None;
    let mut particles = None;
    let mut sound = None;
    let mut block_particles = false;
    while let Ok(packet) = rx.try_recv() {
        match packet.data {
            McpePacketData::LevelEventPacket(event) if event.event_id == 1005 => {
                fuse = Some((event.position, event.data));
            }
            McpePacketData::LevelEventPacket(event) if event.event_id == 2025 => {
                particles = Some((event.position, event.data));
            }
            McpePacketData::LevelEventGenericPacket(event) if event.event_id == 2026 => {
                block_particles = true;
            }
            McpePacketData::LevelSoundEventPacket(event) if event.sound_event == "explode" => {
                sound = Some(event.position);
            }
            _ => {}
        }
    }
    let (fuse_at, fuse_data) = fuse.expect("the fuse sound");
    assert_eq!(
        (fuse_at.x, fuse_at.y, fuse_at.z, fuse_data),
        (4.5, 64.0, 4.5, 0)
    );
    let (centre, power) = particles.expect("the explosion particles");
    assert_eq!(power, 4);
    // A sixteenth of the TNT's height above its feet on the grass (BDS:
    // -59.938747 over a floor at -60).
    assert!(
        (f64::from(centre.y) - (64.0 + 0.98 / 16.0)).abs() < 1e-4,
        "{centre:?}"
    );
    let sound = sound.expect("the explosion sound");
    assert_eq!((sound.x, sound.y, sound.z), (centre.x, centre.y, centre.z));
    assert!(block_particles, "the block explosion particles");
}

/// A primed TNT first moves at 0.02 sideways in a random direction and 0.2
/// up, as unastar primes it; one primed by another's explosion explodes
/// 0.5 to 2 seconds later (its `from_explosion` fuse), the 24 BDS cells of
/// `tnt_prime.json` at both tick parities.
#[test]
fn primed_tnt_moves_off_and_chain_fuses_like_bds() {
    let mut chain = Vec::new();
    for row in rows("tnt_prime") {
        let mut first: HashMap<i64, (u64, [f64; 3])> = HashMap::new();
        let mut last: HashMap<i64, u64> = HashMap::new();
        for entry in row[5].as_array().unwrap() {
            let t = entry[0].as_u64().unwrap();
            for actor in entry[1].as_array().unwrap() {
                if actor[1] != "tnt" {
                    continue;
                }
                let id = actor[0].as_i64().unwrap();
                let v = [5, 6, 7].map(|i| actor[i].as_f64().unwrap());
                first.entry(id).or_insert((t, v));
                last.insert(id, t);
            }
        }
        for (id, (t, v)) in &first {
            let sideways = v[0].hypot(v[2]);
            assert!(
                (sideways - super::PRIME_SPEED).abs() < 0.0006,
                "{row}: {v:?}"
            );
            if *t < 20 {
                assert_eq!(v[1], super::PRIME_LIFT, "{row}");
            } else {
                // Chained: from the explosion's tick to its own.
                chain.push(last[id] + 1 - t);
            }
        }
    }
    assert_eq!(chain.len(), 8);
    assert!(
        chain.iter().all(|ticks| (10..=40).contains(ticks)),
        "{chain:?}"
    );
}
