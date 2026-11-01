//! Positive item delivery keeps the actual living body authoritative during storage I/O.

use super::*;
use crate::item::ItemStack;
use crate::storage::item_grants::{ItemMutation, PersistentItem};

#[derive(Clone, Copy)]
enum Area {
    Main(usize),
    Armour(usize),
    Offhand,
}
#[derive(Clone)]
struct Addition {
    area: Area,
    original: ItemStack,
    item: ItemStack,
}
struct State {
    source: Box<Quarantined>,
    lineage: crate::plugin::manager::players::PlayerLineage,
    epoch: InventoryEpoch,
    additions: Vec<Addition>,
    record: Option<InventoryEffectRecord>,
    phase_key: String,
    step: u8,
    mutations: Vec<ItemMutation>,
    baseline: [u8; 32],
    checkpointed: bool,
}
/// This marker locks only the canonical write lane, never health or simulation components.
#[derive(Component)]
struct Pending(Arc<Mutex<State>>);

#[derive(Component)]
struct DeferredDeath {
    cause: crate::entity::player_life::death::DeathCause,
    attribution: crate::entity::damage::DamageAttribution,
    rules: crate::world::rules::GameRules,
    settlement: Option<Settlement>,
}

enum Settlement {
    Committed {
        lineage: crate::plugin::manager::players::PlayerLineage,
        id: String,
        receipt: InventoryEffectRecord,
    },
    NotApplied {
        lineage: crate::plugin::manager::players::PlayerLineage,
        epoch: InventoryEpoch,
        failure: EffectWriteFailure,
    },
}

pub(super) fn install(world: &mut World) {
    world.add_observer(|event: On<Remove, Pending>, mut commands: Commands| {
        commands.trigger(InventoryBuffersReleased {
            player: event.entity,
        });
    });
}

pub(super) fn pending(world: &World, player: Entity) -> bool {
    world.get::<Pending>(player).is_some()
}

pub(super) fn contains(world: &World, player: Entity, id: &str) -> bool {
    world
        .get::<Pending>(player)
        .is_some_and(|pending| pending.0.lock().is_ok_and(|state| state.source.id == id))
}

fn invalid() -> EffectError {
    EffectError::InvalidInventory("item delivery must contain only positive slot additions".into())
}

fn addition(
    area: Area,
    before: &ItemStack,
    after: &ItemStack,
) -> Result<Option<Addition>, EffectError> {
    if before == after {
        return Ok(None);
    }
    if after.is_empty()
        || (!before.is_empty() && (!before.comparable(after) || after.count <= before.count))
        || after.count > after.max_stack_size()
    {
        return Err(invalid());
    }
    let count = after.count - if before.is_empty() { 0 } else { before.count };
    Ok(Some(Addition {
        area,
        original: before.clone(),
        item: after.clone().with_count(count).with_net_id(0),
    }))
}

pub(super) fn hold(
    world: &mut World,
    player: Entity,
    source: Quarantined,
    baseline: [u8; 32],
) -> Result<(), EffectError> {
    let before = &source.original;
    let after = &source.replacement;
    if before.ui.0.slots() != after.ui.0.slots()
        || before.ender.0.slots() != after.ender.0.slots()
        || before.held.0 != after.held.0
    {
        return Err(invalid());
    }
    let mut additions = Vec::new();
    for (slot, (before, after)) in before
        .main
        .0
        .slots()
        .iter()
        .zip(after.main.0.slots())
        .enumerate()
    {
        additions.extend(addition(Area::Main(slot), before, after)?);
    }
    for (slot, (before, after)) in before
        .armour
        .slots()
        .iter()
        .zip(after.armour.slots())
        .enumerate()
    {
        additions.extend(addition(Area::Armour(slot), before, after)?);
    }
    additions.extend(addition(
        Area::Offhand,
        &before.offhand.0,
        &after.offhand.0,
    )?);
    let phase_key = source.id.clone();
    let lineage = crate::plugin::manager::players::ensure_identity(world, player)
        .map_err(|_| EffectError::MissingPlayer)?;
    let epoch = world.get::<InventoryPersistence>(player).map_or_else(
        || InventoryPersistence::default().epoch(),
        InventoryPersistence::epoch,
    )?;
    world
        .entity_mut(player)
        .insert(Pending(Arc::new(Mutex::new(State {
            source: Box::new(source),
            lineage,
            epoch,
            additions,
            record: None,
            phase_key,
            step: 0,
            mutations: Vec::new(),
            baseline,
            checkpointed: false,
        }))));
    Ok(())
}

/// The catch-up point records native progress without renewing the operation on every tick.
pub(super) fn fingerprint(data: &PlayerData) -> Result<[u8; 32], EffectError> {
    let bytes = serde_json::to_vec(&(
        (
            data.position,
            data.rotation,
            data.dimension,
            data.game_mode,
            data.health,
            data.max_health,
            data.food,
            data.saturation,
            data.exhaustion,
            data.experience,
            data.experience_progress,
            data.cold_exposure,
        ),
        (
            &data.inventory,
            &data.armour,
            &data.offhand,
            &data.ui,
            &data.ender_chest,
            data.held_slot,
        ),
    ))
    .map_err(|_| invalid())?;
    Ok(Sha256::digest(bytes).into())
}

fn current(slots: &Slots, area: Area) -> &ItemStack {
    match area {
        Area::Main(slot) => &slots.main.0.slots()[slot],
        Area::Armour(slot) => &slots.armour.slots()[slot],
        Area::Offhand => &slots.offhand.0,
    }
}
fn put(slots: &mut Slots, area: Area, item: ItemStack) -> Result<(), EffectError> {
    match area {
        Area::Main(slot) => {
            slots.main.0.set_item(slot, item).map_err(|_| invalid())?;
        }
        Area::Armour(slot) => {
            slots.armour.set_slot(slot, item).map_err(|_| invalid())?;
        }
        Area::Offhand => slots.offhand.0 = item.into_slot(),
    }
    Ok(())
}
fn available(before: &ItemStack, current: &ItemStack, gift: &ItemStack) -> u8 {
    if current.is_empty() {
        return gift.count;
    }
    if before.is_empty() || current.net_id != before.net_id || !current.comparable(gift) {
        return 0;
    }
    gift.count
        .min(gift.max_stack_size().saturating_sub(current.count))
}

/// Remove only parts that no longer fit. Every such part needs an acknowledged world row first.
fn reconcile(state: &mut State, slots: &Slots) -> Vec<ItemStack> {
    let mut spill = Vec::new();
    state.additions.retain_mut(|addition| {
        let fits = available(
            &addition.original,
            current(slots, addition.area),
            &addition.item,
        );
        if fits < addition.item.count {
            spill.push(addition.item.clone().with_count(addition.item.count - fits));
            addition.item.count = fits;
        }
        fits != 0
    });
    spill
}

/// Incoming value that would leave an already-dead recipient must keep durable world ownership.
fn dying_spill(world: &World, player: Entity, state: &mut State) -> Vec<ItemStack> {
    let Some(death) = world.get::<DeferredDeath>(player) else {
        return Vec::new();
    };
    if death.rules.keep_inventory {
        return Vec::new();
    }
    let mut spill = Vec::new();
    state.additions.retain(|addition| {
        if crate::entity::player_life::death::keeps_on_death(&addition.item) {
            return true;
        }
        spill.push(addition.item.clone());
        false
    });
    spill
}

fn apply_additions(slots: &mut Slots, additions: &[Addition]) -> Result<(), EffectError> {
    for addition in additions {
        let current = current(slots, addition.area);
        if available(&addition.original, current, &addition.item) != addition.item.count {
            return Err(EffectError::Conflict);
        }
        let item = if current.is_empty() {
            addition.item.clone()
        } else {
            current
                .clone()
                .with_count(current.count + addition.item.count)
        };
        put(slots, addition.area, item)?;
    }
    Ok(())
}

fn same_source(world: &World, player: Entity, source: &Quarantined) -> bool {
    world.get::<Player>(player).is_some()
        && world.get::<Position>(player).is_some()
        && world.get::<Rotation>(player).is_some()
        && world.get::<Health>(player).is_some()
        && world.get::<PlayerUuid>(player).map(|id| id.0) == Some(source.uuid)
        && world.get::<RuntimeEntityId>(player).map(|id| id.0) == Some(source.runtime)
        && world
            .get::<InventoryPersistence>(player)
            .map_or(0, |state| state.revision)
            == source.expected.revision
        && match world.get::<PlayerSession>(player) {
            Some(session) => {
                session.session_id == source.session
                    && session.xuid == source.xuid
                    && session.uuid == source.protocol_uuid
                    && session.outbound_tx.same_channel(&source.outbound)
            }
            None => {
                world
                    .get::<super::super::player_events::DetachedPlayer>(player)
                    .is_some()
                    && super::super::player_state::account_xuid(world, player)
                        == source.xuid.as_deref()
            }
        }
}

fn record(world: &World, player: Entity) -> Result<PlayerData, EffectError> {
    let (_, mut data) = if world
        .get::<super::super::player_state::source_actors::ManagedSource>(player)
        .is_some()
    {
        super::super::player_state::source_actors::active_record(world, player)
    } else {
        super::super::capture_player_record(world, player, false)
    }
    .ok_or(EffectError::MissingPlayer)?;
    data.game_mode = super::super::game_mode_to_storage_id(
        crate::command::exec::base_game_mode(world, player).ok_or(EffectError::MissingPlayer)?,
    );
    Ok(data)
}

fn items(
    world: &World,
    player: Entity,
    state: &State,
    spill: Vec<ItemStack>,
) -> Result<Vec<PersistentItem>, EffectError> {
    let identity = world
        .get_resource::<crate::world::WorldIdentity>()
        .ok_or(EffectError::UnsupportedProvider)?;
    let feet = world
        .get::<Position>(player)
        .ok_or(EffectError::MissingPlayer)?
        .0
        - glam::DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
    Ok(spill
        .into_iter()
        .enumerate()
        .map(|(index, stack)| {
            let mut hash = Sha256::new();
            hash.update(b"unastar.grant-spill.v1\0");
            hash.update(identity.id.as_bytes());
            hash.update(state.source.uuid.as_bytes());
            hash.update((state.source.id.len() as u64).to_be_bytes());
            hash.update(state.source.id.as_bytes());
            hash.update([state.step]);
            hash.update((index as u32).to_be_bytes());
            let bytes: [u8; 32] = hash.finalize().into();
            PersistentItem {
                id: Uuid::from_bytes(bytes[..16].try_into().expect("fixed digest")),
                world: identity.id,
                dimension: identity.dimension,
                feet: feet.to_array(),
                stack: StoredItemStack::from(&stack),
            }
        })
        .collect())
}

fn put_into(data: &mut PlayerData, slots: &Slots) {
    data.inventory = slots.main.0.slots().iter().map(Into::into).collect();
    data.armour = std::array::from_fn(|slot| (&slots.armour.slots()[slot]).into());
    data.offhand = (&slots.offhand.0).into();
}

fn acknowledge(
    world: &mut World,
    player: Entity,
    state: &mut State,
    record: &InventoryEffectRecord,
) {
    let mut persistence = world
        .get::<InventoryPersistence>(player)
        .cloned()
        .unwrap_or_default();
    persistence.revision = record.revision;
    persistence
        .receipts
        .insert(state.phase_key.clone(), record.clone());
    world.entity_mut(player).insert(persistence);
    state.source.expected.revision = record.revision;
}

fn queue(
    world: &mut World,
    player: Entity,
    state: &mut State,
    data: PlayerData,
    mutation: ItemMutation,
    retention: Option<Arc<dyn Send + Sync>>,
) -> Result<(), EffectError> {
    state.step = state.step.checked_add(1).ok_or(EffectError::Conflict)?;
    state.phase_key = format!("{}:delivery:{}", state.source.id, state.step);
    let bytes = serde_json::to_vec(&data).map_err(|_| invalid())?;
    let hash: [u8; 32] = Sha256::digest(bytes).into();
    let key = state.phase_key.clone();
    let coordinator = world.resource::<Coordinator>();
    let writer = coordinator.writer.clone();
    let guard = ReplyGuard {
        tx: coordinator.tx.clone(),
        writer: writer.clone(),
        player,
        uuid: state.source.uuid,
        id: state.source.id.clone(),
        finished: false,
        items: Some(mutation.clone()),
        retention,
    };
    let uuid = state.source.uuid;
    tokio::runtime::Handle::try_current()
        .map_err(|_| EffectError::MissingRuntime)?
        .spawn(async move {
            guard.finish(
                writer
                    .commit_item_effect(uuid, &key, hash, data, mutation)
                    .await,
            );
        });
    Ok(())
}

fn release(world: &mut World, player: Entity, uuid: Uuid) {
    world.resource_mut::<Coordinator>().pending.remove(&uuid);
    if let Ok(mut actor) = world.get_entity_mut(player) {
        actor.remove::<Pending>();
    }
}

fn committed(
    world: &mut World,
    completion: Completion,
    record: InventoryEffectRecord,
    already_committed: bool,
    live_applied: bool,
) {
    world.trigger(InventoryEffectCommitted {
        player: world
            .get_entity(completion.player)
            .is_ok()
            .then_some(completion.player),
        uuid: completion.uuid,
        id: completion.id,
        record,
        already_committed,
        live_applied,
        retained_buffers: pending(world, completion.player),
    });
}

fn fail(world: &mut World, completion: Completion, error: EffectWriteFailure, resume: bool) {
    if resume {
        let shared = world
            .get::<Pending>(completion.player)
            .map(|pending| pending.0.clone());
        if let Some(shared) = shared
            && let Ok(state) = shared.lock()
            && let Some(mut death) = world.get_mut::<DeferredDeath>(completion.player)
        {
            death.settlement = Some(Settlement::NotApplied {
                lineage: state.lineage.clone(),
                epoch: state.epoch,
                failure: error.clone(),
            });
        }
        release(world, completion.player, completion.uuid);
    } else {
        world
            .resource::<Coordinator>()
            .writer
            .require_recovery(completion.uuid);
        kick(
            world,
            completion.player,
            "Inventory persistence requires recovery before continuing.",
        );
    }
    world.trigger(InventoryEffectFailed {
        player: world
            .get_entity(completion.player)
            .is_ok()
            .then_some(completion.player),
        uuid: completion.uuid,
        id: completion.id,
        kind: error.kind,
        message: error.message,
        retained_buffers: pending(world, completion.player),
    });
    if resume {
        resume_death(world, completion.player);
    }
}

pub(super) fn complete(world: &mut World, mut completion: Completion) {
    let shared = world
        .get::<Pending>(completion.player)
        .expect("pending source checked")
        .0
        .clone();
    let Ok(mut state) = shared.lock() else {
        return;
    };
    let valid = state.lineage.matches(world, completion.player)
        && same_source(world, completion.player, &state.source);
    let result = std::mem::replace(
        &mut completion.result,
        Err(EffectWriteFailure {
            kind: WriteFailureKind::RecoveryRequired,
            resume_source: false,
            message: "delivery completion unavailable".into(),
        }),
    );
    let write = match result {
        Ok(write) if valid && !write.already_committed => write,
        Ok(write) => {
            let record = state.record.clone().unwrap_or(write.record);
            let mut mutations = std::mem::take(&mut state.mutations);
            if !write.already_committed
                && let Some(items) = completion.items.take()
            {
                mutations.push(items);
            }
            drop(state);
            for mutation in mutations {
                super::super::item_grants::publish(world, &mutation, None);
            }
            world
                .resource::<Coordinator>()
                .writer
                .require_recovery(completion.uuid);
            kick(
                world,
                completion.player,
                "Inventory state changed; reconnect after its durable result is recovered.",
            );
            committed(world, completion, record, write.already_committed, false);
            return;
        }
        Err(mut error) => {
            if state.record.is_some() {
                // A later correction can fail without undoing the already acknowledged mint.
                error.kind = WriteFailureKind::RecoveryRequired;
                error.resume_source = false;
            }
            let resume = valid
                && state.record.is_none()
                && error.kind == WriteFailureKind::NotApplied
                && error.resume_source;
            drop(state);
            fail(world, completion, error, resume);
            return;
        }
    };
    acknowledge(world, completion.player, &mut state, &write.record);
    if state.record.is_none() {
        state.record = Some(write.record);
    }
    if let Some(items) = completion.items.take() {
        state.mutations.push(items);
    }
    let attempt = (|| -> Result<bool, EffectError> {
        let mut slots = clone_slots(world, completion.player)?;
        let mut spill = dying_spill(world, completion.player, &mut state);
        spill.extend(reconcile(&mut state, &slots));
        let mut data = record(world, completion.player)?;
        let catch_up = !state.checkpointed && fingerprint(&data)? != state.baseline;
        if !spill.is_empty() || catch_up {
            let mutation = ItemMutation::Create(items(world, completion.player, &state, spill)?);
            apply_additions(&mut slots, &state.additions)?;
            put_into(&mut data, &slots);
            state.checkpointed = true;
            queue(
                world,
                completion.player,
                &mut state,
                data,
                mutation,
                completion._retention.clone(),
            )?;
            return Ok(false);
        }
        apply_additions(&mut slots, &state.additions)?;
        world
            .entity_mut(completion.player)
            .insert((slots.main, slots.armour, slots.offhand));
        Ok(true)
    })();
    match attempt {
        Ok(false) => (),
        Ok(true) => {
            let record = state
                .record
                .clone()
                .expect("initial durable record acknowledged");
            let mutations = std::mem::take(&mut state.mutations);
            if let Some(mut death) = world.get_mut::<DeferredDeath>(completion.player) {
                death.settlement = Some(Settlement::Committed {
                    lineage: state.lineage.clone(),
                    id: state.source.id.clone(),
                    receipt: record.clone(),
                });
            }
            drop(state);
            for mutation in mutations {
                super::super::item_grants::publish(world, &mutation, Some(completion.player));
            }
            super::super::send_inventory_contents(world, completion.player);
            release(world, completion.player, completion.uuid);
            let player = completion.player;
            committed(world, completion, record, false, true);
            resume_death(world, player);
        }
        Err(error) => {
            drop(state);
            fail(
                world,
                completion,
                EffectWriteFailure {
                    kind: WriteFailureKind::RecoveryRequired,
                    resume_source: false,
                    message: error.to_string(),
                },
                false,
            );
        }
    }
}

pub(super) fn defer_death(
    world: &mut World,
    player: Entity,
    cause: crate::entity::player_life::death::DeathCause,
    attribution: crate::entity::damage::DamageAttribution,
) -> bool {
    if !pending(world, player) {
        return false;
    }
    let rules = world.resource::<crate::world::rules::GameRules>().clone();
    world.entity_mut(player).insert(DeferredDeath {
        cause,
        attribution,
        rules,
        settlement: None,
    });
    true
}

fn resume_death(world: &mut World, player: Entity) {
    let Some(death) = world.entity_mut(player).take::<DeferredDeath>() else {
        return;
    };
    let accepted = match &death.settlement {
        Some(Settlement::Committed {
            lineage,
            id,
            receipt,
        }) => super::super::player_state::source_actors::defer_death_after_inventory(
            world,
            player,
            death.cause,
            death.attribution,
            death.rules.clone(),
            lineage,
            id,
            receipt,
        ),
        Some(Settlement::NotApplied {
            lineage,
            epoch,
            failure,
        }) => super::super::player_state::source_actors::defer_death_after_inventory_rejection(
            world,
            player,
            death.cause,
            death.attribution,
            death.rules.clone(),
            lineage,
            *epoch,
            failure,
        ),
        None => super::super::player_state::source_actors::defer_death_with_rules(
            world,
            player,
            death.cause,
            death.attribution,
            death.rules.clone(),
        ),
    };
    if accepted {
        return;
    }
    // The ordinary terminal writer is a separate phase; keep unacknowledged death fenced.
    world.entity_mut(player).insert(death);
}

pub(super) fn death_pending(world: &World, player: Entity) -> bool {
    world.get::<DeferredDeath>(player).is_some()
}
