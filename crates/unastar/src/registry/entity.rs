//! Entity registry for runtime entity management.

use super::{Registry, RegistryEntry};
use bytes::BytesMut;
use jolyne::valentine::AvailableActorIdentifiersPacket;
use jolyne::valentine::bedrock::codec::Nbt;

/// Runtime entity entry in the registry.
#[derive(Debug, Clone)]
pub struct EntityEntry {
    /// Internal registry ID.
    ///
    /// This is not a Bedrock packet/runtime entity ID.
    pub id: u32,
    /// Sourced Bedrock runtime entity ID, when present.
    pub runtime_id: Option<u32>,
    /// String identifier (e.g., "minecraft:zombie").
    pub string_id: String,
    /// Display name.
    pub name: String,
    /// Vanilla spawn category, when present in behavior-pack data.
    pub spawn_category: Option<String>,
    /// Whether this entity can spawn naturally.
    pub is_spawnable: bool,
    /// Whether this entity can be summoned by commands.
    pub is_summonable: bool,
    /// Hitbox dimensions.
    pub height: Option<f32>,
    pub width: Option<f32>,
}

impl RegistryEntry for EntityEntry {
    fn id(&self) -> u32 {
        self.id
    }

    fn string_id(&self) -> &str {
        &self.string_id
    }
}

/// Entity registry type alias.
pub type EntityRegistry = Registry<EntityEntry>;

impl EntityRegistry {
    /// Load vanilla entities from behavior-pack sourced generated data.
    ///
    /// Entity metadata comes from `unastar-data` behavior-pack extraction. Exact
    /// packet payloads and runtime IDs should come from BDS packet/native
    /// captures when present rather than Valentine/Prismarine generated data.
    pub fn load_vanilla(&mut self) {
        use unastar_data::entities::definitions::ALL_ENTITIES;

        for entity in ALL_ENTITIES.iter() {
            let entry = EntityEntry {
                id: entity.id,
                runtime_id: entity.runtime_id,
                string_id: entity.identifier.to_string(),
                name: entity_display_name(entity.identifier),
                spawn_category: entity.spawn_category.map(ToString::to_string),
                is_spawnable: entity.is_spawnable,
                is_summonable: entity.is_summonable,
                height: entity.height,
                width: entity.width,
            };
            let _ = self.register(entry);
        }
    }

    /// Convert registry to an `AvailableActorIdentifiers` packet.
    ///
    /// This matches Dragonfly's `AvailableActorIdentifiers` payload: an NBT compound containing an
    /// `idlist` list of compounds, each with a single string field `id`.
    pub fn to_available_entity_identifiers_packet(&self) -> AvailableActorIdentifiersPacket {
        fn write_var_u32(buf: &mut BytesMut, mut v: u32) {
            while v >= 0x80 {
                buf.extend_from_slice(&[(v as u8) | 0x80]);
                v >>= 7;
            }
            buf.extend_from_slice(&[v as u8]);
        }

        fn write_zigzag32(buf: &mut BytesMut, v: i32) {
            let encoded = ((v << 1) ^ (v >> 31)) as u32;
            write_var_u32(buf, encoded);
        }

        fn write_string(buf: &mut BytesMut, s: &str) {
            write_var_u32(buf, s.len() as u32);
            buf.extend_from_slice(s.as_bytes());
        }

        let mut buf = BytesMut::new();

        // Root tag: Compound ("")
        buf.extend_from_slice(&[0x0A]);
        write_string(&mut buf, "");

        // "idlist": List of Compounds
        buf.extend_from_slice(&[0x09]);
        write_string(&mut buf, "idlist");
        buf.extend_from_slice(&[0x0A]); // list element type = Compound

        let count = self.len() as i32;
        write_zigzag32(&mut buf, count);

        for entity in self.iter() {
            // Each element is a compound payload (unnamed): { id: "<string>" }
            buf.extend_from_slice(&[0x08]); // String tag
            write_string(&mut buf, "id");
            write_string(&mut buf, &entity.string_id);
            buf.extend_from_slice(&[0x00]); // End compound
        }

        // End root compound
        buf.extend_from_slice(&[0x00]);

        AvailableActorIdentifiersPacket {
            identifier_list: Nbt(buf.freeze()),
        }
    }
}

fn entity_display_name(identifier: &str) -> String {
    identifier
        .strip_prefix("minecraft:")
        .unwrap_or(identifier)
        .replace('_', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_entities_are_loaded_from_unastar_data() {
        let mut registry = EntityRegistry::new();

        registry.load_vanilla();

        assert_eq!(
            registry.len(),
            unastar_data::entities::definitions::ALL_ENTITIES.len()
        );
        let zombie = registry
            .get_by_name("minecraft:zombie")
            .expect("zombie entity");
        let source =
            unastar_data::entities::definitions::get("minecraft:zombie").expect("source zombie");
        assert_eq!(zombie.id, source.id);
        assert_eq!(zombie.runtime_id, source.runtime_id);
        assert_eq!(zombie.string_id, source.identifier);
        assert_eq!(zombie.spawn_category.as_deref(), source.spawn_category);
        assert_eq!(zombie.is_spawnable, source.is_spawnable);
        assert_eq!(zombie.is_summonable, source.is_summonable);
        assert_eq!(zombie.height, source.height);
        assert_eq!(zombie.width, source.width);
    }

    #[test]
    fn available_entity_packet_uses_behavior_pack_identifiers() {
        let mut registry = EntityRegistry::new();
        registry.load_vanilla();

        let packet = registry.to_available_entity_identifiers_packet();

        assert!(!packet.identifier_list.0.is_empty());
        assert_eq!(
            registry.len(),
            unastar_data::entities::definitions::ALL_ENTITIES.len()
        );
    }
}
