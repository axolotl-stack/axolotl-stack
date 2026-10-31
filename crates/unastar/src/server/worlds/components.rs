//! Explicit ownership-preserving component migration between ECS worlds.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use bevy_ecs::prelude::*;

/// A component's owned data is extracted only after the transfer is accepted.
trait Mover: Send + Sync {
    fn extract(&self, world: &mut World, entity: Entity) -> Option<Box<dyn Any + Send + Sync>>;
    fn insert(&self, world: &mut World, entity: Entity, value: Box<dyn Any + Send + Sync>);
}
struct TypedMover<T: Component> {
    preserve: bool,
    marker: PhantomData<T>,
}
impl<T: Component> Mover for TypedMover<T> {
    /// Move a component without requiring Clone or serializing application data.
    fn extract(&self, world: &mut World, entity: Entity) -> Option<Box<dyn Any + Send + Sync>> {
        let value = world.entity_mut(entity).take::<T>()?;
        self.preserve
            .then(|| Box::new(value) as Box<dyn Any + Send + Sync>)
    }
    /// Reinsert exactly the registered type in its new ECS world.
    fn insert(&self, world: &mut World, entity: Entity, value: Box<dyn Any + Send + Sync>) {
        world.entity_mut(entity).insert(
            *value
                .downcast::<T>()
                .expect("matching typed component migration"),
        );
    }
}

/// Native and application component ownership rules for player world transfers.
/// Unregistered components reject the transfer before any source mutation.
/// Applications register their own stable account/context components; components
/// containing world-local Entity references need a deliberate reset or a
/// projection rebuilt by the committed transfer callback.
#[derive(Resource, Clone)]
pub struct TransferComponents {
    movers: HashMap<TypeId, Arc<dyn Mover>>,
}
impl Default for TransferComponents {
    /// Include the native contract before applications extend it with owned state.
    fn default() -> Self {
        native_components()
    }
}

impl TransferComponents {
    /// Preserve a component's exact owned value, including non-Clone types.
    pub fn preserve<T: Component>(&mut self) {
        self.movers.insert(
            TypeId::of::<T>(),
            Arc::new(TypedMover::<T> {
                preserve: true,
                marker: PhantomData,
            }),
        );
    }
    /// Explicitly discard a world-local component at an accepted boundary.
    pub fn reset<T: Component>(&mut self) {
        self.movers.insert(
            TypeId::of::<T>(),
            Arc::new(TypedMover::<T> {
                preserve: false,
                marker: PhantomData,
            }),
        );
    }
    /// Report unregistered components without removing or cloning any data.
    pub(super) fn unsupported(&self, world: &World, entity: Entity) -> Vec<String> {
        let Ok(entity) = world.get_entity(entity) else {
            return vec!["missing player".into()];
        };
        entity
            .archetype()
            .components()
            .iter()
            .filter_map(|id| {
                let info = world.components().get_info(*id)?;
                (!info
                    .type_id()
                    .is_some_and(|id| self.movers.contains_key(&id)))
                .then(|| info.name().to_string())
            })
            .collect()
    }
    /// Extract a prevalidated set without borrowing its registry during moves.
    pub(super) fn extract(&self, world: &mut World, entity: Entity) -> MovedComponents {
        let movers: Vec<_> = world
            .entity(entity)
            .archetype()
            .components()
            .iter()
            .filter_map(|id| {
                world
                    .components()
                    .get_info(*id)
                    .and_then(|info| info.type_id())
                    .and_then(|id| self.movers.get(&id))
                    .cloned()
            })
            .collect();
        MovedComponents(
            movers
                .into_iter()
                .filter_map(|mover| mover.extract(world, entity).map(|value| (mover, value)))
                .collect(),
        )
    }
}

/// The accepted operation owns every preserved value until destination insertion.
pub(super) struct MovedComponents(Vec<(Arc<dyn Mover>, Box<dyn Any + Send + Sync>)>);
impl MovedComponents {
    /// Install transferred values in a fresh destination entity.
    pub(super) fn install(self, world: &mut World, entity: Entity) {
        for (mover, value) in self.0 {
            mover.insert(world, entity, value);
        }
    }
}

/// Register the native persistent and world-local player component contracts.
pub(super) fn native_components() -> TransferComponents {
    use crate::entity::components::*;
    use crate::server::game;
    use crate::world::ecs::*;
    let mut all = TransferComponents {
        movers: HashMap::new(),
    };
    macro_rules! preserve {($($component:ty),* $(,)?)=>{$(all.preserve::<$component>();)*};}
    macro_rules! reset {($($component:ty),* $(,)?)=>{$(all.reset::<$component>();)*};}
    preserve!(
        Player,
        PlayerName,
        PlayerUuid,
        PlayerSession,
        RuntimeEntityId,
        Rotation,
        GameMode,
        ChunkRadius,
        MainInventory,
        ArmourInventory,
        OffhandSlot,
        HeldSlot,
        PlayerUi,
        EnderChestInventory,
        Health,
        Hunger,
        Experience,
        Effects,
        PlayerNetworkMetrics,
        PlayerEquipmentProjection,
        game::inventory_effects::InventoryPersistence,
        game::hunger::FoodTimer,
        game::hunger::PlayerFoodPolicy,
        game::attributes::Absorption,
        crate::command::Operator,
        crate::entity::player_life::environment::Burning,
        crate::entity::mob::combat::HurtTimer,
        crate::entity::mob::state::LastHurt,
        Velocity,
        OnGround,
        OnFire,
        AirSupply,
        Speed,
        Age,
        crate::entity::freezing::FreezingState,
        crate::entity::freezing::FreezingImmune,
        crate::entity::freezing::FreezingVulnerable,
        crate::entity::definition::metadata::EntityMetadata
    );
    reset!(
        Position,
        PlayerState,
        PlayerInput,
        BreakingState,
        SpatialChunk,
        LastBroadcastPosition,
        ChunkLoader,
        LastPublisherState,
        ContainerWindows,
        game::RequestIdAssignments,
        AwaitingSpawnStatus,
        game::player_events::InitialStatePublished,
        game::attributes::SentAttributes,
        game::effects::SentEffects,
        crate::entity::player_life::environment::PlayerFall,
        crate::entity::player_life::respawn::PlayerSpawnPoint,
        ActorAudience,
        ActorViewers,
        game::item_use::ItemUsing
    );
    crate::command::exec::register_transfer_components(&mut all);
    all
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_initial_native_player_component_has_an_explicit_migration_contract() {
        let mut server = crate::GameServer::with_config(crate::ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        let (tx, _rx) = tokio::sync::mpsc::channel(128);
        let player = server
            .spawn_player(crate::PlayerSpawnData {
                session_id: 1,
                display_name: "actor".into(),
                xuid: Some("account".into()),
                uuid: Some(uuid::Uuid::from_u128(1).to_string()),
                runtime_id: 77,
                position: glam::DVec3::new(0.5, 65.62, 0.5),
                saved_data: None,
                outbound_tx: tx,
                chunk_radius: 0,
            })
            .unwrap();
        assert!(crate::command::exec::set_mode_override(
            server.ecs.world_mut(),
            player,
            Some(crate::entity::components::GameMode::Creative)
        ));
        let unsupported = native_components().unsupported(server.ecs.world(), player);
        assert!(
            unsupported.is_empty(),
            "missing native migration contracts: {unsupported:?}"
        );
    }

    #[test]
    fn component_moves_preserve_nonclone_values_and_reject_unregistered_state() {
        #[derive(Component)]
        struct Owned(String);
        #[derive(Component)]
        struct Local;
        let mut source = World::new();
        let mut destination = World::new();
        let player = source.spawn((Owned("value".into()), Local)).id();
        let target = destination.spawn_empty().id();
        let mut rules = TransferComponents::default();
        rules.preserve::<Owned>();
        assert_eq!(rules.unsupported(&source, player).len(), 1);
        rules.reset::<Local>();
        assert!(rules.unsupported(&source, player).is_empty());
        rules
            .extract(&mut source, player)
            .install(&mut destination, target);
        assert_eq!(destination.get::<Owned>(target).unwrap().0, "value");
        assert!(destination.get::<Local>(target).is_none());
        assert!(source.get::<Owned>(player).is_none());
    }
}
