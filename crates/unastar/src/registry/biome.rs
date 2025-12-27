//! Biome registry for runtime biome management.

use super::{Registry, RegistryEntry};

/// Runtime biome entry in the registry.
#[derive(Debug, Clone)]
pub struct BiomeEntry {
    /// Numeric biome ID.
    pub id: u32,
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
    /// Load vanilla biomes from valentine's generated data.
    pub fn load_vanilla(&mut self) {
        use jolyne::protocol::biomes::ALL_BIOMES;

        for biome in ALL_BIOMES.iter() {
            let entry = BiomeEntry {
                id: biome.id,
                string_id: biome.string_id.to_string(),
                name: biome.name.to_string(),
                category: biome.category.to_string(),
                dimension: biome.dimension.to_string(),
                temperature: biome.temperature,
                has_precipitation: biome.has_precipitation,
                color: biome.color,
            };
            let _ = self.register(entry);
        }
    }

    /// Convert registry to protocol packet with string interning.
    /// Match Go format: -1 for vanilla biome IDs, proper string interning
    pub fn to_packet(&self) -> jolyne::protocol::PacketBiomeDefinitionList {
        use jolyne::protocol::types::biome::BiomeDefinition;
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

        // Max vanilla biome ID - IDs above this get their actual ID, vanilla gets -1
        const MAX_VANILLA_BIOME_ID: u32 = 182;

        let biome_definitions: Vec<BiomeDefinition> = self
            .iter()
            .map(|biome| {
                let name_index = intern(&biome.string_id) as i16;

                // Vanilla biomes get -1 (0xFFFF as u16), custom biomes get their ID
                let biome_id: u16 = if biome.id > MAX_VANILLA_BIOME_ID {
                    biome.id as u16
                } else {
                    (-1i16) as u16 // 0xFFFF
                };

                // Water color: ARGB packed into i32 (big endian)
                // Format: A << 24 | R << 16 | G << 8 | B
                let color_bytes = biome.color.to_be_bytes();
                let map_water_colour = i32::from_be_bytes(color_bytes);

                // Downfall/rainfall (use temperature proxy for now)
                let downfall = if biome.has_precipitation { 0.5 } else { 0.0 };

                BiomeDefinition {
                    name_index,
                    biome_id,
                    temperature: biome.temperature,
                    downfall,
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

        jolyne::protocol::PacketBiomeDefinitionList {
            biome_definitions,
            string_list,
        }
    }
}
