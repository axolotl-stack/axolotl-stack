//! Biome registry for runtime biome management.

use super::{Registry, RegistryEntry};

/// Runtime biome entry in the registry.
#[derive(Debug, Clone)]
pub struct BiomeEntry {
    /// Native Bedrock biome ID, also used as the registry identity.
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
    /// Native rainfall/downfall value.
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
    /// Load native biome identities and climate, with behavior-pack gameplay enrichment.
    pub fn load_vanilla(&mut self) {
        use unastar_data::biomes::ALL_BIOMES;

        for biome in &ALL_BIOMES {
            let climate = biome.climate;
            let entry = BiomeEntry {
                id: biome.id,
                packet_id: Some(
                    u16::try_from(biome.id).expect("native biome ID exceeds packet range"),
                ),
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
    /// Match the map-of-biome-names-to-data wire shape, preserving sourced native IDs.
    pub fn to_packet(&self) -> jolyne::valentine::BiomeDefinitionListPacket {
        use jolyne::valentine::bedrock::codec::BedrockString;
        use jolyne::valentine::{
            BiomeDefinitionData, BiomeDefinitionListPacketMapofBiomenamestodataItem,
            BiomeStringList,
        };
        use std::collections::HashMap;

        let mut string_list = Vec::new();
        let mut string_index: HashMap<BedrockString, usize> = HashMap::new();

        // String interning helper (matches Go intern function)
        let mut intern = |s: &str| -> usize {
            if let Some(&idx) = string_index.get(s) {
                return idx;
            }
            let idx = string_list.len();
            let value: BedrockString = s.into();
            string_list.push(value.clone());
            string_index.insert(value, idx);
            idx
        };

        const VANILLA_BIOME_PACKET_SENTINEL: u16 = (-1i16) as u16;

        let biome_definitions = self
            .iter()
            .map(|biome| {
                let name_index = intern(&biome.string_id) as u16;
                let biome_id = biome.packet_id.unwrap_or(VANILLA_BIOME_PACKET_SENTINEL);

                // Water color: ARGB packed into i32 (big endian)
                // Format: A << 24 | R << 16 | G << 8 | B
                let color_bytes = biome.color.to_be_bytes();
                let map_water_colour = i32::from_be_bytes(color_bytes);

                BiomeDefinitionListPacketMapofBiomenamestodataItem {
                    key: name_index,
                    value: BiomeDefinitionData {
                        id: biome_id,
                        temperature: biome.temperature,
                        downfall: biome.downfall,
                        foliagesnow: 0.0,
                        depth: 0.0,
                        scale: 0.0,
                        mapwatercolor_argb: map_water_colour,
                        rain: biome.has_precipitation,
                        tags: None, // TODO: Add tags when available
                        chunkgendata: None,
                    },
                }
            })
            .collect();

        jolyne::valentine::BiomeDefinitionListPacket {
            mapof_biomenamestodata: biome_definitions,
            stringlist: BiomeStringList {
                strings: string_list,
            },
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
    fn vanilla_biome_packet_preserves_native_numeric_ids() {
        let mut registry = BiomeRegistry::new();
        registry.load_vanilla();

        let packet = registry.to_packet();

        assert_eq!(
            packet.mapof_biomenamestodata.len(),
            unastar_data::biomes::ALL_BIOMES.len()
        );
        for definition in &packet.mapof_biomenamestodata {
            let name = &packet.stringlist.strings[usize::from(definition.key)];
            assert_eq!(
                u32::from(definition.value.id),
                unastar_data::biomes::get(name).unwrap().id
            );
        }
        let plains_index = packet
            .stringlist
            .strings
            .iter()
            .position(|name| name == "minecraft:plains")
            .expect("plains string");
        let plains_packet = packet
            .mapof_biomenamestodata
            .iter()
            .find(|definition| definition.key as usize == plains_index)
            .expect("plains packet definition");
        let plains_source = unastar_data::biomes::get("minecraft:plains")
            .and_then(|biome| biome.climate)
            .expect("plains climate");
        assert_eq!(plains_packet.value.downfall, plains_source.downfall);
    }
}
