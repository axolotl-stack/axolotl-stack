//! Native ownership of complete state while a restoration is awaiting persistence.

use super::*;
use crate::server::game::player_events::{ConnectionIdentity, DetachedPlayer};
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

const HELD: u8 = 0;
const RELEASED: u8 = 1;
const GONE: u8 = 2;

/// The engine keeps accepted frozen buffers independently of guest resource handles.
#[derive(Clone)]
pub(crate) struct Handle {
    pub(crate) player: Entity,
    id: uuid::Uuid,
    life: Arc<AtomicU8>,
}
impl Handle {
    /// This is a lifetime observation, never authority for a replacement actor.
    pub(crate) fn is_gone(&self) -> bool {
        self.life.load(Ordering::Acquire) == GONE
    }
    /// Activation released these exact buffers without implying that their values were consumed.
    pub(crate) fn is_released(&self) -> bool {
        self.life.load(Ordering::Acquire) == RELEASED
    }
}

/// A source hold is permanent until a separate explicit retirement operation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Source,
    Target,
}

struct Owner {
    uuid: uuid::Uuid,
    runtime: i64,
    name: String,
    connection: Option<ConnectionIdentity>,
    xuid: Option<String>,
}
impl Owner {
    /// Keep exactly the original connection, or its actual retained sessionless actor.
    fn matches(&self, world: &World, player: Entity) -> bool {
        world.get::<Player>(player).is_some()
            && world
                .get::<PlayerUuid>(player)
                .is_some_and(|id| id.0 == self.uuid)
            && world
                .get::<RuntimeEntityId>(player)
                .is_some_and(|id| id.0 == self.runtime)
            && world
                .get::<crate::entity::components::PlayerName>(player)
                .is_some_and(|name| name.0 == self.name)
            && match (&self.connection, world.get::<PlayerSession>(player)) {
                (Some(connection), Some(_)) => connection.matches(world, player),
                (_, None) => {
                    world.get::<DetachedPlayer>(player).is_some()
                        && account_xuid(world, player) == self.xuid.as_deref()
                }
                (None, Some(_)) => false,
            }
    }
}

macro_rules! held_fields {
    ($($field:ident:$ty:ty),* $(,)?)=>{
        struct Fields {$($field:Option<$ty>,)*}
        impl Fields {
            /// Native writers cannot replace a protected buffer during transfer.
            fn withheld(world:&World,player:Entity)->bool {true $(&&world.get::<$ty>(player).is_none())*}
            /// Move actual components without changing their network slot identities.
            fn take(world:&mut World,player:Entity)->Self {
                let mut entity=world.entity_mut(player);
                Self {$($field:entity.take::<$ty>(),)*}
            }
            /// Only a proven rejection reinstalls the exact original components.
            fn put(self,world:&mut World,player:Entity) {
                {
                    let mut entity=world.entity_mut(player);
                    $(if let Some(value)=self.$field {entity.insert(value);})*
                }
                reset_input(world,player);
            }
        }
    }
}

/// A legitimate retained actor never inherits a departed connection's input ownership.
fn reset_input(world: &mut World, player: Entity) {
    if world.get::<PlayerSession>(player).is_some() {
        let tick = world
            .get::<crate::entity::components::PlayerInput>(player)
            .map_or(0, |input| input.tick);
        world
            .entity_mut(player)
            .insert(crate::entity::components::PlayerInput {
                tick,
                ..Default::default()
            });
    } else {
        world.entity_mut(player).remove::<(
            crate::entity::components::PlayerInput,
            BreakingState,
            super::super::item_use::ItemUsing,
        )>();
    }
}
held_fields! {
    main:MainInventory,armour:ArmourInventory,offhand:OffhandSlot,ui:PlayerUi,ender:EnderChestInventory,
    held:HeldSlot,health:Health,hunger:Hunger,experience:Experience,position:Position,rotation:Rotation,
    velocity:Velocity,effects:Effects,air:AirSupply,fire:OnFire,burning:Burning,absorption:Absorption,
    age:Age,cold:FreezingState,mode:GameMode,state:crate::entity::components::PlayerState,input:crate::entity::components::PlayerInput,
    breaking:BreakingState,using:super::super::item_use::ItemUsing,
    food_policy:super::super::hunger::PlayerFoodPolicy,food_timer:super::super::hunger::FoodTimer,
    mode_base:crate::command::exec::ModeOverride,flight:super::super::flight::FlightOverride,
    hurt:HurtTimer,last_hurt:LastHurt,fall:PlayerFall,
    cold_immune:crate::entity::freezing::FreezingImmune,cold_vulnerable:crate::entity::freezing::FreezingVulnerable,
    speed:crate::entity::components::Speed,metadata:crate::entity::definition::metadata::EntityMetadata,
    audience:crate::entity::components::ActorAudience,
    equipment_projection:crate::entity::components::PlayerEquipmentProjection,
}

#[derive(Component)]
struct Frozen {
    handle: Handle,
    owner: Owner,
    role: Role,
    fields: Option<Fields>,
    _snapshot: Arc<PlayerSnapshot>,
    _retention: Arc<dyn Send + Sync>,
}
impl Drop for Frozen {
    fn drop(&mut self) {
        let _ = self
            .handle
            .life
            .compare_exchange(HELD, GONE, Ordering::AcqRel, Ordering::Acquire);
    }
}

/// Every input, capture, autosave and inventory path shares this mechanical barrier.
pub(crate) fn is_held(world: &World, player: Entity) -> bool {
    world.get::<Frozen>(player).is_some()
}

/// Target ownership spans loading, provider work and any unresolved physical application.
pub(crate) fn account_pending(world: &World, uuid: uuid::Uuid, xuid: Option<&str>) -> bool {
    world.try_query::<&Frozen>().is_some_and(|mut query| {
        query.iter(world).any(|frozen| {
            frozen.role == Role::Target
                && (frozen.owner.uuid == uuid
                    || xuid
                        .filter(|xuid| !xuid.is_empty())
                        .is_some_and(|xuid| frozen.owner.xuid.as_deref() == Some(xuid)))
        })
    })
}

/// Freeze only a genuine unchanged source. No callback or native borrow is retained.
pub(crate) fn hold(
    world: &mut World,
    player: Entity,
    expected: Arc<PlayerSnapshot>,
    role: Role,
    retention: Arc<dyn Send + Sync>,
) -> Result<Handle, SnapshotError> {
    if super::super::inventory_effects::is_quarantined(world, player)
        || capture(world, player)? != *expected
    {
        return Err(invalid("state source changed before ownership transfer"));
    }
    let owner = Owner {
        uuid: world
            .get::<PlayerUuid>(player)
            .ok_or(SnapshotError::MissingPlayer)?
            .0,
        runtime: world
            .get::<RuntimeEntityId>(player)
            .ok_or(SnapshotError::MissingPlayer)?
            .0,
        name: world
            .get::<crate::entity::components::PlayerName>(player)
            .ok_or(SnapshotError::MissingPlayer)?
            .0
            .clone(),
        connection: ConnectionIdentity::capture(world, player),
        xuid: account_xuid(world, player).map(str::to_owned),
    };
    if !owner.matches(world, player) {
        return Err(SnapshotError::MissingPlayer);
    }
    let handle = Handle {
        player,
        id: uuid::Uuid::new_v4(),
        life: Arc::new(AtomicU8::new(HELD)),
    };
    close_window(world, player);
    let fields = Fields::take(world, player);
    super::super::player_events::discard_input(world, player);
    world.entity_mut(player).insert(Frozen {
        handle: handle.clone(),
        owner,
        role,
        fields: Some(fields),
        _snapshot: expected,
        _retention: retention,
    });
    Ok(handle)
}

/// Require the actual component and original ownership; a historical handle is insufficient.
pub(crate) fn matches(world: &World, handle: &Handle) -> bool {
    world.get::<Frozen>(handle.player).is_some_and(|frozen| {
        frozen.handle.id == handle.id
            && frozen.owner.matches(world, handle.player)
            && Fields::withheld(world, handle.player)
    })
}

/// Read only the exact genuine buffers still owned by this native source hold.
pub(crate) fn source_snapshot(world: &World, handle: &Handle) -> Option<Arc<PlayerSnapshot>> {
    if !matches(world, handle) {
        return None;
    }
    let frozen = world.get::<Frozen>(handle.player)?;
    (frozen.role == Role::Source).then(|| frozen._snapshot.clone())
}

/// Acknowledged receipts advance without replacing the actual held source's gameplay buffers.
pub(crate) fn acknowledge_source_receipts(
    world: &mut World,
    handle: &Handle,
    data: &PlayerData,
) -> bool {
    if !matches(world, handle) {
        return false;
    }
    let Some(mut frozen) = world.get_mut::<Frozen>(handle.player) else {
        return false;
    };
    if frozen.role != Role::Source || frozen._snapshot.data.uuid != data.uuid {
        return false;
    }
    let mut snapshot = (*frozen._snapshot).clone();
    snapshot.data.inventory_revision = data.inventory_revision;
    snapshot.data.inventory_effects = data.inventory_effects.clone();
    snapshot.data.restoration_checkpoint = None;
    snapshot.data.source_actor = None;
    frozen._snapshot = Arc::new(snapshot);
    true
}

/// A source becomes physically active only after its canonical activation is acknowledged.
pub(crate) fn release_source(world: &mut World, handle: &Handle) -> bool {
    if !matches(world, handle)
        || world
            .get::<Frozen>(handle.player)
            .is_none_or(|frozen| frozen.role != Role::Source)
    {
        return false;
    }
    let mut frozen = world
        .entity_mut(handle.player)
        .take::<Frozen>()
        .expect("validated held source");
    let snapshot = frozen._snapshot.clone();
    frozen.handle.life.store(RELEASED, Ordering::Release);
    frozen
        .fields
        .take()
        .expect("owned source buffers")
        .put(world, handle.player);
    if restore(world, handle.player, &snapshot).is_err() {
        frozen.fields = Some(Fields::take(world, handle.player));
        frozen.handle.life.store(HELD, Ordering::Release);
        world.entity_mut(handle.player).insert(frozen);
        return false;
    }
    true
}

/// Reinstall exact target state after a proven pre-write rejection, never a source fence.
pub(crate) fn reject(world: &mut World, handle: &Handle) -> bool {
    if !matches(world, handle)
        || world
            .get::<Frozen>(handle.player)
            .is_none_or(|frozen| frozen.role != Role::Target)
    {
        return false;
    }
    let mut frozen = world
        .entity_mut(handle.player)
        .take::<Frozen>()
        .expect("validated held target");
    frozen.handle.life.store(RELEASED, Ordering::Release);
    frozen
        .fields
        .take()
        .expect("owned original fields")
        .put(world, handle.player);
    true
}

/// Apply acknowledged raw state only to the original still-held target actor.
pub(crate) fn apply(
    world: &mut World,
    handle: &Handle,
    snapshot: &PlayerSnapshot,
) -> Result<bool, SnapshotError> {
    if !matches(world, handle) {
        return Ok(false);
    }
    if world
        .get::<Frozen>(handle.player)
        .is_none_or(|frozen| frozen.role != Role::Target)
    {
        return Err(invalid("invalid restoration target role"));
    }
    let mut frozen = world
        .entity_mut(handle.player)
        .take::<Frozen>()
        .expect("validated held target");
    frozen.handle.life.store(RELEASED, Ordering::Release);
    frozen
        .fields
        .take()
        .expect("owned original fields")
        .put(world, handle.player);
    if let Err(error) = restore(world, handle.player, snapshot) {
        // Validation fails before replacement; keep the exact old buffers withheld,
        // including when the original actor has legitimately lost its session.
        frozen.fields = Some(Fields::take(world, handle.player));
        frozen.handle.life.store(HELD, Ordering::Release);
        world.entity_mut(handle.player).insert(frozen);
        return Err(error);
    }
    Ok(true)
}

/// One synchronous physical move can roll back before any packet or ownership publication.
/// No provider IO or guest callback is permitted while this provisional move exists.
pub(crate) struct LiveMove {
    source: Entity,
    target: Handle,
    frozen: Option<Frozen>,
    target_ground: Option<crate::entity::components::OnGround>,
}
impl LiveMove {
    /// Restore the exact live buffers if publication capacity is unavailable.
    pub(crate) fn rollback(mut self, world: &mut World) {
        let actual = Fields::take(world, self.target.player);
        actual.put(world, self.source);
        if let Some(ground) = world
            .entity_mut(self.target.player)
            .take::<crate::entity::components::OnGround>()
        {
            world.entity_mut(self.source).insert(ground);
        }
        if let Some(ground) = self.target_ground.take() {
            world.entity_mut(self.target.player).insert(ground);
        }
        let frozen = self.frozen.take().expect("owned target buffers");
        world.entity_mut(self.target.player).insert(frozen);
    }
    /// Canonical ownership and a reserved initial batch authorize the physical move.
    pub(crate) fn commit(mut self) {
        let mut frozen = self.frozen.take().expect("owned target buffers");
        frozen.handle.life.store(RELEASED, Ordering::Release);
        frozen.fields.take();
    }
}
/// Move current native components, never a historical submitted snapshot. The caller
/// must commit or roll back before releasing its exclusive world borrow.
pub(crate) fn move_live(
    world: &mut World,
    source: Entity,
    target: &Handle,
) -> Result<LiveMove, SnapshotError> {
    if source == target.player
        || !matches(world, target)
        || world
            .get::<Frozen>(target.player)
            .is_none_or(|frozen| frozen.role != Role::Target)
        || super::super::inventory_effects::is_quarantined(world, source)
        || world.get::<Dead>(source).is_some()
        || world.get::<PlayerSession>(source).is_some()
    {
        return Err(invalid("live source changed before physical move"));
    }
    // Complete-source validation occurs while every component still belongs to it.
    capture(world, source)?;
    let frozen = world
        .entity_mut(target.player)
        .take::<Frozen>()
        .expect("validated target buffers");
    let actual = Fields::take(world, source);
    actual.put(world, target.player);
    let target_ground = world
        .entity_mut(target.player)
        .take::<crate::entity::components::OnGround>();
    if let Some(ground) = world
        .entity_mut(source)
        .take::<crate::entity::components::OnGround>()
    {
        world.entity_mut(target.player).insert(ground);
    }
    Ok(LiveMove {
        source,
        target: target.clone(),
        frozen: Some(frozen),
        target_ground,
    })
}
