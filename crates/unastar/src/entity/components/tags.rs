//! Entity tags (`/tag`, the selector argument `tag=`) and the input
//! permissions `/inputpermission` sets (the selector argument
//! `haspermission=`).

use bevy_ecs::prelude::*;

/// The tags `/tag` gave an entity (players included), in the order they
/// were added. An entity without the component has no tags.
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct Tags(pub Vec<String>);

impl Tags {
    pub fn contains(&self, tag: &str) -> bool {
        self.0.iter().any(|t| t == tag)
    }
}

/// One player input `/inputpermission` can turn off, with the bit
/// `UpdateClientInputLocks` carries for it (measured on BDS 1.26.51 by
/// turning each one off in turn).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputPermission {
    Camera,
    Movement,
    Jump,
    LateralMovement,
    Sneak,
    Dismount,
    Mount,
    MoveBackward,
    MoveForward,
    MoveLeft,
    MoveRight,
}

impl InputPermission {
    pub const ALL: [Self; 11] = [
        Self::Camera,
        Self::Movement,
        Self::Jump,
        Self::LateralMovement,
        Self::Sneak,
        Self::Dismount,
        Self::Mount,
        Self::MoveBackward,
        Self::MoveForward,
        Self::MoveLeft,
        Self::MoveRight,
    ];

    /// The name in the captured `permission` enum and in `haspermission`.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Camera => "camera",
            Self::Movement => "movement",
            Self::Jump => "jump",
            Self::LateralMovement => "lateral_movement",
            Self::Sneak => "sneak",
            Self::Dismount => "dismount",
            Self::Mount => "mount",
            Self::MoveBackward => "move_backward",
            Self::MoveForward => "move_forward",
            Self::MoveLeft => "move_left",
            Self::MoveRight => "move_right",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// The `UpdateClientInputLocks` bit that turns the input off.
    pub const fn bit(self) -> u32 {
        match self {
            Self::Camera => 1 << 1,
            Self::Movement => 1 << 2,
            Self::LateralMovement => 1 << 4,
            Self::Sneak => 1 << 5,
            Self::Jump => 1 << 6,
            Self::Mount => 1 << 7,
            Self::Dismount => 1 << 8,
            Self::MoveForward => 1 << 9,
            Self::MoveBackward => 1 << 10,
            Self::MoveLeft => 1 << 11,
            Self::MoveRight => 1 << 12,
        }
    }
}

impl InputLocks {
    /// Whether `permission` is enabled (not locked).
    pub fn enabled(self, permission: InputPermission) -> bool {
        self.0 & permission.bit() == 0
    }
}

/// The inputs a player has had turned off, as the `UpdateClientInputLocks`
/// bit set. A player without the component has every input enabled.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputLocks(pub u32);
