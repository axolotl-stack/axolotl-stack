//! Item and item stack types.
//!
//! This module provides the core item representation used throughout the server.

pub mod enchant;
pub mod equipment;
mod stack;
pub mod user_data;

#[cfg(test)]
pub(crate) use stack::test_support;
pub use stack::{ItemStack, fresh_net_id};
