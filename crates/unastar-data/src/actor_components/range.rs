//! The schemas' legacy `FloatRange` / `IntRange` definitions, implemented by
//! hand because the engine reads more shapes than the published
//! `{"min", "max"}` object (both default 0). Vanilla also writes a bare number
//! (`min == max`; `behavior.ranged_attack.attack_interval` 3.5,
//! `behavior.place_block.xz_range` 0), `[min, max]`, and
//! `{"range_min", "range_max"}` (`behavior.celebrate.jump_interval`). The
//! generator lowers the published definitions for default literals and
//! re-exports these types from `body`.

use serde::{Deserialize, Deserializer};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatRange {
    pub max: f32,
    pub min: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntRange {
    pub max: i32,
    pub min: i32,
}

impl<'de> Deserialize<'de> for FloatRange {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (min, max) = Repr::deserialize(deserializer)?.bounds();
        Ok(Self { max, min })
    }
}

impl<'de> Deserialize<'de> for IntRange {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (min, max) = Repr::deserialize(deserializer)?.bounds();
        Ok(Self { max, min })
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
#[serde(bound(deserialize = "T: Deserialize<'de> + Default"))]
enum Repr<T> {
    Value(T),
    Pair([T; 2]),
    MinMax(MinMax<T>),
    RandomRange(RandomRange<T>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "T: Deserialize<'de> + Default"))]
struct MinMax<T> {
    #[serde(default)]
    max: T,
    #[serde(default)]
    min: T,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RandomRange<T> {
    range_min: T,
    range_max: T,
}

impl<T> Repr<T> {
    /// `(min, max)`.
    fn bounds(self) -> (T, T)
    where
        T: Copy,
    {
        match self {
            Self::Value(value) => (value, value),
            Self::Pair([min, max]) => (min, max),
            Self::MinMax(MinMax { max, min }) => (min, max),
            Self::RandomRange(RandomRange {
                range_min,
                range_max,
            }) => (range_min, range_max),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_authored_range_shape_decodes() {
        let range = |value| serde_json::from_value::<FloatRange>(value).unwrap();
        assert_eq!(
            range(json!({"min": 2, "max": 4})),
            FloatRange { max: 4.0, min: 2.0 }
        );
        assert_eq!(range(json!({"max": 4})), FloatRange { max: 4.0, min: 0.0 });
        assert_eq!(range(json!(3.5)), FloatRange { max: 3.5, min: 3.5 });
        assert_eq!(range(json!([1, 7])), FloatRange { max: 7.0, min: 1.0 });
        assert_eq!(
            range(json!({"range_min": 1, "range_max": 3.5})),
            FloatRange { max: 3.5, min: 1.0 }
        );
        assert!(serde_json::from_value::<FloatRange>(json!({"min": 1, "extra": 2})).is_err());
        assert_eq!(
            serde_json::from_value::<IntRange>(json!(0)).unwrap(),
            IntRange { max: 0, min: 0 }
        );
    }
}
