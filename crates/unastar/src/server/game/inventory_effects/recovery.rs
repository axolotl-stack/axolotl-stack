//! Terminal ownership fences for an external persistence result requiring recovery.

use super::*;
use crate::entity::components::{Effects, Hunger, Velocity};
use crate::server::game::player_events::ConnectionIdentity;
use crate::server::game::player_state::{self, PlayerSnapshot};

/// An opaque handle for one exact actor generation and transport ownership.
///
/// A terminal fence has no automatic release or worker completion. The application
/// must reconstruct coherent durable state and restart the runtime before admitting
/// this account again. Ordinary snapshot restoration cannot bypass a fence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryFence {
    player: Entity,
    generation: Uuid,
}
impl RecoveryFence {
    /// Identify the original actor without granting permission to restore its held state.
    pub fn player(self) -> Entity {
        self.player
    }

    /// A reused session ID or replacement transport does not own the original fence.
    pub fn is_current(self, world: &World) -> bool {
        world
            .get::<Fenced>(self.player)
            .is_some_and(|fenced| fenced.handle == self && fenced.owner.matches(world, self.player))
    }
}

struct Owner {
    identity: ConnectionIdentity,
    uuid: Uuid,
    runtime: i64,
}

/// Capture ownership before an external operation so a late failure cannot fence a replacement.
pub struct RecoverySource {
    player: Entity,
    owner: Owner,
}
impl RecoverySource {
    /// Bind the actual account, runtime identity and outbound transport without changing gameplay.
    pub fn capture(world: &World, player: Entity) -> Result<Self, EffectError> {
        if world.get::<Player>(player).is_none() {
            return Err(EffectError::MissingPlayer);
        }
        Ok(Self {
            player,
            owner: Owner {
                identity: ConnectionIdentity::capture(world, player)
                    .ok_or(EffectError::MissingPlayer)?,
                uuid: world
                    .get::<PlayerUuid>(player)
                    .ok_or(EffectError::MissingPlayer)?
                    .0,
                runtime: world
                    .get::<RuntimeEntityId>(player)
                    .ok_or(EffectError::MissingPlayer)?
                    .0,
            },
        })
    }
    /// Bind an immutable native event identity only if its original transport still owns this actor.
    pub fn from_connection(
        world: &World,
        player: Entity,
        identity: &ConnectionIdentity,
    ) -> Result<Self, EffectError> {
        if !identity.matches(world, player) {
            return Err(EffectError::Conflict);
        }
        Self::capture(world, player)
    }
}
impl Owner {
    /// Compare all account, actor-generation and outbound-channel identity facts.
    fn matches(&self, world: &World, player: Entity) -> bool {
        self.identity.matches(world, player)
            && world.get::<PlayerUuid>(player).map(|id| id.0) == Some(self.uuid)
            && world.get::<RuntimeEntityId>(player).map(|id| id.0) == Some(self.runtime)
    }
}

/// Held state is never published merely because a writer or a replacement session completes.
enum Source {
    Snapshot {
        _state: Box<PlayerSnapshot>,
    },
    Pending {
        _transaction: Box<Quarantined>,
        _hunger: Option<Hunger>,
        _position: Option<Position>,
        _rotation: Option<Rotation>,
        _velocity: Option<Velocity>,
        _effects: Option<Effects>,
    },
}
#[derive(Component)]
struct Fenced {
    handle: RecoveryFence,
    owner: Owner,
    _source: Source,
}
#[derive(Resource, Default)]
struct FencedAccounts(BTreeMap<Uuid, Account>);
struct Account {
    _handle: RecoveryFence,
    xuid: Option<String>,
}

/// Whether normal gameplay, captures and ordinary persistence must remain withheld.
pub fn is_recovery_fenced(world: &World, player: Entity) -> bool {
    world.get::<Fenced>(player).is_some()
}

/// Keep account admission closed even if the original entity is detached or removed.
pub(super) fn account_fenced(world: &World, uuid: Uuid) -> bool {
    recovery_account_fenced(world, uuid, None)
}

/// Query permanent account ownership across registered worlds, including after actor removal.
/// Empty or absent authenticated account IDs do not match unrelated anonymous accounts.
pub fn recovery_account_fenced(world: &World, uuid: Uuid, xuid: Option<&str>) -> bool {
    world
        .get_resource::<FencedAccounts>()
        .is_some_and(|accounts| {
            accounts.0.contains_key(&uuid)
                || xuid.filter(|id| !id.is_empty()).is_some_and(|id| {
                    accounts
                        .0
                        .values()
                        .any(|account| account.xuid.as_deref() == Some(id))
                })
        })
}

/// Withhold a connected actor after an external commit cannot be made coherent.
///
/// This performs no persistence and never represents a durable acknowledgement.
/// The exact current source is retained, queued input is discarded, and mutable
/// gameplay components are physically withheld. Existing inventory-effect worker
/// completions cannot restore this terminally fenced actor. Unrelated actors are
/// unaffected. Invalid or replaced ownership fails before any actor mutation.
pub fn fence_for_recovery(
    world: &mut World,
    source: &RecoverySource,
) -> Result<RecoveryFence, EffectError> {
    let player = source.player;
    if !source.owner.matches(world, player) {
        return Err(EffectError::Conflict);
    }
    if let Some(fenced) = world.get::<Fenced>(player) {
        return if fenced.owner.matches(world, player) {
            Ok(fenced.handle)
        } else {
            Err(EffectError::Conflict)
        };
    }
    let owner = Owner {
        identity: ConnectionIdentity::capture(world, player).ok_or(EffectError::MissingPlayer)?,
        uuid: world
            .get::<PlayerUuid>(player)
            .ok_or(EffectError::MissingPlayer)?
            .0,
        runtime: world
            .get::<RuntimeEntityId>(player)
            .ok_or(EffectError::MissingPlayer)?
            .0,
    };
    if world.get::<Player>(player).is_none() {
        return Err(EffectError::MissingPlayer);
    }
    let source = if let Some(pending) = world.get::<Quarantined>(player) {
        if !same_source(world, player, pending, &pending.id) {
            return Err(EffectError::Conflict);
        }
        None
    } else {
        Some(player_state::capture(world, player).map_err(|_| EffectError::MissingPlayer)?)
    };
    let handle = RecoveryFence {
        player,
        generation: Uuid::new_v4(),
    };
    close_window(world, player);
    let source = match source {
        Some(snapshot) => Source::Snapshot {
            _state: Box::new(snapshot),
        },
        None => Source::Pending {
            _hunger: world.get::<Hunger>(player).cloned(),
            _position: world.get::<Position>(player).cloned(),
            _rotation: world.get::<Rotation>(player).copied(),
            _velocity: world.get::<Velocity>(player).cloned(),
            _effects: world.entity_mut(player).take::<Effects>(),
            _transaction: Box::new(
                world
                    .entity_mut(player)
                    .take::<Quarantined>()
                    .expect("validated pending ownership"),
            ),
        },
    };
    if let Some(coordinator) = world.get_resource::<Coordinator>() {
        coordinator.writer.require_recovery(owner.uuid);
    }
    world.init_resource::<FencedAccounts>();
    world.resource_mut::<FencedAccounts>().0.insert(
        owner.uuid,
        Account {
            _handle: handle,
            xuid: owner
                .identity
                .xuid()
                .filter(|id| !id.is_empty())
                .map(str::to_owned),
        },
    );
    // Position/velocity/effects are withheld too, so autonomous physics and life
    // systems cannot continue mutating a terminal source while recovery is pending.
    world
        .entity_mut(player)
        .remove::<(
            MainInventory,
            ArmourInventory,
            OffhandSlot,
            PlayerUi,
            EnderChestInventory,
            Health,
            Hunger,
            PlayerInput,
            BreakingState,
            Position,
            Rotation,
            Velocity,
            Effects,
            super::super::item_use::ItemUsing,
        )>()
        .insert(Fenced {
            handle,
            owner,
            _source: source,
        });
    clear_inputs(world, player);
    Ok(handle)
}
