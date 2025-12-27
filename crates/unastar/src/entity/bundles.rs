//! Component bundles for spawning common entity types.

use bevy_ecs::prelude::*;

use super::components::*;

/// Bundle for spawning a player entity.
/// Contains all components needed to spawn a player in the ECS.
#[derive(Bundle)]
pub struct PlayerBundle {
    pub player: Player,
    pub name: PlayerName,
    pub uuid: PlayerUuid,
    pub session: PlayerSession,
    pub runtime_id: RuntimeEntityId,
    pub position: Position,
    pub rotation: Rotation,
    pub game_mode: GameMode,
    pub state: PlayerState,
    pub input: PlayerInput,
    pub chunk_radius: ChunkRadius,
    pub breaking_state: BreakingState,
    pub spatial_chunk: SpatialChunk,
    pub pending_spawn: PendingSpawnBroadcast,
    // Inventory components
    pub main_inventory: MainInventory,
    pub armour: ArmourInventory,
    pub offhand: OffhandSlot,
    pub held_slot: HeldSlot,
    pub cursor: CursorItem,
    pub inventory_opened: InventoryOpened,
    pub item_stack_state: ItemStackRequestState,
}

/// Bundle for spawning a basic living entity (mob).
#[derive(Bundle)]
pub struct LivingBundle {
    pub living: Living,
    pub position: Position,
    pub velocity: Velocity,
    pub rotation: Rotation,
    pub on_ground: OnGround,
    pub runtime_id: RuntimeId,
    pub health: Health,
    pub effects: Effects,
    pub speed: Speed,
    pub age: Age,
}

/// Bundle for spawning a mob entity.
#[derive(Bundle)]
pub struct MobBundle {
    pub mob: Mob,
    pub mob_type: MobType,
    pub living: Living,
    pub position: Position,
    pub velocity: Velocity,
    pub rotation: Rotation,
    pub on_ground: OnGround,
    pub runtime_id: RuntimeId,
    pub health: Health,
    pub effects: Effects,
    pub speed: Speed,
    pub ai_state: AiState,
    pub hostile: Hostile,
    pub age: Age,
}

/// Bundle for spawning a dropped item entity.
#[derive(Bundle)]
pub struct ItemBundle {
    pub dropped_item: DroppedItem,
    pub item_data: ItemStackData,
    pub position: Position,
    pub velocity: Velocity,
    pub rotation: Rotation,
    pub on_ground: OnGround,
    pub runtime_id: RuntimeId,
    pub pickup_delay: PickupDelay,
    pub item_owner: ItemOwner,
    pub despawn_timer: DespawnTimer,
    pub age: Age,
}

/// Bundle for spawning a projectile entity.
#[derive(Bundle)]
pub struct ProjectileBundle {
    pub projectile: Projectile,
    pub projectile_data: ProjectileData,
    pub position: Position,
    pub velocity: Velocity,
    pub rotation: Rotation,
    pub runtime_id: RuntimeId,
    pub hit_state: ProjectileHit,
    pub pickup_mode: PickupMode,
    pub age: Age,
}
