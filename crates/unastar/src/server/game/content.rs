use super::GameServer;
use crate::registry::{BlockRegistry, ItemRegistry};
use jolyne::valentine::CreativeContentPacket;

impl GameServer {
    /// Build creative content packet from vanilla Bedrock creative inventory data.
    ///
    /// Loads item data from pmmp/BedrockData JSON files and maps them to protocol types.
    /// This ensures the creative inventory matches vanilla Minecraft Bedrock exactly.
    pub(super) fn build_creative_content(
        items: &ItemRegistry,
        blocks: &BlockRegistry,
    ) -> CreativeContentPacket {
        use crate::registry::CreativeInventoryData;
        use jolyne::valentine::{
            CreativeContentPacket, CreativeContentPacketGroupsItem,
            CreativeContentPacketGroupsItemCategory, CreativeContentPacketItemsItem, ItemLegacy,
            ItemLegacyContent, ItemLegacyContentExtra,
        };

        // Load vanilla creative inventory data from JSON files
        let creative_data = match CreativeInventoryData::load() {
            Ok(data) => data,
            Err(e) => {
                tracing::error!(error = %e, "Failed to load creative inventory data, falling back to empty inventory");
                return CreativeContentPacket {
                    groups: vec![],
                    items: vec![],
                };
            }
        };

        // Map tab names to protocol categories
        let category_map = |tab_name: &str| match tab_name {
            "Construction" => CreativeContentPacketGroupsItemCategory::Construction,
            "Nature" => CreativeContentPacketGroupsItemCategory::Nature,
            "Equipment" => CreativeContentPacketGroupsItemCategory::Equipment,
            "Items" => CreativeContentPacketGroupsItemCategory::Items,
            _ => CreativeContentPacketGroupsItemCategory::Items,
        };

        // Build groups from JSON data
        let mut protocol_groups = Vec::new();
        let mut items_list = Vec::new();
        let mut entry_id_counter = 1i32;
        let mut global_group_index = 0i32;

        for (tab_name, tab_groups) in creative_data.all_groups_ordered() {
            let category = category_map(tab_name);

            for group in tab_groups {
                // Parse group icon
                let icon_item = if let Some(icon_value) = &group.group_icon {
                    // Icon can be a string or an object with name and block_states
                    if let Some(icon_str) = icon_value.as_str() {
                        // Simple string icon
                        items.get_by_name(icon_str)
                            .map(|item| {
                                let block_runtime_id = blocks
                                    .get_by_name(icon_str)
                                    .map(|b| b.default_state_id as i32)
                                    .unwrap_or(0);

                                ItemLegacy {
                                    network_id: item.id as i32,
                                    content: Some(Box::new(ItemLegacyContent {
                                        count: 1,
                                        metadata: 0,
                                        block_runtime_id,
                                        extra: ItemLegacyContentExtra::default(),
                                    })),
                                }
                            })
                            .unwrap_or_else(|| ItemLegacy {
                                network_id: 0, // Air
                                content: None,
                            })
                    } else {
                        // Complex icon with block states - use air for now
                        ItemLegacy {
                            network_id: 0,
                            content: None,
                        }
                    }
                } else {
                    // No icon specified - anonymous group
                    ItemLegacy {
                        network_id: 0,
                        content: None,
                    }
                };

                protocol_groups.push(CreativeContentPacketGroupsItem {
                    category: category.clone(),
                    name: group.group_name.clone(),
                    icon_item,
                });
                // Process items in this group
                for creative_item in &group.items {
                    let item_id = creative_item.item_id();

                    // Look up in ItemRegistry - skip if not found
                    let Some(item) = items.get_by_name(item_id) else {
                        // Item not in registry - skip it
                      continue;
                    };
                    
                    let network_id = item.id as i32;
                    
                    // Get block runtime ID if this is a block
                    let block_runtime_id = blocks
                        .get_by_name(item_id)
                        .map(|b| b.default_state_id as i32)
                        .unwrap_or(0);

                    // Get metadata (damage value) from JSON
                    let metadata = creative_item.damage();

                    items_list.push(CreativeContentPacketItemsItem {
                        entry_id: entry_id_counter,
                        item: ItemLegacy {
                            network_id,
                            content: Some(Box::new(ItemLegacyContent {
                                count: 1,
                                metadata: metadata.into(),
                                block_runtime_id,
                                extra: ItemLegacyContentExtra::default(),
                            })),
                        },
                        group_index: global_group_index,
                    });

                    entry_id_counter += 1;
                }

                global_group_index += 1;
            }
        }

        let skipped_count = entry_id_counter - 1 - items_list.len() as i32;
        tracing::info!(
            groups = protocol_groups.len(),
            items = items_list.len(),
            skipped = skipped_count,
            "Built creative content from vanilla data"
        );

        CreativeContentPacket {
            groups: protocol_groups,
            items: items_list,
        }
    }
}
