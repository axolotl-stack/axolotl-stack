//! Entity registry for runtime entity management.

use super::{Registry, RegistryEntry};
use bytes::BytesMut;
use jolyne::protocol::PacketAvailableEntityIdentifiers;
use jolyne::protocol::bedrock::codec::Nbt;
use jolyne::protocol::entities::EntityType;

/// Runtime entity entry in the registry.
#[derive(Debug, Clone)]
pub struct EntityEntry {
    /// Numeric entity ID.
    pub id: u32,
    /// Internal entity ID.
    pub internal_id: u32,
    /// String identifier (e.g., "minecraft:zombie").
    pub string_id: String,
    /// Display name.
    pub name: String,
    /// Entity behavior type.
    pub entity_type: EntityType,
    /// Hitbox dimensions.
    pub height: f32,
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
    /// Load vanilla entities from valentine's generated data.
    pub fn load_vanilla(&mut self) {
        use jolyne::protocol::entities::ALL_ENTITIES;

        for entity in ALL_ENTITIES.iter() {
            let entry = EntityEntry {
                id: entity.id,
                internal_id: entity.internal_id,
                string_id: entity.string_id.to_string(),
                name: entity.name.to_string(),
                entity_type: entity.entity_type,
                height: entity.height,
                width: entity.width,
            };
            let _ = self.register(entry);
        }
    }

    /// Convert registry to an `AvailableEntityIdentifiers` packet.
    ///
    /// This matches Dragonfly's `AvailableActorIdentifiers` payload: an NBT compound containing an
    /// `idlist` list of compounds, each with a single string field `id`.
    pub fn to_available_entity_identifiers_packet(&self) -> PacketAvailableEntityIdentifiers {
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

        PacketAvailableEntityIdentifiers {
            nbt: Nbt(buf.freeze()),
        }
    }
}
