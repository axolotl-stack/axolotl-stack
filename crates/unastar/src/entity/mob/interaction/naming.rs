//! Name tags: a name tag renamed in an anvil names the mob it is used on.
//!
//! The mob must have `minecraft:nameable` with `allow_name_tag_renaming`;
//! the tag must carry a custom name. The mob's actor data `NAME` becomes
//! that name (viewers show it), one tag is used up outside creative and the
//! interaction makes the mob persistent. The name is saved with the mob as
//! `CustomName`.

use bevy_ecs::prelude::*;
use bevy_ecs::world::EntityWorldMut;
use jolyne::valentine::actor_data_i_ds as ids;

use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::components::NameableConfig;
use crate::entity::mob::state::CustomName;
use crate::item::ItemStack;
use crate::item::enchant::custom_name;

pub const NAME_TAG: &str = "minecraft:name_tag";

/// Name the mob `name` and show it in its actor data.
pub fn set_custom_name(entity: &mut EntityWorldMut<'_>, name: String) {
    if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
        metadata.set_string(ids::NAME, name.as_str());
    }
    entity.insert(CustomName(name));
}

/// A named name tag used on `target`: whether it named the mob.
pub(super) fn apply_name_tag(
    world: &mut World,
    player: Entity,
    target: Entity,
    held: &ItemStack,
) -> bool {
    if held.item_id() != NAME_TAG
        || !world
            .get::<NameableConfig>(target)
            .is_some_and(|config| config.allow_name_tag_renaming)
    {
        return false;
    }
    let Some(name) = custom_name(held).filter(|name| !name.is_empty()) else {
        return false;
    };
    set_custom_name(&mut world.entity_mut(target), name);
    super::interact::use_held_item(world, player, None);
    true
}
