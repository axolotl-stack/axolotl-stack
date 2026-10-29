//! Typed command arguments: one Rust enum per command, one variant per
//! captured overload.
//!
//! ```ignore
//! overloads! {
//!     /// `/xp`
//!     pub enum Xp for "xp" {
//!         0 => Points { amount: i32, player: Option<Target> },
//!         1 => Levels { amount: Levels = "amount", player: Option<Target> },
//!     }
//! }
//! ```
//!
//! Each variant lists the overload's parameters in captured order; a field
//! named differently from the captured parameter gives the captured name
//! (`= "yRot"`). Optional parameters are `Option<T>`. The registry checks every
//! declaration against the captured syntax when it is registered (index,
//! names, order, optionality, and that each field type can hold the
//! parameter's kind), so a declaration that drifts from BDS fails at startup
//! and in the registry test, and executors `match` exhaustively over the
//! overloads instead of probing names.

use super::parser::{ArgValue, CommandPosition, IntRange, StateLiteral, Target};
use std::collections::HashMap;
use unastar_data::commands::{BasicType, CommandDefinition, ParamKind};

/// The raw arguments of the overload a command line matched.
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

    /// The raw parsed value of a parameter of the matched overload.
    pub fn raw(&self, name: &str) -> Option<&ArgValue> {
        self.values.get(name)
    }

    /// Whether the line supplied this parameter.
    pub fn has(&self, name: &str) -> bool {
        self.values.contains_key(name)
    }
}

/// Conversion from one parsed argument.
pub trait FromArg: Sized {
    /// Whether a parameter declared as `kind` produces this type.
    fn accepts(kind: &ParamKind) -> bool;
    fn from_arg(value: &ArgValue) -> Option<Self>;
}

fn basic(kind: &ParamKind, types: &[BasicType]) -> bool {
    matches!(kind, ParamKind::Basic(b) if types.contains(b))
}

macro_rules! from_arg {
    ($ty:ty, |$kind:ident| $accepts:expr, |$value:ident| $convert:expr) => {
        impl FromArg for $ty {
            fn accepts($kind: &ParamKind) -> bool {
                $accepts
            }
            fn from_arg($value: &ArgValue) -> Option<Self> {
                $convert
            }
        }
    };
}

from_arg!(
    i32,
    |kind| basic(kind, &[BasicType::Int]),
    |value| match value {
        ArgValue::Int(v) => Some(*v),
        _ => None,
    }
);
from_arg!(
    f32,
    |kind| basic(kind, &[BasicType::Float]),
    |value| match value {
        ArgValue::Float(v) => Some(*v),
        _ => None,
    }
);
// The captured `Boolean` enum.
from_arg!(
    bool,
    |kind| matches!(kind, ParamKind::Enum(e) if e == "Boolean"),
    |value| match value {
        ArgValue::Enum("true") => Some(true),
        ArgValue::Enum("false") => Some(false),
        _ => None,
    }
);
from_arg!(
    Target,
    |kind| basic(kind, &[BasicType::Target]),
    |value| match value {
        ArgValue::Target(t) => Some(t.clone()),
        _ => None,
    }
);
from_arg!(
    CommandPosition,
    |kind| basic(kind, &[BasicType::Position]),
    |value| match value {
        ArgValue::Position(p) => Some(*p),
        _ => None,
    }
);
from_arg!(
    serde_json::Value,
    |kind| basic(kind, &[BasicType::Json]),
    |value| match value {
        ArgValue::Json(v) => Some(v.clone()),
        _ => None,
    }
);
from_arg!(
    BlockStates,
    |kind| basic(kind, &[BasicType::BlockStates]),
    |value| match value {
        ArgValue::BlockStates(v) => Some(BlockStates(v.clone())),
        _ => None,
    }
);
from_arg!(
    IntRange,
    |kind| basic(kind, &[BasicType::IntegerRange]),
    |value| match value {
        ArgValue::IntRange(v) => Some(*v),
        _ => None,
    }
);

macro_rules! text_arg {
    ($(#[$meta:meta])* $name:ident, |$kind:ident| $accepts:expr, $($variant:ident)|+) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name(pub String);

        impl std::ops::Deref for $name {
            type Target = str;
            fn deref(&self) -> &str {
                &self.0
            }
        }

        from_arg!($name, |$kind| $accepts, |value| match value {
            $(ArgValue::$variant(s))|+ => Some($name(s.clone())),
            _ => None,
        });
    };
}

text_arg!(
    /// A `string` parameter (one word or a quoted string).
    Text,
    |kind| basic(kind, &[BasicType::String]),
    String
);
text_arg!(
    /// A `message` parameter: the rest of the line, as typed.
    Message,
    |kind| basic(kind, &[BasicType::Message]),
    Message
);
text_arg!(
    /// A `rawtext` parameter: the rest of the line.
    RawText,
    |kind| basic(kind, &[BasicType::RawText]),
    RawText
);
text_arg!(
    /// A nested command line (without `/`).
    CommandLine,
    |kind| basic(kind, &[BasicType::Command]),
    Command
);
text_arg!(
    /// A `filepath` parameter.
    FilePath,
    |kind| basic(kind, &[BasicType::FilePath]),
    FilePath
);
text_arg!(
    /// A soft-enum value (any word; the server may extend the list).
    SoftEnumValue,
    |kind| matches!(kind, ParamKind::SoftEnum(_)),
    SoftEnum
);
text_arg!(
    /// A chained subcommand and everything after it (`/execute`).
    Chained,
    |kind| matches!(kind, ParamKind::Chained(_)),
    Chained
);
text_arg!(
    /// A scoreboard operation (`+=`, `><`, …).
    Operation,
    |kind| basic(kind, &[BasicType::Operator]),
    Operator
);
text_arg!(
    /// A comparison (`<`, `>=`, …).
    Comparison,
    |kind| basic(kind, &[BasicType::CompareOperator]),
    CompareOperator
);

/// A value of a hard enum the code does not model as a Rust enum, in the
/// enum's own spelling (item, block and entity identifiers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnumValue(pub &'static str);

impl std::ops::Deref for EnumValue {
    type Target = str;
    fn deref(&self) -> &str {
        self.0
    }
}

from_arg!(
    EnumValue,
    |kind| matches!(kind, ParamKind::Enum(_)),
    |value| match value {
        ArgValue::Enum(v) => Some(EnumValue(v)),
        _ => None,
    }
);

/// A `block_position` parameter (floored when resolved).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockPosition(pub CommandPosition);

from_arg!(
    BlockPosition,
    |kind| basic(kind, &[BasicType::BlockPosition]),
    |value| match value {
        ArgValue::BlockPosition(p) => Some(BlockPosition(*p)),
        _ => None,
    }
);

/// `int` or `*`; `None` is the wildcard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WildcardInt(pub Option<i32>);

from_arg!(
    WildcardInt,
    |kind| basic(kind, &[BasicType::WildcardInt]),
    |value| match value {
        ArgValue::WildcardInt(v) => Some(WildcardInt(*v)),
        _ => None,
    }
);

/// A target or `*`; `None` is the wildcard.
#[derive(Debug, Clone, PartialEq)]
pub struct WildcardTarget(pub Option<Target>);

from_arg!(
    WildcardTarget,
    |kind| basic(kind, &[BasicType::WildcardTarget]),
    |value| match value {
        ArgValue::WildcardTarget(v) => Some(WildcardTarget(v.clone())),
        _ => None,
    }
);

/// A number written with a postfix (`5L`): the number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Postfixed(pub i32);

from_arg!(
    Postfixed,
    |kind| matches!(kind, ParamKind::Postfix(_)),
    |value| match value {
        ArgValue::Postfix(v) => Some(Postfixed(*v)),
        _ => None,
    }
);

/// A block-state list (`["color"="red"]`).
#[derive(Debug, Clone, PartialEq)]
pub struct BlockStates(pub Vec<(String, StateLiteral)>);

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

from_arg!(
    RelativeFloat,
    |kind| basic(kind, &[BasicType::Value]),
    |value| match value {
        ArgValue::Value { relative, value } => Some(RelativeFloat {
            relative: *relative,
            value: *value,
        }),
        _ => None,
    }
);

/// A field of an overload variant: required (`T`) or optional (`Option<T>`).
pub trait Param: Sized {
    const OPTIONAL: bool;
    fn accepts(kind: &ParamKind) -> bool;
    /// Reads the field from the matched overload's arguments.
    fn take(args: &Args, name: &str) -> Self;
}

impl<T: FromArg> Param for T {
    const OPTIONAL: bool = false;
    fn accepts(kind: &ParamKind) -> bool {
        T::accepts(kind)
    }
    fn take(args: &Args, name: &str) -> Self {
        args.raw(name)
            .and_then(T::from_arg)
            .unwrap_or_else(|| panic!("/{} {name}: required by the syntax", args.command().name))
    }
}

impl<T: FromArg> Param for Option<T> {
    const OPTIONAL: bool = true;
    fn accepts(kind: &ParamKind) -> bool {
        T::accepts(kind)
    }
    fn take(args: &Args, name: &str) -> Self {
        args.raw(name).map(|value| {
            T::from_arg(value).unwrap_or_else(|| {
                panic!(
                    "/{} {name}: {value:?} has another type",
                    args.command().name
                )
            })
        })
    }
}

/// One declared field, for checking against the captured syntax.
#[derive(Debug, Clone, Copy)]
pub struct FieldShape {
    pub name: &'static str,
    pub optional: bool,
    pub accepts: fn(&ParamKind) -> bool,
}

/// One declared variant: the overload it reads and its fields.
#[derive(Debug, Clone, Copy)]
pub struct VariantShape {
    pub overload: usize,
    pub variant: &'static str,
    pub fields: &'static [FieldShape],
}

/// A command's typed arguments (declared with [`overloads!`](crate::command::overloads)).
pub trait Overloads: Sized + 'static {
    /// The captured command name.
    const COMMAND: &'static str;
    /// One entry per variant.
    const SHAPE: &'static [VariantShape];
    /// The variant of the matched overload.
    fn from_args(args: &Args) -> Self;
}

/// Checks a declaration against the captured command: every overload has
/// exactly one variant whose fields are its parameters in order, with the
/// same optionality and a type that holds the parameter's kind.
pub fn check_shape(shape: &[VariantShape], command: &CommandDefinition) -> Result<(), String> {
    let name = &command.name;
    for (index, overload) in command.overloads.iter().enumerate() {
        let variants: Vec<_> = shape.iter().filter(|v| v.overload == index).collect();
        let [variant] = variants[..] else {
            return Err(format!(
                "/{name} overload {index} has {} variants, needs exactly one",
                variants.len()
            ));
        };
        let declared: Vec<_> = variant.fields.iter().map(|f| f.name).collect();
        let captured: Vec<_> = overload.params.iter().map(|p| p.name.as_str()).collect();
        if declared != captured {
            return Err(format!(
                "/{name} {}: fields {declared:?}, captured overload {index} has {captured:?}",
                variant.variant
            ));
        }
        for (field, param) in variant.fields.iter().zip(&overload.params) {
            if field.optional != param.optional {
                return Err(format!(
                    "/{name} {}.{}: optional is {}, captured {}",
                    variant.variant, field.name, field.optional, param.optional
                ));
            }
            if !(field.accepts)(&param.kind) {
                return Err(format!(
                    "/{name} {}.{}: field type cannot hold captured {}",
                    variant.variant, field.name, param.kind
                ));
            }
        }
    }
    if let Some(extra) = shape.iter().find(|v| v.overload >= command.overloads.len()) {
        return Err(format!(
            "/{name} {} reads overload {}, the command has {}",
            extra.variant,
            extra.overload,
            command.overloads.len()
        ));
    }
    Ok(())
}

/// Declares a command's typed arguments; see the module docs.
macro_rules! overloads {
    (@name $field:ident $param:literal) => { $param };
    (@name $field:ident) => { stringify!($field) };
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident for $command:literal {
            $(
                $(#[$vmeta:meta])*
                $index:literal => $variant:ident {
                    $( $field:ident : $ty:ty $(= $param:literal)? ),* $(,)?
                }
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        // Fields mirror the captured parameters; keyword parameters (`facing`)
        // carry no value an executor reads.
        #[allow(dead_code)]
        $vis enum $name {
            $( $(#[$vmeta])* $variant { $( $field: $ty ),* } ),*
        }

        impl $crate::command::args::Overloads for $name {
            const COMMAND: &'static str = $command;
            const SHAPE: &'static [$crate::command::args::VariantShape] = &[
                $( $crate::command::args::VariantShape {
                    overload: $index,
                    variant: stringify!($variant),
                    fields: &[ $( $crate::command::args::FieldShape {
                        name: $crate::command::args::overloads!(@name $field $($param)?),
                        optional: <$ty as $crate::command::args::Param>::OPTIONAL,
                        accepts: <$ty as $crate::command::args::Param>::accepts,
                    } ),* ],
                } ),*
            ];

            #[allow(unused_variables)]
            fn from_args(args: &$crate::command::args::Args) -> Self {
                match args.overload() {
                    $( $index => Self::$variant { $(
                        $field: <$ty as $crate::command::args::Param>::take(
                            args,
                            $crate::command::args::overloads!(@name $field $($param)?),
                        ),
                    )* }, )*
                    other => unreachable!(
                        "/{} has no overload {other} (the registry checked the shape)",
                        $command
                    ),
                }
            }
        }
    };
}
pub(crate) use overloads;

/// A Rust enum for one captured command enum; usable as an overload field.
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

        impl $crate::command::args::FromArg for $name {
            fn accepts(kind: &unastar_data::commands::ParamKind) -> bool {
                matches!(kind, unastar_data::commands::ParamKind::Enum(e) if e == $captured)
            }
            fn from_arg(value: &$crate::command::parser::ArgValue) -> Option<Self> {
                match value {
                    $crate::command::parser::ArgValue::Enum(v) => {
                        <Self as $crate::command::args::ArgEnum>::from_value(v)
                    }
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
