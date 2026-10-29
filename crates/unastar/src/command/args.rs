//! Typed access to a parsed command's arguments.
//!
//! Executors read parameters by their captured name and the Rust type they
//! expect: `args.get::<i32>("amount")`, `args.get::<&Target>("player")`,
//! `args.choice::<TimeSpec>("time")`. Optional parameters the player left out
//! are `None`. Asking for a name the command does not have, or for the wrong
//! type, is an executor bug: debug builds (and so every test) panic on it.

use super::parser::{ArgValue, CommandPosition, IntRange, StateLiteral, Target};
use std::collections::HashMap;
use unastar_data::commands::{BasicType, CommandDefinition, ParamKind};

/// The arguments of the overload a command line matched.
#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    definition: &'static CommandDefinition,
    overload: usize,
    values: HashMap<&'static str, ArgValue>,
}

impl Args {
    pub(super) fn new(definition: &'static CommandDefinition, overload: usize) -> Self {
        Self {
            definition,
            overload,
            values: HashMap::new(),
        }
    }

    pub(super) fn insert(&mut self, name: &'static str, value: ArgValue) {
        self.values.insert(name, value);
    }

    /// The captured command (its canonical name, not the alias typed).
    pub fn command(&self) -> &'static CommandDefinition {
        self.definition
    }

    /// Index of the matched overload in the captured syntax.
    pub fn overload(&self) -> usize {
        self.overload
    }

    /// Debug check that `name` is a parameter of this command and that at
    /// least one overload declares it with a kind `accepts` takes.
    fn check(&self, name: &str, accepts: impl Fn(&ParamKind) -> bool, wanted: &str) {
        if cfg!(debug_assertions) {
            let mut declared = self
                .definition
                .overloads
                .iter()
                .flat_map(|o| &o.params)
                .filter(|p| p.name == name)
                .peekable();
            assert!(
                declared.peek().is_some(),
                "/{} has no parameter named {name:?}",
                self.definition.name
            );
            assert!(
                declared.any(|p| accepts(&p.kind)),
                "/{} parameter {name:?} is never a {wanted}",
                self.definition.name
            );
        }
    }

    /// The raw parsed value.
    pub fn raw(&self, name: &str) -> Option<&ArgValue> {
        self.check(name, |_| true, "value");
        self.values.get(name)
    }

    /// Whether the line supplied this parameter.
    pub fn has(&self, name: &str) -> bool {
        self.raw(name).is_some()
    }

    /// A parameter as `T`; `None` when it was not given, or when the matched
    /// overload declares it with another type (`/gamemode`'s `gameMode` is an
    /// enum in one overload and an int in the other).
    pub fn get<'a, T: FromArg<'a>>(&'a self, name: &str) -> Option<T> {
        self.check(name, T::accepts, std::any::type_name::<T>());
        T::from_arg(self.values.get(name)?)
    }

    /// An enum parameter as a Rust enum (see [`arg_enum!`](crate::command::arg_enum)).
    pub fn choice<T: ArgEnum>(&self, name: &str) -> Option<T> {
        self.check(
            name,
            |kind| matches!(kind, ParamKind::Enum(e) if e == T::ENUM),
            T::ENUM,
        );
        match self.values.get(name)? {
            ArgValue::Enum(value) => {
                let choice = T::from_value(value);
                debug_assert!(
                    choice.is_some(),
                    "{} does not cover {value:?}",
                    std::any::type_name::<T>()
                );
                choice
            }
            _ => None,
        }
    }
}

/// Conversion from a parsed argument.
pub trait FromArg<'a>: Sized {
    /// Whether a parameter declared as `kind` can produce this type.
    fn accepts(kind: &ParamKind) -> bool;
    fn from_arg(value: &'a ArgValue) -> Option<Self>;
}

fn basic(kind: &ParamKind, types: &[BasicType]) -> bool {
    matches!(kind, ParamKind::Basic(b) if types.contains(b))
}

impl FromArg<'_> for i32 {
    fn accepts(kind: &ParamKind) -> bool {
        basic(kind, &[BasicType::Int, BasicType::WildcardInt])
            || matches!(kind, ParamKind::Postfix(_))
    }
    fn from_arg(value: &ArgValue) -> Option<Self> {
        match value {
            ArgValue::Int(v) | ArgValue::Postfix(v) => Some(*v),
            ArgValue::WildcardInt(v) => *v,
            _ => None,
        }
    }
}

impl FromArg<'_> for f32 {
    fn accepts(kind: &ParamKind) -> bool {
        basic(kind, &[BasicType::Float, BasicType::Int])
    }
    fn from_arg(value: &ArgValue) -> Option<Self> {
        match value {
            ArgValue::Float(v) => Some(*v),
            ArgValue::Int(v) => Some(*v as f32),
            _ => None,
        }
    }
}

/// The captured `Boolean` enum.
impl FromArg<'_> for bool {
    fn accepts(kind: &ParamKind) -> bool {
        matches!(kind, ParamKind::Enum(e) if e == "Boolean")
    }
    fn from_arg(value: &ArgValue) -> Option<Self> {
        match value {
            ArgValue::Enum("true") => Some(true),
            ArgValue::Enum("false") => Some(false),
            _ => None,
        }
    }
}

/// Any textual parameter: strings, messages, enum and soft-enum values, paths.
impl<'a> FromArg<'a> for &'a str {
    fn accepts(kind: &ParamKind) -> bool {
        basic(
            kind,
            &[
                BasicType::String,
                BasicType::Message,
                BasicType::RawText,
                BasicType::Command,
                BasicType::FilePath,
                BasicType::Operator,
                BasicType::CompareOperator,
            ],
        ) || matches!(
            kind,
            ParamKind::Enum(_) | ParamKind::SoftEnum(_) | ParamKind::Chained(_)
        )
    }
    fn from_arg(value: &'a ArgValue) -> Option<Self> {
        match value {
            ArgValue::String(s)
            | ArgValue::Message(s)
            | ArgValue::RawText(s)
            | ArgValue::Command(s)
            | ArgValue::SoftEnum(s)
            | ArgValue::FilePath(s)
            | ArgValue::Chained(s)
            | ArgValue::Operator(s)
            | ArgValue::CompareOperator(s) => Some(s),
            ArgValue::Enum(s) => Some(s),
            _ => None,
        }
    }
}

impl<'a> FromArg<'a> for &'a Target {
    fn accepts(kind: &ParamKind) -> bool {
        basic(kind, &[BasicType::Target, BasicType::WildcardTarget])
    }
    fn from_arg(value: &'a ArgValue) -> Option<Self> {
        match value {
            ArgValue::Target(t) => Some(t),
            ArgValue::WildcardTarget(t) => t.as_ref(),
            _ => None,
        }
    }
}

/// `position` and `block_position` parameters.
impl<'a> FromArg<'a> for &'a CommandPosition {
    fn accepts(kind: &ParamKind) -> bool {
        basic(kind, &[BasicType::Position, BasicType::BlockPosition])
    }
    fn from_arg(value: &'a ArgValue) -> Option<Self> {
        match value {
            ArgValue::Position(p) | ArgValue::BlockPosition(p) => Some(p),
            _ => None,
        }
    }
}

impl<'a> FromArg<'a> for &'a serde_json::Value {
    fn accepts(kind: &ParamKind) -> bool {
        basic(kind, &[BasicType::Json])
    }
    fn from_arg(value: &'a ArgValue) -> Option<Self> {
        match value {
            ArgValue::Json(v) => Some(v),
            _ => None,
        }
    }
}

impl<'a> FromArg<'a> for &'a [(String, StateLiteral)] {
    fn accepts(kind: &ParamKind) -> bool {
        basic(kind, &[BasicType::BlockStates])
    }
    fn from_arg(value: &'a ArgValue) -> Option<Self> {
        match value {
            ArgValue::BlockStates(v) => Some(v),
            _ => None,
        }
    }
}

impl FromArg<'_> for IntRange {
    fn accepts(kind: &ParamKind) -> bool {
        basic(kind, &[BasicType::IntegerRange])
    }
    fn from_arg(value: &ArgValue) -> Option<Self> {
        match value {
            ArgValue::IntRange(v) => Some(*v),
            _ => None,
        }
    }
}

/// A `value` parameter: a number that may be relative (`~10`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RelativeFloat {
    pub relative: bool,
    pub value: f32,
}

impl RelativeFloat {
    /// The value, added to `base` when relative.
    pub fn resolve(self, base: f32) -> f32 {
        if self.relative {
            base + self.value
        } else {
            self.value
        }
    }
}

impl FromArg<'_> for RelativeFloat {
    fn accepts(kind: &ParamKind) -> bool {
        basic(kind, &[BasicType::Value])
    }
    fn from_arg(value: &ArgValue) -> Option<Self> {
        match value {
            ArgValue::Value { relative, value } => Some(Self {
                relative: *relative,
                value: *value,
            }),
            _ => None,
        }
    }
}

/// A Rust enum for one captured command enum; declare with [`arg_enum!`](crate::command::arg_enum).
pub trait ArgEnum: Sized + Copy + 'static {
    /// Name of the captured enum the type covers (`GameMode`, `TimeSpec`, …).
    const ENUM: &'static str;
    /// Every spelling the type accepts, in declaration order.
    const SPELLINGS: &'static [&'static str];
    /// Maps a captured spelling (as the parser returns it) to a variant.
    fn from_value(value: &str) -> Option<Self>;
}

/// Declares a Rust enum over a captured command enum:
///
/// ```ignore
/// arg_enum! {
///     /// `/time set` presets.
///     pub enum TimeSpec in "TimeSpec" {
///         Day = "day",
///         Noon = "noon",
///     }
/// }
/// ```
///
/// A variant may list several spellings (`Survival = "survival" | "s"`).
/// [`assert_covers`] checks a declaration against the captured enum.
macro_rules! arg_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident in $captured:literal {
            $($variant:ident = $first:literal $(| $more:literal)*),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        $vis enum $name { $($variant),* }

        impl $crate::command::args::ArgEnum for $name {
            const ENUM: &'static str = $captured;
            const SPELLINGS: &'static [&'static str] = &[$($first $(, $more)*),*];
            fn from_value(value: &str) -> Option<Self> {
                match value {
                    $($first $(| $more)* => Some(Self::$variant),)*
                    _ => None,
                }
            }
        }
    };
}
pub(crate) use arg_enum;

/// Fails unless `T` accepts exactly the values of its captured enum.
pub fn assert_covers<T: ArgEnum>() {
    let captured = unastar_data::commands::vanilla()
        .enum_named(T::ENUM)
        .unwrap_or_else(|| panic!("no captured enum {}", T::ENUM));
    let mut captured_values: Vec<&str> = captured.values.iter().map(String::as_str).collect();
    let mut spellings = T::SPELLINGS.to_vec();
    captured_values.sort_unstable();
    spellings.sort_unstable();
    assert_eq!(
        spellings,
        captured_values,
        "{} vs captured {}",
        std::any::type_name::<T>(),
        T::ENUM
    );
}
