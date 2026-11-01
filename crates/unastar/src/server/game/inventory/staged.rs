//! Owned per-request previews with no ECS borrow across native policy callbacks.

use super::*;
use crate::entity::components::Rotation;
use crate::server::game::container_mutation::{self, ContainerMutationCause};
use crate::server::game::player_events::ConnectionIdentity;
use jolyne::valentine::{
    EnumsItemStackNetResult as Status, ItemStackRequestPacket, ItemStackResponseInfo,
};
use std::time::Instant;

/// Complete player state that one inventory request may consume or replace.
#[derive(Clone)]
struct Frame {
    identity: ConnectionIdentity,
    main: MainInventory,
    armor: ArmourInventory,
    offhand: OffhandSlot,
    ui: PlayerUi,
    windows: ContainerWindows,
    held: u8,
    request_ids: RequestIdAssignments,
    mode: GameMode,
    position: glam::DVec3,
    rotation: Option<Rotation>,
    thrower: Option<i64>,
}
impl Frame {
    /// Copy the actual player's complete baseline before computing any effects.
    fn capture(world: &World, player: Entity) -> Option<Self> {
        Some(Self {
            identity: ConnectionIdentity::capture(world, player)?,
            main: world.get::<MainInventory>(player)?.clone(),
            armor: world.get::<ArmourInventory>(player)?.clone(),
            offhand: world.get::<OffhandSlot>(player)?.clone(),
            ui: world.get::<PlayerUi>(player)?.clone(),
            windows: world.get::<ContainerWindows>(player)?.clone(),
            held: world.get::<HeldSlot>(player)?.0,
            request_ids: world.get::<RequestIdAssignments>(player)?.clone(),
            mode: *world.get::<GameMode>(player)?,
            position: world.get::<Position>(player)?.0,
            rotation: world.get::<Rotation>(player).copied(),
            thrower: crate::entity::item_drop::drop_thrower_id(world, player),
        })
    }
    /// Compare every input after callbacks, including untouched player slots and
    /// request aliases, so a nested operation never loses its accepted changes.
    fn matches(&self, world: &World, player: Entity) -> bool {
        self.identity.matches(world, player)
            && world.get::<MainInventory>(player) == Some(&self.main)
            && world.get::<ArmourInventory>(player) == Some(&self.armor)
            && world
                .get::<OffhandSlot>(player)
                .map(|slot| (&slot.0, slot.0.net_id))
                == Some((&self.offhand.0, self.offhand.0.net_id))
            && world.get::<PlayerUi>(player) == Some(&self.ui)
            && world.get::<ContainerWindows>(player) == Some(&self.windows)
            && world.get::<HeldSlot>(player).map(|slot| slot.0) == Some(self.held)
            && world.get::<RequestIdAssignments>(player) == Some(&self.request_ids)
            && world.get::<GameMode>(player) == Some(&self.mode)
            && world.get::<Position>(player).map(|pos| pos.0) == Some(self.position)
            && world
                .get::<Rotation>(player)
                .map(|r| (r.yaw, r.pitch, r.head_yaw))
                == self.rotation.map(|r| (r.yaw, r.pitch, r.head_yaw))
            && crate::entity::item_drop::drop_thrower_id(world, player) == self.thrower
    }
    /// Publish the already validated player stores and request-ID aliases.
    fn install(self, world: &mut World, player: Entity) {
        *world
            .get_mut::<MainInventory>(player)
            .expect("revalidated main inventory") = self.main;
        *world
            .get_mut::<ArmourInventory>(player)
            .expect("revalidated armour") = self.armor;
        *world
            .get_mut::<OffhandSlot>(player)
            .expect("revalidated offhand") = self.offhand;
        *world.get_mut::<PlayerUi>(player).expect("revalidated UI") = self.ui;
        *world
            .get_mut::<RequestIdAssignments>(player)
            .expect("revalidated request aliases") = self.request_ids;
    }
}

/// Process frames independently, publishing no successful response or equipment
/// update until the full frame and any affected block view are accepted.
pub(super) fn apply(world: &mut World, player: Entity, packet: &ItemStackRequestPacket) {
    let Some(identity) = ConnectionIdentity::capture(world, player) else {
        return;
    };
    let Some(outbound) = world
        .get::<PlayerSession>(player)
        .map(|session| session.outbound_tx.clone())
    else {
        return;
    };
    let mut responses = Vec::with_capacity(packet.requests.len());
    let mut fatal = false;
    for request in &packet.requests {
        if !identity.matches(world, player) {
            break;
        }
        let Some(before) = Frame::capture(world, player) else {
            break;
        };
        let screen = match before.windows.screen() {
            ScreenKind::Block(screen) => Some(screen),
            _ => None,
        };
        // Preserve native window authorization before cloning its current slots.
        let view =
            screen.and_then(|screen| super::super::containers::window_slots(world, player, screen));
        let mut after = before.clone();
        let mut block = view.as_ref().map(|view| view.slots.clone());
        let mut outcome = {
            let items = world.resource::<super::super::types::ItemRegistryResource>();
            let crafting = world.resource::<super::super::types::CraftingExecutorResource>();
            let template = world.resource::<super::super::types::ServerWorldTemplate>();
            let ids = super::super::types::block_network_ids(world);
            let creative_item = |net_id| creative_stack(&items.0, template, net_id, ids);
            let ctx = engine::Context {
                screen: after.windows.screen(),
                creative: after.mode == GameMode::Creative,
                can_drop: after.rotation.is_some() && after.thrower.is_some(),
                now: Instant::now(),
                creative_item: &creative_item,
                crafting: &crafting.0,
            };
            let mut stores = engine::Stores {
                main: &mut after.main.0,
                armor: &mut after.armor,
                offhand: &mut after.offhand.0,
                ui: &mut after.ui.0,
                block: block.as_mut(),
            };
            // Keep the engine's fatal ID validation and normal Error response for
            // a retained block window that no longer has backing storage.
            let mut denied;
            let request = if screen.is_some() && view.is_none() {
                denied = request.clone();
                denied.actions.clear();
                &denied
            } else {
                request
            };
            engine::process(request, &mut stores, &mut after.request_ids, &ctx)
        };
        if outcome.fatal_invalid_id {
            fatal = true;
            break;
        }
        let mut accepted = None;
        let successful = outcome
            .response
            .as_ref()
            .is_some_and(|response| response.result == Status::Success);
        let changes = view
            .as_ref()
            .zip(block.as_ref())
            .filter(|(view, after)| view.slots != **after)
            .map(|(view, after)| vec![view.change(after)]);
        let allowed = if successful && let Some(changes) = changes {
            accepted = container_mutation::authorize(
                world,
                ContainerMutationCause::Player {
                    player,
                    identity: before.identity.clone(),
                },
                changes,
            );
            accepted.is_some()
        } else {
            true
        };
        if !allowed
            || !before.matches(world, player)
            || view.as_ref().is_some_and(|view| !view.matches(world))
        {
            outcome.response = Some(error_response(request));
            responses.extend(outcome.response);
            if !identity.matches(world, player) {
                break;
            }
            continue;
        }
        let block_ids = super::super::types::block_network_ids(world);
        if accepted.is_some()
            && let Some(view) = &view
        {
            view.prepare(world);
        }
        after.install(world, player);
        if let (Some(screen), Some(view), Some(block)) = (screen, view, block) {
            super::super::containers::commit_window(world, player, screen, &view.slots, block);
        }
        if let Some(session) = world.get::<PlayerSession>(player) {
            send_equipment_updates(
                &outcome.equipment,
                world.get::<ArmourInventory>(player).unwrap(),
                &world.get::<OffhandSlot>(player).unwrap().0,
                session,
                block_ids,
            );
        }
        for stack in outcome.drops {
            let spawned = crate::entity::item_drop::spawn_item_drop(
                world,
                before.position,
                stack,
                Some(player),
            );
            assert!(
                spawned.is_some(),
                "validated exclusive frame retains its thrower"
            );
        }
        responses.extend(outcome.response);
        if let Some(accepted) = accepted {
            container_mutation::committed(world, accepted);
        }
    }
    if !responses.is_empty() {
        let _ = outbound.try_send(McpePacket::from(
            jolyne::valentine::ItemStackResponsePacket { responses },
        ));
    }
    if fatal {
        let _ = outbound.try_send(McpePacket::from(invalid_request_warning()));
        if identity.matches(world, player) {
            world.entity_mut(player).insert(FatalInventoryRequest);
        }
    }
}

/// Reject policy or stale proposals without publishing any staged aliases/data.
fn error_response(
    request: &jolyne::valentine::ItemStackRequestPacketDataRequestData,
) -> ItemStackResponseInfo {
    ItemStackResponseInfo {
        result: Status::Error,
        client_request_id: request.client_request_id.clone(),
        containers: None,
    }
}
