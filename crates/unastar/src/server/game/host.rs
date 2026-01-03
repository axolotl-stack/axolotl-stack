use crate::entity::components::{PlayerName, PlayerUuid, transform::Position};
use abi_stable::std_types::{ROption, RStr, RString};
use bevy_ecs::prelude::*;
use unastar_api::PluginAction;
use unastar_api::native::{NativeActionQueue, PlayerInfo, PluginEntity, RawPluginHost, Vec3};

pub struct ServerHost<'a> {
    pub world: &'a mut World,
}

impl<'a> RawPluginHost for ServerHost<'a> {
    fn send_message(&mut self, player_uuid: RStr<'_>, message: RStr<'_>) {
        if let Some(mut queue) = self.world.get_resource_mut::<NativeActionQueue>() {
            queue.actions.push(PluginAction::SendMessage {
                player_id: player_uuid.to_string(),
                message: message.to_string(),
            });
        }
    }

    fn teleport(&mut self, player_uuid: RStr<'_>, position: Vec3) {
        if let Some(mut queue) = self.world.get_resource_mut::<NativeActionQueue>() {
            queue.actions.push(PluginAction::Teleport {
                player_id: player_uuid.to_string(),
                position,
            });
        }
    }

    fn kick(&mut self, player_uuid: RStr<'_>, reason: RStr<'_>) {
        if let Some(mut queue) = self.world.get_resource_mut::<NativeActionQueue>() {
            queue.actions.push(PluginAction::Kick {
                player_id: player_uuid.to_string(),
                reason: reason.to_string(),
            });
        }
    }

    fn give_item(&mut self, player_uuid: RStr<'_>, item_id: RStr<'_>, count: u8) {
        if let Some(mut queue) = self.world.get_resource_mut::<NativeActionQueue>() {
            queue.actions.push(PluginAction::GiveItem {
                player_id: player_uuid.to_string(),
                item_id: item_id.to_string(),
                count,
            });
        }
    }

    fn entity_count(&self) -> u32 {
        self.world.entities().len()
    }

    fn get_player_info(&self, entity: PluginEntity) -> ROption<PlayerInfo> {
        let be = Entity::from_bits(entity.to_bits());
        let info = (|| {
            let name = self.world.get::<PlayerName>(be).map(|n| n.0.clone())?;
            let uuid = self.world.get::<PlayerUuid>(be).map(|u| u.0.to_string())?;
            let pos = self
                .world
                .get::<Position>(be)
                .map(|p| Vec3::new(p.0.x, p.0.y, p.0.z));

            Some(PlayerInfo {
                name: RString::from(name),
                uuid: RString::from(uuid),
                position: pos.into(),
            })
        })();

        info.into()
    }
}
