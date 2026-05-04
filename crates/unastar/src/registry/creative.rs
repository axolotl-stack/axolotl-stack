//! Creative inventory data surface.
//!
//! Runtime code consumes generated, source-attributed creative inventory data
//! from `unastar-data` instead of parsing PMMP/BedrockData JSON directly.

pub use unastar_data::creative::{
    CreativeEntryData as CreativeItemEntry, CreativeGroupData as CreativeGroup,
    CreativeInventoryData,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_creative_data() {
        let data = CreativeInventoryData::load();

        // Verify all tabs have content.
        assert!(!data.construction.is_empty(), "Construction tab is empty");
        assert!(!data.equipment.is_empty(), "Equipment tab is empty");
        assert!(!data.items.is_empty(), "Items tab is empty");
        assert!(!data.nature.is_empty(), "Nature tab is empty");

        // Verify first group in construction.
        let first_group = &data.construction[0];
        assert_eq!(first_group.group_name, "itemGroup.name.planks");
        assert!(!first_group.items.is_empty());
    }

    #[test]
    fn test_pmmp_meta_variants_are_preserved() {
        let data = CreativeInventoryData::load();
        let (_, equipment) = data
            .all_groups_ordered()
            .into_iter()
            .find(|(tab, _)| *tab == "Equipment")
            .expect("equipment tab exists");
        let arrow_group = equipment
            .iter()
            .find(|group| group.group_name == "itemGroup.name.arrow")
            .expect("arrow creative group exists");

        assert!(
            arrow_group
                .items
                .iter()
                .any(|item| item.item_id() == "minecraft:arrow" && item.damage() == 6),
            "PMMP `meta` values must survive creative artifact generation"
        );
    }
}
