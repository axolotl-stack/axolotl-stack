//! `minecraft:the_end`: the End's terrain is end stone throughout. The
//! builder turns the shaped terrain's stone into end stone over its whole
//! window (no top or mid layer; the End has no sea).

use super::super::generated::bb;
use super::{Column, SurfaceColumn};

/// Run the End builder; returns the lowest y it changed.
pub(super) fn build<C: SurfaceColumn>(col: &mut C, c: &Column<'_>) -> Option<i32> {
    let mut lowest = None;
    for y in c.lo..c.hi {
        if col.get(y) == bb::STONE {
            col.set(y, bb::END_STONE);
            lowest.get_or_insert(y);
        }
    }
    lowest
}

#[cfg(test)]
mod tests {
    use super::super::super::rng::BedrockRandom;
    use super::super::test_support::{VecColumn, biome, input};
    use super::super::{BiomeData, Builder, SurfaceNoises, build_column};
    use super::*;

    #[test]
    fn stone_becomes_end_stone() {
        let noises = SurfaceNoises::with_biomes(1, &[]);
        let b = BiomeData {
            overworld: false,
            ..biome(Builder::TheEnd)
        };
        let mut col = VecColumn::terrain(60, None);
        col.set(10, bb::BEDROCK);
        build_column(
            &mut col,
            &input(&b, 0, 0),
            &noises,
            &mut BedrockRandom::new(0),
        );
        assert_eq!(col.count(|id| id == bb::STONE), 0);
        assert_eq!(col.count(|id| id == bb::END_STONE), 60 + 64);
        assert_eq!(col.get(10), bb::BEDROCK);
        assert_eq!(col.get(61), bb::AIR);
    }
}
