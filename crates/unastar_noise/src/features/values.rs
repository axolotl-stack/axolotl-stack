//! Java value providers: `IntProvider`, `FloatProvider` and the height
//! provider helpers the generated placement code calls.

use super::random::{BitSource, WorldgenRandom};

/// Java `IntProvider`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IntProvider {
    Constant(i32),
    /// `Mth.randomBetweenInclusive(min, max)`.
    Uniform {
        min: i32,
        max: i32,
    },
    /// `min + nextInt(nextInt(max - min + 1) + 1)`.
    BiasedToBottom {
        min: i32,
        max: i32,
    },
    Clamped {
        source: &'static IntProvider,
        min: i32,
        max: i32,
    },
    /// `(int) clamp(mean + (float) nextGaussian() * deviation, min, max)`.
    ClampedNormal {
        mean: f32,
        deviation: f32,
        min: i32,
        max: i32,
    },
    WeightedList {
        entries: &'static [(IntProvider, i32)],
        total_weight: i32,
    },
}

impl IntProvider {
    /// `sample(random)`.
    pub fn sample<S: BitSource>(&self, rng: &mut WorldgenRandom<S>) -> i32 {
        match *self {
            IntProvider::Constant(v) => v,
            IntProvider::Uniform { min, max } => rng.next_int_between_inclusive(min, max),
            IntProvider::BiasedToBottom { min, max } => {
                let inner = rng.next_int_bounded(max - min + 1);
                min + rng.next_int_bounded(inner + 1)
            }
            IntProvider::Clamped { source, min, max } => source.sample(rng).clamp(min, max),
            IntProvider::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => {
                let v = mean + rng.next_gaussian() as f32 * deviation;
                v.clamp(min as f32, max as f32) as i32
            }
            IntProvider::WeightedList {
                entries,
                total_weight,
            } => {
                let item = weighted_pick(entries, total_weight, rng);
                item.sample(rng)
            }
        }
    }

    /// `getMinValue()`.
    pub fn min_value(&self) -> i32 {
        match *self {
            IntProvider::Constant(v) => v,
            IntProvider::Uniform { min, .. } | IntProvider::BiasedToBottom { min, .. } => min,
            IntProvider::Clamped { source, min, max } => source.min_value().clamp(min, max),
            IntProvider::ClampedNormal { min, .. } => min,
            IntProvider::WeightedList { entries, .. } => entries
                .iter()
                .map(|(p, _)| p.min_value())
                .min()
                .unwrap_or(0),
        }
    }

    /// `getMaxValue()`.
    pub fn max_value(&self) -> i32 {
        match *self {
            IntProvider::Constant(v) => v,
            IntProvider::Uniform { max, .. } | IntProvider::BiasedToBottom { max, .. } => max,
            IntProvider::Clamped { source, min, max } => source.max_value().clamp(min, max),
            IntProvider::ClampedNormal { max, .. } => max,
            IntProvider::WeightedList { entries, .. } => entries
                .iter()
                .map(|(p, _)| p.max_value())
                .max()
                .unwrap_or(0),
        }
    }
}

/// `WeightedList.getRandomOrThrow`: `nextInt(totalWeight)` walked over the
/// cumulative weights.
#[inline]
pub fn weighted_pick<'a, T, S: BitSource>(
    entries: &'a [(T, i32)],
    total_weight: i32,
    rng: &mut WorldgenRandom<S>,
) -> &'a T {
    let mut roll = rng.next_int_bounded(total_weight);
    for (item, weight) in entries {
        roll -= weight;
        if roll < 0 {
            return item;
        }
    }
    unreachable!("weighted list total weight does not match its entries")
}

/// Java `FloatProvider`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FloatProvider {
    Constant(f32),
    /// `nextFloat() * (max - min) + min`.
    Uniform {
        min: f32,
        max_exclusive: f32,
    },
    ClampedNormal {
        mean: f32,
        deviation: f32,
        min: f32,
        max: f32,
    },
    Trapezoid {
        min: f32,
        max: f32,
        plateau: f32,
    },
}

impl FloatProvider {
    /// `sample(random)`.
    pub fn sample<S: BitSource>(&self, rng: &mut WorldgenRandom<S>) -> f32 {
        match *self {
            FloatProvider::Constant(v) => v,
            FloatProvider::Uniform { min, max_exclusive } => {
                rng.next_float() * (max_exclusive - min) + min
            }
            FloatProvider::ClampedNormal {
                mean,
                deviation,
                min,
                max,
            } => (mean + rng.next_gaussian() as f32 * deviation).clamp(min, max),
            FloatProvider::Trapezoid { min, max, plateau } => {
                let range = max - min;
                let slope = (range - plateau) / 2.0;
                let rest = range - slope;
                min + rng.next_float() * rest + rng.next_float() * slope
            }
        }
    }

    /// `getMinValue()`.
    pub fn min_value(&self) -> f32 {
        match *self {
            FloatProvider::Constant(v) => v,
            FloatProvider::Uniform { min, .. }
            | FloatProvider::ClampedNormal { min, .. }
            | FloatProvider::Trapezoid { min, .. } => min,
        }
    }

    /// `getMaxValue()`.
    pub fn max_value(&self) -> f32 {
        match *self {
            FloatProvider::Constant(v) => v,
            FloatProvider::Uniform { max_exclusive, .. } => max_exclusive,
            FloatProvider::ClampedNormal { max, .. } | FloatProvider::Trapezoid { max, .. } => max,
        }
    }
}

/// Height providers, given resolved anchors (`VerticalAnchor.resolveY`).
///
/// Java logs and returns `min` when the range is empty; the generated code
/// calls these with anchors resolved against the level's min Y and height.
pub mod height {
    use super::super::random::{BitSource, WorldgenRandom};

    /// `UniformHeight.sample`.
    #[inline]
    pub fn uniform<S: BitSource>(rng: &mut WorldgenRandom<S>, min: i32, max: i32) -> i32 {
        if min > max {
            return min;
        }
        rng.next_int_between_inclusive(min, max)
    }

    /// `TrapezoidHeight.sample`.
    #[inline]
    pub fn trapezoid<S: BitSource>(
        rng: &mut WorldgenRandom<S>,
        min: i32,
        max: i32,
        plateau: i32,
    ) -> i32 {
        if min > max {
            return min;
        }
        let range = max - min;
        if plateau >= range {
            return rng.next_int_between_inclusive(min, max);
        }
        let slope = (range - plateau) / 2;
        let rest = range - slope;
        min + rng.next_int_between_inclusive(0, rest) + rng.next_int_between_inclusive(0, slope)
    }

    /// `BiasedToBottomHeight.sample`.
    #[inline]
    pub fn biased_to_bottom<S: BitSource>(
        rng: &mut WorldgenRandom<S>,
        min: i32,
        max: i32,
        inner: i32,
    ) -> i32 {
        if max - min - inner + 1 <= 0 {
            return min;
        }
        let k = rng.next_int_bounded(max - min - inner + 1);
        rng.next_int_bounded(k + inner) + min
    }

    /// `VeryBiasedToBottomHeight.sample`.
    #[inline]
    pub fn very_biased_to_bottom<S: BitSource>(
        rng: &mut WorldgenRandom<S>,
        min: i32,
        max: i32,
        inner: i32,
    ) -> i32 {
        if max - min - inner + 1 <= 0 {
            return min;
        }
        let k = rng.next_int_between_inclusive(min + inner, max);
        let l = rng.next_int_between_inclusive(min, k - 1);
        rng.next_int_between_inclusive(min, l - 1 + inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::random::LegacyRandom;

    static LIST: [(IntProvider, i32); 2] =
        [(IntProvider::Constant(1), 3), (IntProvider::Constant(7), 1)];

    #[test]
    fn weighted_list_walks_cumulative_weights() {
        // Every roll in [0, 3) picks the first entry, roll 3 the second: over many
        // draws the split must be 3:1 within noise.
        let p = IntProvider::WeightedList {
            entries: &LIST,
            total_weight: 4,
        };
        let mut rng = LegacyRandom::new(1);
        let n = 40_000;
        let sevens = (0..n).filter(|_| p.sample(&mut rng) == 7).count();
        assert!((9_000..11_000).contains(&sevens), "{sevens}");
        assert_eq!(p.min_value(), 1);
        assert_eq!(p.max_value(), 7);
    }

    #[test]
    fn empty_height_range_returns_min() {
        let mut rng = LegacyRandom::new(1);
        assert_eq!(height::uniform(&mut rng, 10, 5), 10);
        assert_eq!(height::trapezoid(&mut rng, 10, 5, 0), 10);
        assert_eq!(height::very_biased_to_bottom(&mut rng, 5, 5, 1), 5);
    }
}
