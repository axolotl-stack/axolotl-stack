//! Isolated dimension worlds and native player migration.

mod components;
pub use components::TransferComponents;

mod hooks;
pub use hooks::{
    PlayerWorldTransfer, PlayerWorldTransferred, TransferError, WorldTransferFailed,
    WorldTransferHandler, WorldTransferHooks, request_transfer_checked,
};

mod directory;
pub(crate) mod loading;
mod router;
mod transfer;
pub use directory::{
    PlayerAvailability, RouterTickHandler, WorldDirectory, WorldPlayer, queue_router_request,
};
pub use router::{DimensionRouter, WorldTickError};
pub use transfer::{find_safe_destination, is_safe_destination};

pub use loading::is_loading;

#[cfg(test)]
mod tests;
