//! Viewer metadata is projected internally from committed scoped component intents.

use super::*;
use crate::plugin::manager::views::NameTagVisibility;
use jolyne::valentine::McpePacketData;
use jolyne::valentine::types::{DataItemBytePayload, DataItemStringPayload};

/// Self metadata uses no duplicate local actor representation.
pub(super) fn has_projection(world: &World, actor: Entity, viewer: Entity) -> bool {
    crate::plugin::manager::views::projection(world, actor, viewer).is_some()
}

/// Replace one typed actor-data row without affecting unrelated metadata or flag words.
fn put(data: &mut Vec<DataItemEntry>, id: u32, payload: DataItemEntryPayload) {
    if let Some(entry) = data.iter_mut().find(|entry| entry.id == id) {
        entry.payload = payload;
    } else {
        data.push(DataItemEntry { id, payload });
    }
}

/// Ensure an override can be cleared even when the authoritative snapshot omitted a name row.
pub(super) fn baseline(world: &World, actor: Entity, data: &mut Vec<DataItemEntry>) {
    if !data.iter().any(|entry| entry.id == ids::NAME)
        && let Some(name) = world.get::<PlayerName>(actor)
    {
        put(
            data,
            ids::NAME,
            DataItemEntryPayload::DataItemStringPayload(DataItemStringPayload {
                type_: EnumsDataItemType::String,
                value: name.0.clone().into(),
            }),
        );
    }
    if !data
        .iter()
        .any(|entry| entry.id == ids::NAMETAG_ALWAYS_SHOW)
    {
        put(
            data,
            ids::NAMETAG_ALWAYS_SHOW,
            DataItemEntryPayload::DataItemBytePayload(DataItemBytePayload {
                type_: EnumsDataItemType::Byte,
                value: 0,
            }),
        );
    }
}

/// Apply only presentation rows while preserving every unrelated authoritative value.
pub(super) fn metadata(
    world: &World,
    actor: Entity,
    viewer: Entity,
    original: &[DataItemEntry],
) -> Vec<DataItemEntry> {
    let Some(projection) = crate::plugin::manager::views::projection(world, actor, viewer) else {
        return original.to_vec();
    };
    let mut data = original.to_vec();
    baseline(world, actor, &mut data);
    if let Some(tag) = &projection.name_tag {
        put(
            &mut data,
            ids::NAME,
            DataItemEntryPayload::DataItemStringPayload(DataItemStringPayload {
                type_: EnumsDataItemType::String,
                value: tag.clone().into(),
            }),
        );
    }
    if projection.name_tag_visibility.is_some() || projection.invisible {
        let mut flags = data
            .iter()
            .find_map(|entry| {
                if entry.id == ids::RESERVED_0
                    && let DataItemEntryPayload::DataItemInt64Payload(value) = &entry.payload
                {
                    Some(value.value as u64)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| {
                player_flags(
                    world,
                    actor,
                    world
                        .get::<crate::server::game::item_use::ItemUsing>(actor)
                        .is_some(),
                ) as u64
            });
        if let Some(visibility) = projection.name_tag_visibility {
            let can_show = 1u64 << actor_flags::CAN_SHOW_NAME;
            let always = 1u64 << actor_flags::ALWAYS_SHOW_NAME;
            flags &= !(can_show | always);
            if visibility != NameTagVisibility::Hidden {
                flags |= can_show;
            }
            if visibility == NameTagVisibility::Always {
                flags |= always;
            }
            put(
                &mut data,
                ids::NAMETAG_ALWAYS_SHOW,
                DataItemEntryPayload::DataItemBytePayload(DataItemBytePayload {
                    type_: EnumsDataItemType::Byte,
                    value: i8::from(visibility == NameTagVisibility::Always),
                }),
            );
        }
        if projection.invisible {
            flags |= 1u64 << actor_flags::INVISIBLE;
        }
        put(
            &mut data,
            ids::RESERVED_0,
            DataItemEntryPayload::DataItemInt64Payload(DataItemInt64Payload {
                type_: EnumsDataItemType::Int64,
                value: flags as i64,
            }),
        );
    }
    data.sort_by_key(|entry| entry.id);
    data
}

/// Keep immediate native flag updates consistent with the viewer's projected snapshot.
pub(crate) fn packet(
    world: &World,
    actor: Entity,
    viewer: Entity,
    original: &McpePacket,
) -> McpePacket {
    let mut packet = original.clone();
    if let McpePacketData::SetActorDataPacket(data) = &mut packet.data
        && world
            .get::<RuntimeEntityId>(actor)
            .is_some_and(|id| id.0 as u64 == data.target_runtime_id.actor_runtime_id)
    {
        data.actor_data.data = metadata(world, actor, viewer, &data.actor_data.data);
    }
    packet
}
