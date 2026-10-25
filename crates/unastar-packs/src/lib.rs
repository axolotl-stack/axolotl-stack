//! Bevy-free reader for Bedrock behaviour packs.
//!
//! One implementation serves every consumer (architecture §2: datagen and
//! build scripts, the Bedrock-style worldgen codegen, and the startup loader
//! for addon packs):
//!
//! 1. **Containers** ([`brarchive`], [`json`]): BDS `.brarchive` files and
//!    JSON with `//` / `/* */` comments.
//! 2. **Stacks** ([`packs`]): the layered `vanilla` / `vanilla_<version>`
//!    stack of a BDS install, or a single pack directory (bedrock-samples).
//! 3. **Family loaders** ([`actor`]): parse one document the way BDS does
//!    (parser fidelity, entities research §4.7) and emit the normalised form
//!    that the runtime decodes; the runtime never sees a raw pack spelling.
//!
//! The brarchive reader, JSON comment stripping and pack stack were written
//! for the Bedrock-style worldgen codegen and moved here when actor
//! definitions became their second consumer.

pub mod actor;
pub mod brarchive;
pub mod error;
pub mod json;
pub mod packs;

pub use error::{Error, Result};
