//! Available-command overlays belong to live exact connections, never leaked static syntax.

use super::*;
use crate::command::CommandSchema;
use crate::entity::components::{Player, PlayerSession};
use jolyne::valentine::types::{
    AvailableCommandsPacketCommandDatajson, AvailableCommandsPacketPayloadOverloadData,
    AvailableCommandsPacketPayloadParamData,
};
use jolyne::valentine::{AvailableCommandsPacket, McpePacket};
use std::collections::HashMap;

/// The immutable inputs that produced one successfully sent advertisement.
pub(crate) struct Stamp {
    catalogue: Arc<Vec<Root>>,
    schema: Arc<CommandSchema>,
}
struct Sent {
    identity: ConnectionIdentity,
    stamp: Stamp,
}
/// Interior synchronization permits recording the immutable initial join batch.
#[derive(Resource, Default)]
pub(super) struct Publications(Mutex<HashMap<Entity, Sent>>);

/// Extend an owned wire packet without altering captured builtin definitions or handlers.
fn packet(schema: &CommandSchema, catalogue: &[Root]) -> AvailableCommandsPacket {
    let mut packet = schema.packet().clone();
    for root in catalogue {
        if schema.command(&root.name).is_some() {
            continue;
        }
        packet
            .commands
            .push(AvailableCommandsPacketCommandDatajson {
                name: root.name.as_str().into(),
                description: root.description.as_str().into(),
                flags: 0,
                permission_level: "any".into(),
                alias_enum: -1,
                command_data_chained_subcommand_indexes: Vec::new(),
                overloads: vec![AvailableCommandsPacketPayloadOverloadData {
                    is_chaining: false,
                    parameter_data: vec![AvailableCommandsPacketPayloadParamData {
                        name: "arguments".into(),
                        parse_symbol: unastar_data::commands::SYMBOL_VALID
                            | unastar_data::commands::BasicType::Message.symbol(),
                        is_optional: true,
                        options: 0,
                    }],
                }],
            });
    }
    packet
}

/// Prepare the initial packet and retain its catalogue identity until the send completes.
pub(crate) fn initial_packet(
    world: &World,
    schema: Arc<CommandSchema>,
) -> (AvailableCommandsPacket, Stamp) {
    let catalogue = world
        .get_resource::<CommandRoots>()
        .map(CommandRoots::snapshot)
        .unwrap_or_default();
    let packet = packet(&schema, &catalogue);
    (packet, Stamp { catalogue, schema })
}

/// Cache only successful initial sends, bound to the actual account and outbound channel.
pub(crate) fn note_initial_packet(world: &World, player: Entity, stamp: Stamp) {
    let Some(identity) = ConnectionIdentity::capture(world, player) else {
        return;
    };
    if let Some(state) = world.get_resource::<Publications>() {
        state.0.lock().insert(player, Sent { identity, stamp });
    }
}

/// Changed catalogues reach each admitted current transport once; backpressure remains retryable.
pub(super) fn reconcile(world: &mut World) {
    let Some(roots) = world.get_resource::<CommandRoots>() else {
        return;
    };
    let catalogue = roots.snapshot();
    let Some(schema) = world
        .get_resource::<crate::command::CommandRegistry>()
        .map(|registry| registry.schema().clone())
    else {
        return;
    };
    let players: Vec<_> = world
        .query_filtered::<Entity, (With<Player>, With<PlayerSession>)>()
        .iter(world)
        .collect();
    world.init_resource::<Publications>();
    let state = world.resource::<Publications>();
    let mut sent = state.0.lock();
    sent.retain(|player, previous| previous.identity.matches(world, *player));
    let mut prepared = None;
    for player in players {
        if crate::server::game::player_events::initial_state_pending(world, player)
            || crate::server::game::inventory_effects::is_quarantined(world, player)
        {
            continue;
        }
        let Some(identity) = ConnectionIdentity::capture(world, player) else {
            continue;
        };
        if sent.get(&player).is_some_and(|previous| {
            previous.identity.matches(world, player)
                && Arc::ptr_eq(&previous.stamp.catalogue, &catalogue)
                && Arc::ptr_eq(&previous.stamp.schema, &schema)
        }) {
            continue;
        }
        let packet = prepared.get_or_insert_with(|| packet(&schema, &catalogue));
        if world
            .get::<PlayerSession>(player)
            .is_some_and(|session| session.send(McpePacket::from(packet.clone())))
        {
            sent.insert(
                player,
                Sent {
                    identity,
                    stamp: Stamp {
                        catalogue: catalogue.clone(),
                        schema: schema.clone(),
                    },
                },
            );
        }
    }
}
