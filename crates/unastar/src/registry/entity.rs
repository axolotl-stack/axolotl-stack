//! Entity registry for runtime entity management.

use super::{Registry, RegistryEntry};
use bevy_ecs::prelude::Resource;
use bytes::BytesMut;
use jolyne::valentine::AvailableActorIdentifiersPacket;
use jolyne::valentine::bedrock::codec::Nbt;
use std::collections::BTreeMap;
use std::sync::Arc;
use unastar_data::actor_definitions::ActorDefinition;

/// Runtime entity entry in the registry.
#[derive(Debug, Clone)]
pub struct EntityEntry {
    /// Internal registry ID.
    ///
    /// This is not a Bedrock packet/runtime entity ID.
    pub id: u32,
    /// Captured static actor type id (`rid`), not an actor instance id.
    pub runtime_id: u32,
    /// Captured base identifier (`bid`).
    pub base_identifier: String,
    pub has_spawn_egg: bool,
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
    /// Join pack definitions with captured wire identities. JSON-less built-in
    /// actors remain present; pack metadata never guesses their wire fields.
    pub fn load_vanilla(&mut self, definitions: &ActorDefinitionRegistry) {
        for (id, captured) in unastar_data::actor_identifiers::vanilla()
            .iter()
            .enumerate()
        {
            let actor = definitions.get(&captured.id);
            let description = actor.map(|d| &d.description);
            let collision = actor.and_then(|d| d.components.get("minecraft:collision_box"));
            self.register(EntityEntry {
                id: id as u32,
                runtime_id: captured.rid,
                string_id: captured.id.clone(),
                base_identifier: captured.bid.clone(),
                has_spawn_egg: captured.hasspawnegg != 0,
                name: entity_display_name(&captured.id),
                spawn_category: description.and_then(|d| d.spawn_category.clone()),
                is_spawnable: description.is_some_and(|d| d.is_spawnable),
                is_summonable: captured.summonable != 0,
                height: collision
                    .and_then(|c| c["height"].as_f64())
                    .map(|n| n as f32),
                width: collision
                    .and_then(|c| c["width"].as_f64())
                    .map(|n| n as f32),
            })
            .expect("unique captured actor identifier");
        }
    }

    /// Convert registry to an `AvailableActorIdentifiers` packet.
    ///
    /// BDS capture order and all five fields are preserved. Members of each
    /// NBT compound use BDS's lexical field order.
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
            // Compound member order is part of byte-exact capture equality.
            buf.extend_from_slice(&[0x08]);
            write_string(&mut buf, "bid");
            write_string(&mut buf, &entity.base_identifier);
            buf.extend_from_slice(&[0x01]);
            write_string(&mut buf, "hasspawnegg");
            buf.extend_from_slice(&[u8::from(entity.has_spawn_egg)]);
            buf.extend_from_slice(&[0x08]); // String tag
            write_string(&mut buf, "id");
            write_string(&mut buf, &entity.string_id);
            buf.extend_from_slice(&[0x03]);
            write_string(&mut buf, "rid");
            write_zigzag32(
                &mut buf,
                i32::try_from(entity.runtime_id).expect("actor rid fits i32"),
            );
            buf.extend_from_slice(&[0x01]);
            write_string(&mut buf, "summonable");
            buf.extend_from_slice(&[u8::from(entity.is_summonable)]);
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

/// Definition registrations are mutable only before freeze (D5).
#[derive(Debug, Default)]
pub struct ActorDefinitionRegistryBuilder {
    definitions: BTreeMap<String, Arc<ActorDefinition>>,
}

impl ActorDefinitionRegistryBuilder {
    pub fn load_vanilla(&mut self) {
        for definition in unastar_data::actor_definitions::vanilla() {
            self.register(Arc::clone(definition));
        }
    }

    /// Whole-definition override by identifier, never a per-component merge.
    pub fn register(&mut self, definition: Arc<ActorDefinition>) -> Option<Arc<ActorDefinition>> {
        self.definitions
            .insert(definition.identifier().to_owned(), definition)
    }

    pub fn freeze(self) -> ActorDefinitionRegistry {
        ActorDefinitionRegistry {
            definitions: Arc::new(self.definitions),
        }
    }
}

#[derive(Debug, Clone, Resource)]
pub struct ActorDefinitionRegistry {
    definitions: Arc<BTreeMap<String, Arc<ActorDefinition>>>,
}

impl ActorDefinitionRegistry {
    pub fn vanilla() -> Self {
        let mut builder = ActorDefinitionRegistryBuilder::default();
        builder.load_vanilla();
        builder.freeze()
    }

    pub fn get(&self, identifier: &str) -> Option<&Arc<ActorDefinition>> {
        self.definitions.get(identifier)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<ActorDefinition>> {
        self.definitions.values()
    }
}

/// The identifier registry survives packet construction; gameplay and addon
/// startup use the same immutable registry that produced the cached packet.
#[derive(Resource, Clone)]
pub struct EntityRegistryResource(pub Arc<EntityRegistry>);

#[cfg(test)]
mod tests {
    use super::*;
    use jolyne::valentine::bedrock::codec::BedrockCodec;

    #[test]
    fn available_identifiers_equal_bds_1_26_51_byte_for_byte() {
        let definitions = ActorDefinitionRegistry::vanilla();
        let mut registry = EntityRegistry::new();
        registry.load_vanilla(&definitions);
        let actual = registry.to_available_entity_identifiers_packet();
        let capture = include_bytes!("testdata/bds-1.26.51-actor-identifiers.bin");
        assert_eq!(capture[0], 0x77);
        let mut payload = &capture[1..];
        let expected = AvailableActorIdentifiersPacket::decode(&mut payload, ()).unwrap();
        assert!(payload.is_empty());
        assert_eq!(actual.identifier_list.0, expected.identifier_list.0);
        // The old definition-only packet omitted these built-in actors.
        for id in [
            "minecraft:item",
            "minecraft:falling_block",
            "minecraft:painting",
            "minecraft:leash_knot",
        ] {
            assert!(registry.get_by_name(id).is_some(), "{id}");
        }
        assert_eq!(
            registry.get_by_name("minecraft:item").unwrap().runtime_id,
            64
        );
    }

    #[test]
    fn addon_registration_replaces_the_entire_definition_before_freeze() {
        let original = unastar_data::actor_definitions::get("minecraft:cow").unwrap();
        let mut replacement = original.as_ref().clone();
        replacement.components.clear();
        replacement.component_groups.clear();
        replacement.events.clear();
        let replacement = Arc::new(replacement);
        let mut builder = ActorDefinitionRegistryBuilder::default();
        builder.load_vanilla();
        let old = builder.register(Arc::clone(&replacement)).unwrap();
        assert!(Arc::ptr_eq(&old, original));
        let frozen = builder.freeze();
        let cow = frozen.get("minecraft:cow").unwrap();
        assert!(Arc::ptr_eq(cow, &replacement));
        assert!(
            cow.components.is_empty() && cow.component_groups.is_empty() && cow.events.is_empty()
        );
        assert!(
            frozen
                .get("minecraft:pig")
                .unwrap()
                .components
                .contains_key("minecraft:health")
        );
    }
}
