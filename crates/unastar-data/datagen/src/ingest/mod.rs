//! Parse vanilla behavior pack JSON and override KDL files.

mod overrides;
mod vanilla;

pub use overrides::{parse_overrides, EntityOverride, Overrides};
pub use vanilla::parse_vanilla_entities;
