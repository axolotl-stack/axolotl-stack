//! Java-style structures: templates, processors, placement and pieces.

pub mod processor;
pub mod ruined;
pub mod template;
pub mod template_data;
pub mod transform;
pub mod util;

/// Generated structure data (`structures.rs`): processor lists, pools,
/// structures and structure sets.
#[allow(clippy::all, missing_docs)]
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/structures.rs"));
}
