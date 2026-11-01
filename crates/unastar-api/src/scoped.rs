//! Transaction-scoped SDK views over the component's resource capabilities.

use crate::{
    AccessError, GameMode, PlayerAvailability, PlayerFlags, PlayerHandle, PlayerIdentity,
    PlayerSnapshot, Vec3, WorldHandle, raw,
};
use std::marker::PhantomData;

/// A world transaction is valid only during the callback that supplied it.
/// Store a [`WorldHandle`] to schedule later work instead.
pub struct WorldTx<'scope> {
    raw: &'scope raw::WorldTx,
}
impl<'scope> WorldTx<'scope> {
    /// Observe an exact surviving native source hold without recreating its actor.
    pub fn source_fence(
        &self,
        archive: &raw::ArchivedPlayerState,
    ) -> Result<Option<raw::SourceFence>, raw::StateError> {
        archive.source_fence(self.raw)
    }
    /// Derive a raw image for this actual world without moving an actor.
    pub fn state_at(
        &self,
        image: &crate::PlayerStateImage,
        feet: Vec3,
        rotation: (f32, f32),
    ) -> Result<crate::PlayerStateImage, AccessError> {
        image.at(self.raw, feet, rotation)
    }
    /// Wrap the canonical borrowed resource for generated component bridges.
    #[doc(hidden)]
    pub fn __from_borrowed(raw: &'scope raw::WorldTx) -> Self {
        Self { raw }
    }
    /// Obtain a storable world reference, never a second live world borrow.
    pub fn world(&self) -> Result<WorldHandle, AccessError> {
        self.raw.world()
    }
    /// Construct an owned item value without changing a player's inventory.
    ///
    /// ```no_run
    /// use unastar_api::{AccessError, ItemStack, ItemValue, WorldTx};
    /// fn example(tx: &WorldTx<'_>) -> Result<ItemStack, AccessError> {
    ///     let paper = tx.item("minecraft:paper", 1)?;
    ///     let named = paper.with_name(Some("Example"))?;
    ///     named.with_value(
    ///         &["extension".into(), "serial".into()],
    ///         Some(&ItemValue::Long(42)),
    ///     )
    /// }
    /// ```
    pub fn item(&self, identifier: &str, count: u8) -> Result<crate::ItemStack, AccessError> {
        self.raw.item(identifier, count)
    }
    /// Capture typed loaded terrain and its host-owned expected-state witness.
    /// This does not load chunks, generate terrain or materialize block actors.
    pub fn block(
        &self,
        position: crate::BlockPos,
    ) -> Result<crate::BlockCell, crate::WorldBlockError> {
        self.raw.block(position)
    }
    /// Construct a detached builder bound to this world's actual runtime lifetime.
    pub fn edit_blocks(&self) -> Result<crate::BlockEdit, crate::WorldBlockError> {
        self.raw.edit_blocks()
    }
    /// Stage the whole builder for revalidation and physical publication after the callback.
    pub fn apply_blocks(
        &self,
        edit: &crate::BlockEdit,
    ) -> Result<crate::BlockEditRequest, crate::WorldBlockError> {
        self.raw.apply_blocks(edit)
    }
    /// Enumerate owned actor references in this transaction's current world.
    pub fn players(&self) -> Result<Vec<PlayerHandle>, AccessError> {
        self.raw.players()
    }
    /// Stage this exact viewer's sidebar, or release this component's sidebar with none.
    pub fn sidebar(
        &self,
        viewer: &Player<'_>,
        value: Option<&crate::Sidebar>,
    ) -> Result<(), crate::PresentationError> {
        crate::presentation::set_sidebar(self.raw, viewer.raw(), value)
    }
    /// Stage a maintained action bar, or clear this component's value with none.
    pub fn action_bar(
        &self,
        viewer: &Player<'_>,
        value: Option<&str>,
    ) -> Result<(), crate::PresentationError> {
        crate::presentation::set_action_bar(self.raw, viewer.raw(), value)
    }
    /// Display a typed button menu after callback success and initial publication.
    pub fn show_form(
        &self,
        viewer: &Player<'_>,
        value: &crate::SimpleForm,
        callback_id: u64,
    ) -> Result<u64, crate::PresentationError> {
        crate::presentation::show_form(self.raw, viewer.raw(), value, callback_id)
    }
    /// Close only this component's current exact request, never a replacement.
    pub fn close_form(
        &self,
        viewer: &Player<'_>,
        request_id: u64,
    ) -> Result<bool, crate::PresentationError> {
        crate::presentation::close_form(self.raw, viewer.raw(), request_id)
    }
    /// Stage a viewer-specific representation; none clears only this component's pair.
    /// Native audiences, invisibility and publication restrictions remain authoritative.
    pub fn project_player(
        &self,
        subject: &Player<'_>,
        viewer: &Player<'_>,
        value: Option<&crate::PlayerProjection>,
    ) -> Result<(), crate::ProjectionError> {
        crate::views::project_player(self.raw, subject.raw(), viewer.raw(), value)
    }
    /// Resolve a reference only in this world and this callback.
    /// The returned player cannot outlive the transaction borrow.
    ///
    /// ```compile_fail
    /// use unastar_api::{Player, PlayerHandle, WorldTx};
    /// fn escape(tx: &WorldTx<'_>, handle: &PlayerHandle) -> Player<'static> {
    ///     tx.player(handle).unwrap()
    /// }
    /// ```
    pub fn player<'tx>(&'tx self, handle: &PlayerHandle) -> Result<Player<'tx>, AccessError> {
        self.raw.player(handle).map(|raw| Player {
            raw: PlayerResource::Owned(raw),
            scope: PhantomData,
        })
    }
}

/// A storable acknowledgement bound to one component and original connection.
/// Dropping an unresolved deferred hold rejects admission rather than releasing it.
pub struct AdmissionHold {
    raw: raw::AdmissionHold,
}
impl AdmissionHold {
    /// Stage a full-state plan on this hold's exact initializing actor in a fresh task.
    /// The operation's durable acknowledgement does not resolve this admission hold.
    pub fn submit_state(
        &self,
        tx: &WorldTx<'_>,
        request_id: &str,
        expected: &crate::PlayerStateImage,
        desired: &crate::PlayerStateImage,
        source: Option<&crate::SourceFence>,
    ) -> Result<crate::StateOperation, crate::StateError> {
        self.raw
            .submit_state(tx.raw, request_id, expected, desired, source)
    }
    /// Stage this hold's inventory in a fresh task after Deferred admission has finished.
    pub fn submit_inventory(
        &self,
        tx: &WorldTx<'_>,
        request_id: &str,
        image: &crate::InventoryImage,
    ) -> Result<crate::InventoryOperation, crate::InventoryError> {
        self.raw.submit_inventory(tx.raw, request_id, image)
    }

    /// Stage a decision in a fresh world transaction; publication has a separate notification.
    pub fn resolve(
        &self,
        tx: &WorldTx<'_>,
        decision: crate::AdmissionResolution,
    ) -> Result<crate::AdmissionAcknowledgement, AccessError> {
        self.raw.resolve(tx.raw, &decision)
    }
}

enum PlayerResource<'scope> {
    Borrowed(&'scope raw::Player),
    Owned(raw::Player),
}

/// A live player valid only inside its originating transaction callback.
/// Use [`Self::handle`] when storing a roster or scheduling later work.
pub struct Player<'scope> {
    raw: PlayerResource<'scope>,
    scope: PhantomData<&'scope WorldTx<'scope>>,
}
impl<'scope> Player<'scope> {
    /// Stage native loading, safe-Y checks and durable movement to one actual world.
    /// Only `on_player_travel_result` reports the physical outcome. A successful
    /// callback keeps explicit requests even when its default action is cancelled.
    pub fn relocate(
        &self,
        destination: &WorldHandle,
        feet: Vec3,
        safe_y_radius: u16,
        correlation: u64,
    ) -> Result<crate::TravelAcknowledgement, AccessError> {
        self.raw()
            .relocate(destination, feet, safe_y_radius, correlation)
    }
    /// Own a complete actual state observation; it never changes gameplay by itself.
    pub fn state_image(&self) -> Result<crate::PlayerStateImage, AccessError> {
        self.raw().state_image()
    }
    /// Stage native ownership of this genuine old source. Dropping the returned
    /// observer cannot reactivate an accepted source hold.
    pub fn fence_state(
        &self,
        expected: &crate::PlayerStateImage,
    ) -> Result<crate::SourceFence, crate::StateError> {
        self.raw().fence_state(expected)
    }
    /// Stage an exact-source full-state write; only the owned operation reports ACK/application.
    pub fn submit_state(
        &self,
        request_id: &str,
        expected: &crate::PlayerStateImage,
        desired: &crate::PlayerStateImage,
        source: Option<&crate::SourceFence>,
    ) -> Result<crate::StateOperation, crate::StateError> {
        self.raw()
            .submit_state(request_id, expected, desired, source)
    }
    /// Read this component's already loaded or acknowledged canonical proof.
    pub fn state_receipt(
        &self,
        request_id: &str,
    ) -> Result<Option<crate::InventoryReceipt>, crate::StateError> {
        self.raw().state_receipt(request_id)
    }
    /// Bind generated callback arguments to the same transaction lifetime.
    #[doc(hidden)]
    pub fn __from_borrowed(raw: &'scope raw::Player, _tx: &'scope WorldTx<'scope>) -> Self {
        Self {
            raw: PlayerResource::Borrowed(raw),
            scope: PhantomData,
        }
    }
    /// Select the underlying capability without extending its callback lifetime.
    fn raw(&self) -> &raw::Player {
        match &self.raw {
            PlayerResource::Borrowed(raw) => raw,
            PlayerResource::Owned(raw) => raw,
        }
    }
    /// Query actual native initial-state completeness without claiming the player is Ready.
    pub fn initial_state_ready(&self) -> Result<bool, AccessError> {
        self.raw().initial_state_ready()
    }
    /// Retain this token and return Deferred when initialization needs later scoped callbacks.
    pub fn defer_admission(&self) -> Result<AdmissionHold, AccessError> {
        self.raw()
            .defer_admission()
            .map(|raw| AdmissionHold { raw })
    }
    /// Retain a stable actor reference instead of this scoped player.
    pub fn handle(&self) -> Result<PlayerHandle, AccessError> {
        self.raw().to_handle()
    }
    /// Read immutable authenticated identity from the current actor binding.
    pub fn identity(&self) -> Result<PlayerIdentity, AccessError> {
        self.raw().identity()
    }
    /// Read feet coordinates in this transaction's world.
    pub fn position(&self) -> Result<Vec3, AccessError> {
        self.raw().position()
    }
    /// Read the current yaw and pitch.
    pub fn rotation(&self) -> Result<(f32, f32), AccessError> {
        self.raw().rotation()
    }
    /// Read authoritative current health without substituting default values.
    pub fn health(&self) -> Result<f32, AccessError> {
        self.raw().health()
    }
    /// Read authoritative maximum health.
    pub fn max_health(&self) -> Result<f32, AccessError> {
        self.raw().max_health()
    }
    /// Read the durable base mode, independently of transient overlays.
    pub fn game_mode(&self) -> Result<GameMode, AccessError> {
        self.raw().game_mode()
    }
    /// Read the mode currently used by native gameplay and permissions.
    pub fn effective_game_mode(&self) -> Result<GameMode, AccessError> {
        self.raw().effective_game_mode()
    }
    /// Read current native movement and life facts.
    pub fn flags(&self) -> Result<PlayerFlags, AccessError> {
        self.raw().flags()
    }
    /// Read native lifecycle availability, independently of plugin game rules.
    pub fn availability(&self) -> Result<PlayerAvailability, AccessError> {
        self.raw().availability()
    }
    /// Read the immutable host configuration grant, not a mutable permission component.
    pub fn configured_admin(&self) -> Result<bool, AccessError> {
        self.raw().configured_admin()
    }
    /// Copy read-only observations when data must survive this callback.
    pub fn snapshot(&self) -> Result<PlayerSnapshot, AccessError> {
        self.raw().snapshot()
    }
    /// Capture a complete owned inventory image without granting a live inventory borrow.
    pub fn inventory(&self) -> Result<crate::InventoryImage, AccessError> {
        self.raw().inventory()
    }
    /// Observe an exact main, armour or offhand stack for a later bounded physical change.
    pub fn inventory_stack(
        &self,
        area: crate::InventoryArea,
        index: u32,
    ) -> Result<crate::ItemStack, AccessError> {
        self.raw().inventory_stack(area, index)
    }
    /// Move existing hotbar stacks without granting arbitrary replacements or durable delivery.
    /// Destination i receives original slot order[i]; the selection index stays unchanged.
    pub fn reorder_hotbar(
        &self,
        order: [u8; 9],
    ) -> Result<crate::InventoryMechanicsOperation, AccessError> {
        self.raw().reorder_hotbar(&order)
    }
    /// Stage consumption of the exact original selected stack at this callback's boundary.
    pub fn consume_selected(
        &self,
        expected: &crate::ItemStack,
        count: u8,
    ) -> Result<crate::InventoryMechanicsOperation, AccessError> {
        self.raw().consume_selected(expected, count)
    }
    /// Repair an exact captured main, armour or offhand stack without changing its other data.
    pub fn repair_item(
        &self,
        area: crate::InventoryArea,
        index: u32,
        expected: &crate::ItemStack,
        points: u32,
    ) -> Result<crate::InventoryMechanicsOperation, AccessError> {
        self.raw().repair_item(area, index, expected, points)
    }
    /// Copy current native effects, optional stored motion and mode/flight state.
    pub fn mechanics(&self) -> Result<crate::PlayerMechanicsState, AccessError> {
        self.raw().mechanics()
    }
    /// Copy actual native health, absorption and food values, excluding pending imports.
    pub fn vitals(&self) -> Result<crate::PlayerVitals, AccessError> {
        self.raw().vitals()
    }
    /// Stage finite nonnegative healing through native health clamping without reviving an actor.
    pub fn heal(&self, amount: f32) -> Result<crate::MechanicsAcknowledgement, AccessError> {
        self.raw().heal(amount)
    }
    /// Set transient native food-processing gates; none restores ordinary processing.
    pub fn set_food_policy(
        &self,
        policy: Option<crate::FoodPolicy>,
    ) -> Result<crate::MechanicsAcknowledgement, AccessError> {
        self.raw().set_food_policy(policy)
    }
    /// Stage a finite native effect. A source must be a real player in this same callback.
    /// Native precedence, immunity and effect authorization remain authoritative.
    pub fn apply_effect(
        &self,
        effect: &crate::EffectApplication,
        source: Option<&Player<'_>>,
    ) -> Result<crate::MechanicsAcknowledgement, AccessError> {
        self.raw().apply_effect(*effect, source.map(Player::raw))
    }
    /// Stage an actual infinite native effect on this living physical actor.
    /// Instant-only effect kinds are rejected; source remains a real same-world player.
    pub fn apply_infinite_effect(
        &self,
        effect: &crate::InfiniteEffectApplication,
        source: Option<&Player<'_>>,
    ) -> Result<crate::MechanicsAcknowledgement, AccessError> {
        self.raw()
            .apply_infinite_effect(*effect, source.map(Player::raw))
    }
    /// Stage removal of one native effect.
    pub fn remove_effect(
        &self,
        kind: crate::EffectKind,
    ) -> Result<crate::MechanicsAcknowledgement, AccessError> {
        self.raw().remove_effect(kind)
    }
    /// Stage milk-style clearing through native effect synchronization.
    pub fn clear_effects(&self) -> Result<crate::MechanicsAcknowledgement, AccessError> {
        self.raw().clear_effects()
    }
    /// Stage finite server-assigned motion, bounded to a magnitude of 64 blocks per tick.
    pub fn set_velocity(
        &self,
        velocity: Vec3,
    ) -> Result<crate::MechanicsAcknowledgement, AccessError> {
        self.raw().set_velocity(velocity)
    }
    /// Stage the durable base mode, preserving an active temporary overlay.
    pub fn set_game_mode(
        &self,
        mode: GameMode,
    ) -> Result<crate::MechanicsAcknowledgement, AccessError> {
        self.raw().set_game_mode(mode)
    }
    /// Stage a temporary native mode; clearing it restores the durable base.
    pub fn set_mode_override(
        &self,
        mode: Option<GameMode>,
    ) -> Result<crate::MechanicsAcknowledgement, AccessError> {
        self.raw().set_mode_override(mode)
    }
    /// Stage independent flight permission without changing creative inventory or immunity.
    pub fn set_flight_override(
        &self,
        allowed: Option<bool>,
    ) -> Result<crate::MechanicsAcknowledgement, AccessError> {
        self.raw().set_flight_override(allowed)
    }
    /// Persist complete host-owned state without exporting native encoded buffers.
    /// Acknowledgement is durable independently of this callback's later success.
    /// This does not detach, restore or otherwise change the current actor.
    pub fn archive(&self, key: &str) -> Result<crate::ArchiveReference, crate::ArchiveError> {
        self.raw().archive(key)
    }
    /// Stage an exact-source replacement; inspect the returned operation for a durable result.
    pub fn submit_inventory(
        &self,
        request_id: &str,
        image: &crate::InventoryImage,
    ) -> Result<crate::InventoryOperation, crate::InventoryError> {
        self.raw().submit_inventory(request_id, image)
    }
    /// Read a receipt already loaded or durably acknowledged for this account and component.
    pub fn inventory_receipt(
        &self,
        request_id: &str,
    ) -> Result<Option<crate::InventoryReceipt>, crate::InventoryError> {
        self.raw().inventory_receipt(request_id)
    }
    /// Stage termination for this exact admitted connection after callback success.
    /// The returned ticket distinguishes staged/pending/queued delivery from failure;
    /// queued delivery is not a client acknowledgement.
    pub fn disconnect(
        &self,
        reason: &crate::ConnectionCloseReason,
    ) -> Result<crate::ConnectionCloseRequest, crate::ConnectionControlError> {
        self.raw().disconnect(reason)
    }
    /// Request removal using the semantic kicked reason; inspect the returned delivery ticket.
    pub fn kick(
        &self,
        message: &str,
    ) -> Result<crate::ConnectionCloseRequest, crate::ConnectionControlError> {
        self.disconnect(&crate::ConnectionCloseReason {
            kind: crate::ConnectionCloseKind::Kicked,
            message: message.into(),
        })
    }
    /// Stage a bounded message for this exact connection after callback success.
    pub fn message(&self, message: &str) -> Result<(), AccessError> {
        self.raw().message(message)
    }
}

/// A handler's current player and world transaction, with its own cancellation scope.
pub struct PlayerContext<'scope> {
    tx: &'scope WorldTx<'scope>,
    player: &'scope Player<'scope>,
    cancelled: bool,
    cancellable: bool,
}
impl<'scope> PlayerContext<'scope> {
    /// Construct one event scope for generated Handler bridges.
    #[doc(hidden)]
    pub fn __new(
        tx: &'scope WorldTx<'scope>,
        player: &'scope Player<'scope>,
        cancellable: bool,
    ) -> Self {
        Self {
            tx,
            player,
            cancelled: false,
            cancellable,
        }
    }
    /// Access the player only within this event's callback.
    pub fn player(&self) -> &Player<'scope> {
        self.player
    }
    /// Query other actors in this same live world transaction.
    pub fn tx(&self) -> &WorldTx<'scope> {
        self.tx
    }
    /// Cancel a cancellable proposal. Returns false for committed notifications.
    pub fn cancel(&mut self) -> bool {
        if !self.cancellable {
            return false;
        }
        self.cancelled = true;
        true
    }
    /// Read this event's cancellation without affecting any other callback.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
}

/// Global enumeration returns storable handles, never live foreign-world players.
pub mod server {
    pub use crate::host::{configuration, get_tick_id, log, unix_time_seconds};
    use crate::{AccessError, PlayerHandle, WorldHandle};
    /// Enumerate immutable references to actors across registered worlds.
    pub fn players() -> Result<Vec<PlayerHandle>, AccessError> {
        crate::host::players()
    }
    /// Schedule work through a world reference to receive its own fresh transaction.
    pub fn worlds() -> Result<Vec<WorldHandle>, AccessError> {
        crate::host::worlds()
    }
}
