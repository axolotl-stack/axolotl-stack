//! Java-style structures: templates, processors, placement and pieces.

pub mod context;
pub mod jigsaw;
pub mod piece;
pub mod kinds;
pub mod placement;
pub mod processor;
pub mod ruined;
pub mod start;
pub mod template;
pub mod template_data;
pub mod template_piece;
pub mod transform;
pub mod util;

/// Generated structure data (`structures.rs`): processor lists, pools,
/// structures and structure sets.
#[allow(clippy::all, missing_docs)]
pub mod generated {
    include!(concat!(env!("OUT_DIR"), "/structures.rs"));
}
