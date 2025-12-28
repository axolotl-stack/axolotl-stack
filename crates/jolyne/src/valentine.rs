//! Protocol facade for the single Bedrock version supported by `jolyne`.
//!
//! This hides the underlying `valentine` version module from downstream users.

pub use valentine::bedrock::v1_21_130::*;
