//! Random movement scenarios for the differential (`movement-diff random`):
//! a seeded generator of small block worlds, from a list of blocks the
//! corpus already confirmed BDS places as written, and of input sequences.
//! The same seed always gives the same scenario (SplitMix64, no external
//! generator whose stream could change with a dependency update).

use serde_json::{Value, json};

/// SplitMix64: a small, fixed pseudo-random stream.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// A number in `0..n`.
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

/// Blocks a random world is built from: their states are ones BDS keeps as
/// placed (no neighbour-computed connections, nothing that falls, flows or
/// needs support: the first batch of 150 seeds placed carpets and snow in
/// the air, and the setup check rejected 48 of them), each as
/// `(name, states)`.
const BLOCKS: &[(&str, &str)] = &[
    ("minecraft:stone", "{}"),
    ("minecraft:glass", "{}"),
    ("minecraft:smooth_stone_slab", "{}"),
    (
        "minecraft:smooth_stone_slab",
        r#"{"minecraft:vertical_half": "top"}"#,
    ),
    ("minecraft:oak_stairs", r#"{"weirdo_direction": 0}"#),
    ("minecraft:oak_stairs", r#"{"weirdo_direction": 2}"#),
    ("minecraft:ice", "{}"),
    ("minecraft:packed_ice", "{}"),
    ("minecraft:blue_ice", "{}"),
    ("minecraft:slime", "{}"),
    ("minecraft:honey_block", "{}"),
    ("minecraft:soul_sand", "{}"),
    ("minecraft:trapdoor", "{}"),
    ("minecraft:web", "{}"),
];

/// Floors a random world stands on.
const FLOORS: &[&str] = &[
    "minecraft:stone",
    "minecraft:ice",
    "minecraft:slime",
    "minecraft:soul_sand",
    "minecraft:blue_ice",
];

/// The scenario of `seed`, as the corpus writes it.
pub fn scenario(seed: u64) -> Value {
    let mut rng = Rng(seed);
    let floor = *rng.pick(FLOORS);
    let mut world = vec![json!({
        "from": [-8, 99, -8], "to": [8, 99, 40], "block": {"name": floor}
    })];
    // A few boxes of blocks around the player's path.
    for _ in 0..rng.below(7) + 2 {
        let (name, states) = *rng.pick(BLOCKS);
        let x = rng.below(9) as i64 - 4;
        let z = rng.below(10) as i64 + 1;
        let y = 100 + rng.below(3) as i64;
        let to = [
            x + rng.below(3) as i64,
            y + rng.below(2) as i64 * i64::from(rng.chance(30)),
            z + rng.below(3) as i64,
        ];
        let states: Value = serde_json::from_str(states).unwrap_or_else(|_| json!({}));
        world.push(json!({
            "from": [x, y, z], "to": to, "block": {"name": name, "states": states}
        }));
    }
    let mut inputs = vec![json!({"ticks": 3 + rng.below(3)})];
    let mut yaw = 0.0f32;
    for _ in 0..rng.below(6) + 5 {
        let forward = *rng.pick(&[1.0f32, 1.0, 1.0, 0.0, -1.0, 0.5]);
        let strafe = *rng.pick(&[0.0f32, 0.0, 1.0, -1.0, 0.3]);
        let mut segment = json!({
            "ticks": rng.below(12) + 3,
            "forward": forward,
            "strafe": strafe,
            "jump": rng.chance(35),
            "sneak": rng.chance(20),
            "sprint": rng.chance(40),
        });
        if rng.chance(30) {
            yaw += (rng.below(25) as f32 - 12.0) * 7.5;
            segment["yaw"] = json!(yaw);
        }
        if rng.chance(20) {
            segment["turn"] = json!((rng.below(11) as f32 - 5.0) * 1.5);
        }
        inputs.push(segment);
    }
    inputs.push(json!({"ticks": 10}));
    json!({
        "name": format!("random_{seed}"),
        "description": format!("Random scenario {seed} (movement-diff random)."),
        "world": world,
        "start": {"feet": [0.5, 100, -2.5]},
        "inputs": inputs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Corpus;

    #[test]
    fn random_scenarios_are_valid_and_the_same_for_a_seed() {
        let scenarios: Vec<Value> = (1..=50).map(scenario).collect();
        let text = json!({ "scenarios": scenarios }).to_string();
        Corpus::parse(&text).unwrap();
        assert_eq!(scenario(7), scenario(7));
        assert_ne!(scenario(7), scenario(8));
    }
}
