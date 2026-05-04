//! Biome registry for runtime biome management.

use super::{Registry, RegistryEntry};

/// Runtime biome entry in the registry.
#[derive(Debug, Clone)]
pub struct BiomeEntry {
    /// Internal registry ID.
    ///
    /// This is not a Bedrock packet/runtime biome ID. Those IDs must be sourced
    /// from BDS packet/native data before they are exposed to the protocol.
    pub id: u32,
    /// Sourced packet biome ID, when available.
    ///
    /// `None` means the protocol packet should emit Bedrock's vanilla sentinel
    /// (`0xffff`) instead of inventing a numeric ID.
    pub packet_id: Option<u16>,
    /// String identifier (e.g., "minecraft:plains").
    pub string_id: String,
    /// Display name.
    pub name: String,
    /// Biome category.
    pub category: String,
    /// Dimension.
    pub dimension: String,
    /// Temperature.
    pub temperature: f32,
    /// Rainfall/downfall value from the behavior-pack climate component.
    pub downfall: f32,
    /// Has precipitation (rain/snow).
    pub has_precipitation: bool,
    /// Map color (RGB).
    pub color: u32,
}

impl RegistryEntry for BiomeEntry {
    fn id(&self) -> u32 {
        self.id
    }

    fn string_id(&self) -> &str {
        &self.string_id
    }
}

/// Biome registry type alias.
pub type BiomeRegistry = Registry<BiomeEntry>;

impl BiomeRegistry {
    /// Load vanilla biomes from behavior-pack sourced generated data.
    ///
    /// Numeric packet/runtime biome IDs are deliberately not sourced here. The
    /// behavior pack is authoritative for names, tags, and climate, while packet
    /// IDs must come from BDS packet/native captures when available.
    pub fn load_vanilla(&mut self) {
        use unastar_data::biomes::ALL_BIOMES;

        for (id, biome) in ALL_BIOMES.iter().enumerate() {
            let climate = biome.climate;
            let entry = BiomeEntry {
                id: id as u32,
                packet_id: None,
                string_id: biome.identifier.to_string(),
                name: biome_display_name(biome.identifier),
                category: biome_category(biome.tags).to_string(),
                dimension: biome_dimension(biome.tags).to_string(),
                temperature: climate.map(|value| value.temperature).unwrap_or(0.5),
                downfall: climate.map(|value| value.downfall).unwrap_or(0.0),
                has_precipitation: climate.map(|value| value.downfall > 0.0).unwrap_or(false),
                // Water/map colour is not present in behavior-pack biome JSON.
                // Keep this neutral until sourced from BDS packet/native data.
                color: 0,
            };
            let _ = self.register(entry);
        }
    }

    /// Convert registry to protocol packet with string interning.
    /// Match Go format: -1 for vanilla biome IDs, proper string interning
    pub fn to_packet(&self) -> jolyne::valentine::BiomeDefinitionListPacket {
        use jolyne::valentine::BiomeDefinition;
        use std::collections::HashMap;

        let mut string_list: Vec<String> = Vec::new();
        let mut string_index: HashMap<String, usize> = HashMap::new();

        // String interning helper (matches Go intern function)
        let mut intern = |s: &str| -> usize {
            if let Some(&idx) = string_index.get(s) {
                return idx;
            }
            let idx = string_list.len();
            string_list.push(s.to_string());
            string_index.insert(s.to_string(), idx);
            idx
        };

        const VANILLA_BIOME_PACKET_SENTINEL: u16 = (-1i16) as u16;

        let biome_definitions: Vec<BiomeDefinition> = self
            .iter()
            .map(|biome| {
                let name_index = intern(&biome.string_id) as i16;
                let biome_id = biome.packet_id.unwrap_or(VANILLA_BIOME_PACKET_SENTINEL);

                // Water color: ARGB packed into i32 (big endian)
                // Format: A << 24 | R << 16 | G << 8 | B
                let color_bytes = biome.color.to_be_bytes();
                let map_water_colour = i32::from_be_bytes(color_bytes);

                BiomeDefinition {
                    name_index,
                    biome_id,
                    temperature: biome.temperature,
                    downfall: biome.downfall,
                    snow_foliage: 0.0,
                    depth: 0.0,
                    scale: 0.0,
                    map_water_colour,
                    rain: biome.has_precipitation,
                    tags: None, // TODO: Add tags when available
                    chunk_generation: None,
                }
            })
            .collect();

        jolyne::valentine::BiomeDefinitionListPacket {
            biome_definitions,
            string_list,
        }
    }
}

fn biome_display_name(identifier: &str) -> String {
    identifier
        .strip_prefix("minecraft:")
        .unwrap_or(identifier)
        .replace('_', " ")
}

fn biome_dimension(tags: &[&str]) -> &'static str {
    if tags.contains(&"nether") {
        "nether"
    } else if tags.contains(&"the_end") {
        "the_end"
    } else {
        "overworld"
    }
}

fn biome_category<'a>(tags: &'a [&'a str]) -> &'a str {
    const NON_CATEGORY_TAGS: &[&str] = &[
        "animal",
        "monster",
        "overworld",
        "nether",
        "the_end",
        "warm",
        "spawns_warm_variant_farm_animals",
        "spawns_snow_variant_farm_animals",
        "spawns_cold_variant_farm_animals",
        "spawns_jungle_mobs",
        "spawn_ghast",
        "spawn_many_magma_cubes",
    ];

    tags.iter()
        .copied()
        .find(|tag| !NON_CATEGORY_TAGS.contains(tag))
        .or_else(|| tags.first().copied())
        .unwrap_or("unknown")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_biomes_are_loaded_from_unastar_data() {
        let mut registry = BiomeRegistry::new();

        registry.load_vanilla();

        assert_eq!(registry.len(), unastar_data::biomes::ALL_BIOMES.len());
        let plains = registry
            .get_by_name("minecraft:plains")
            .expect("plains biome");
        let source = unastar_data::biomes::get("minecraft:plains").expect("source plains");
        let climate = source.climate.expect("plains climate");
        assert_eq!(plains.string_id, source.identifier);
        assert_eq!(plains.temperature, climate.temperature);
        assert_eq!(plains.downfall, climate.downfall);
        assert_eq!(plains.has_precipitation, climate.downfall > 0.0);
        assert_eq!(plains.dimension, "overworld");
    }

    #[test]
    fn vanilla_biome_packet_does_not_invent_numeric_ids() {
        let mut registry = BiomeRegistry::new();
        registry.load_vanilla();

        let packet = registry.to_packet();

        assert_eq!(
            packet.biome_definitions.len(),
            unastar_data::biomes::ALL_BIOMES.len()
        );
        assert!(
            packet
                .biome_definitions
                .iter()
                .all(|definition| definition.biome_id == (-1i16) as u16)
        );
        let plains_index = packet
            .string_list
            .iter()
            .position(|name| name == "minecraft:plains")
            .expect("plains string");
        let plains_packet = packet
            .biome_definitions
            .iter()
            .find(|definition| definition.name_index as usize == plains_index)
            .expect("plains packet definition");
        let plains_source = unastar_data::biomes::get("minecraft:plains")
            .and_then(|biome| biome.climate)
            .expect("plains climate");
        assert_eq!(plains_packet.downfall, plains_source.downfall);
    }
}
