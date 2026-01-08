//! Parse vanilla behavior pack JSON and override KDL files.

mod overrides;
mod vanilla;

pub use overrides::{EntityOverride, Overrides, parse_overrides};
pub use vanilla::parse_vanilla_entities;
