//! Core polymorphic types for handling Bedrock's inconsistent JSON.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Generic Bedrock data value for schema branches that stay open-ended.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(untagged)]
pub enum BedrockValue {
    #[default]
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Array(Vec<BedrockValue>),
    Object(HashMap<String, BedrockValue>),
}

impl From<bool> for BedrockValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i32> for BedrockValue {
    fn from(value: i32) -> Self {
        Self::Integer(value as i64)
    }
}

impl From<i64> for BedrockValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<f32> for BedrockValue {
    fn from(value: f32) -> Self {
        Self::Float(value as f64)
    }
}

impl From<f64> for BedrockValue {
    fn from(value: f64) -> Self {
        Self::Float(value)
    }
}

impl From<String> for BedrockValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for BedrockValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

/// Handles fields that can be a single value OR a min/max range.
/// Examples: `fuse_length`, `damage`, `cooldown`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RangeOrVal<T> {
    Range {
        #[serde(alias = "range_min")]
        min: T,
        #[serde(alias = "range_max")]
        max: T,
    },
    Fixed(T),
}

impl<T: Copy> RangeOrVal<T> {
    pub fn fixed(&self) -> Option<T> {
        match self {
            Self::Fixed(val) => Some(*val),
            Self::Range { .. } => None,
        }
    }

    pub fn range(&self) -> Option<(T, T)> {
        match self {
            Self::Range { min, max } => Some((*min, *max)),
            Self::Fixed(_) => None,
        }
    }
}

impl<T: Copy + PartialOrd + rand::distributions::uniform::SampleUniform> RangeOrVal<T> {
    pub fn sample(&self, rng: &mut impl rand::Rng) -> T {
        match self {
            Self::Fixed(val) => *val,
            Self::Range { min, max } => rng.gen_range(*min..=*max),
        }
    }
}

impl<T: Default> Default for RangeOrVal<T> {
    fn default() -> Self {
        Self::Fixed(T::default())
    }
}

/// Handles fields that can be a literal value OR a Molang expression.
/// Examples: `rotate_rider_by`, `success_chance`, `priority`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MolangOr<T> {
    Value(T),
    Expr(String),
}

impl<T> MolangOr<T> {
    pub fn as_value(&self) -> Option<&T> {
        match self {
            Self::Value(v) => Some(v),
            Self::Expr(_) => None,
        }
    }

    pub fn as_expr(&self) -> Option<&str> {
        match self {
            Self::Expr(s) => Some(s),
            Self::Value(_) => None,
        }
    }
}

impl<T: Default> Default for MolangOr<T> {
    fn default() -> Self {
        Self::Value(T::default())
    }
}

/// Handles boolean fields that can be `true`/`false` OR string `"yes"`/`"no"`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum BoolOrString {
    Bool(bool),
    String(String),
}

impl Default for BoolOrString {
    fn default() -> Self {
        Self::Bool(false)
    }
}

impl BoolOrString {
    pub fn as_bool(&self) -> bool {
        match self {
            Self::Bool(b) => *b,
            Self::String(s) => {
                s.eq_ignore_ascii_case("yes") || s.eq_ignore_ascii_case("true") || s == "1"
            }
        }
    }
}

impl From<bool> for BoolOrString {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

/// Deserializer helper for fields that can be a single object or array.
pub mod one_or_many {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum OneOrMany<T> {
            One(T),
            Many(Vec<T>),
        }

        match OneOrMany::deserialize(deserializer)? {
            OneOrMany::One(val) => Ok(vec![val]),
            OneOrMany::Many(vec) => Ok(vec),
        }
    }

    pub fn serialize<S, T>(value: &Vec<T>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: Serialize,
    {
        value.serialize(serializer)
    }
}
