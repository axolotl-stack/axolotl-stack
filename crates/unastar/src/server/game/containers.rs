//! Container blocks in the world (I8/I9): chests (single and double),
//! trapped chests, barrels, ender chests, furnaces, blast furnaces, smokers
//! and hoppers.
//!
//! The chunk's block actor NBT stays the source of truth for persistence and
//! chunk sends: every committed change re-encodes the actor
//! ([`BlockContainer::encode`]) into the chunk and marks it dirty.
//! [`BlockContainers`] caches decoded actors of loaded chunks so ticks and
//! windows do not decode NBT, and holds the runtime-only state BDS keeps
//! outside the save: who views which container and the furnace values each
//! viewer was last sent.
//!
//! BDS 1.26.51 rules implemented here (research notes in
//! `thoughts/shared/research/bds-quirks.md` and the items research doc):
//! - Opening allocates the next window id and sends `ContainerOpen
//!   {id, type, clicked position, actor -1}` followed by `InventoryContent`
//!   for that id (container name byte 0). Double chests show 54 slots, the
//!   pair lead's 27 first, whichever half was clicked.
//! - The first viewer opens the lid (`BlockEvent {lead, 1, 1}` plus the open
//!   sound), the last one closes it (`{lead, 1, 0}`); the barrel flips its
//!   `open_bit` instead. While open the event is repeated every 80 ticks.
//! - A placed chest pairs with an unpaired neighbour of the same chest type
//!   and cardinal direction along the axis perpendicular to its facing; the
//!   existing chest becomes the lead. Breaking either half unpairs both and
//!   closes their viewers with a server-initiated close.
//! - Breaking drops chest, barrel and hopper contents in random 10..30 item
//!   chunks; a furnace drops whole stacks.
//! - Furnaces tick once per game tick: refuel when burn time
//!   runs out and the input can smelt, cook interval 200 (furnace) or 100
//!   (blast furnace, smoker), progress decays by 2 while not burning, the
//!   lit/unlit block swaps in the tick burning starts or stops, and viewers get
//!   `ContainerSetData` ids 0 cook, 1 burn time, 2 burn duration, 3 stored XP
//!   whenever a value changed.
//! - Taking from a furnace's result slot pays `count x XP multiplier` plus the
//!   stored XP, rounded up with probability equal to the fraction.
//!
//! Not modelled: loot-table unpacking of generated chests (the `LootTable`
//! keys are preserved), chest obstruction, trapped-chest redstone, copper
//! chests, hopper redstone power other than an existing `toggle_bit`, and XP
//! orbs (smelting XP goes straight to the taking player).

use std::collections::{BTreeSet, HashMap};

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::{
    ActorUniqueId, BlockActorDataPacket, BlockEventPacket, BlockPos, ContainerClosePacket,
    ContainerOpenPacket, ContainerSetDataPacket, InventoryContentPacket, LevelSoundEventPacket,
    McpePacket, Vec3,
};
use rand::Rng;
use tracing::warn;

use crate::block_entity::{BlockContainer, ContainerKind, Pair, burn_ticks};
use crate::entity::components::{
    BlockScreen, ContainerWindows, DroppedItem, EnderChestInventory, Experience, Inventory,
    ItemEntityStack, PlayerSession, Position, RuntimeEntityId, ScreenKind,
};
use crate::entity::item_drop::EntityRandom;
use crate::item::ItemStack;
use crate::registry::block::StateValue;
use crate::world::block_entity::BlockEntity;
use crate::world::ecs::{
    ChunkData, ChunkManager, ChunkStateFlags, ChunkViewers, world_to_chunk_coords,
    world_to_local_coords,
};

use super::types::{BlockRegistryResource, ItemRegistryResource, RecipeRegistryResource};

/// Ticks between hopper transfers.
const HOPPER_COOLDOWN: i32 = 8;
/// BDS repeats the open-lid block event this often while viewed.
const LID_KEEPALIVE_TICKS: u64 = 80;
/// `BlockEvent` type that sets a chest's lid state.
const CHEST_LID_EVENT: i32 = 1;

/// Decoded container actors of loaded chunks and their viewers.
#[derive(Resource, Default)]
pub struct BlockContainers {
    live: HashMap<IVec3, BlockContainer>,
    /// Viewers per lid position (a double chest's lead).
    viewers: HashMap<IVec3, Vec<Viewer>>,
    /// Furnaces and hoppers to tick, in a stable order.
    ticking: BTreeSet<[i32; 3]>,
    tick: u64,
}

#[derive(Debug, Clone, Copy)]
struct Viewer {
    player: Entity,
    window: u8,
    /// Furnace values last sent (`ContainerSetData` ids 0..3).
    sent: [i32; 4],
}

// ── World access ────────────────────────────────────────────────────────────

fn chunk_entity(world: &World, pos: IVec3) -> Option<Entity> {
    let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
    world.get_resource::<ChunkManager>()?.get_by_coords(cx, cz)
}

/// The container kind of the block at `pos` and its runtime id.
pub(super) fn kind_at(world: &World, pos: IVec3) -> Option<(ContainerKind, u32)> {
    let runtime_id = super::blocks::block_at(world, (pos.x, pos.y, pos.z))?;
    let entry = world
        .get_resource::<BlockRegistryResource>()?
        .0
        .get_by_runtime_id(runtime_id)?;
    Some((ContainerKind::from_block(&entry.string_id)?, runtime_id))
}

fn state_value(world: &World, pos: IVec3, property: &str) -> Option<StateValue> {
    let runtime_id = super::blocks::block_at(world, (pos.x, pos.y, pos.z))?;
    world
        .get_resource::<BlockRegistryResource>()?
        .0
        .get_by_runtime_id(runtime_id)?
        .state_value(runtime_id, property)
}

/// Decode (or create) the actor at `pos` into the cache. `None` when the
/// block is not a container, its chunk is not loaded, or its saved actor
/// cannot be read (it is then left untouched rather than overwritten).
pub(super) fn load(world: &mut World, pos: IVec3) -> Option<ContainerKind> {
    let (kind, _) = kind_at(world, pos)?;
    if world
        .resource::<BlockContainers>()
        .live
        .get(&pos)
        .is_some_and(|container| container.kind == kind)
    {
        return Some(kind);
    }
    let chunk = chunk_entity(world, pos)?;
    let actor = world
        .get::<ChunkData>(chunk)?
        .inner
        .block_entities()
        .iter()
        .find(|actor| actor.position() == pos.to_array())
        .cloned();
    let container = match actor.as_ref().map(BlockEntity::bedrock_compound) {
        None => BlockContainer::new(kind, pos),
        Some(None) => return None,
        Some(Some(Err(error))) => {
            warn!(?pos, %error, "unreadable container block actor");
            return None;
        }
        Some(Some(Ok(compound))) => {
            let items = &world.resource::<ItemRegistryResource>().0;
            let blocks = &world.resource::<BlockRegistryResource>().0;
            BlockContainer::decode(kind, pos, compound, items, blocks)
        }
    };
    let mut containers = world.resource_mut::<BlockContainers>();
    if kind.smelter().is_some() || kind == ContainerKind::Hopper {
        containers.ticking.insert(pos.to_array());
    }
    containers.live.insert(pos, container);
    Some(kind)
}

/// Write the cached actor at `pos` back into its chunk.
fn save(world: &mut World, pos: IVec3) {
    let Some(compound) = world
        .resource::<BlockContainers>()
        .live
        .get(&pos)
        .map(BlockContainer::encode)
    else {
        return;
    };
    let actor = match BlockEntity::from_bedrock_compound(compound) {
        Ok(actor) => actor,
        Err(error) => {
            warn!(?pos, %error, "container actor failed to encode");
            return;
        }
    };
    let Some(chunk) = chunk_entity(world, pos) else {
        return;
    };
    if let Some(mut data) = world.get_mut::<ChunkData>(chunk) {
        data.inner.set_block_entity(actor);
    }
    if let Some(mut flags) = world.get_mut::<ChunkStateFlags>(chunk) {
        flags.mark_dirty();
    }
}

fn container(world: &World, pos: IVec3) -> Option<&BlockContainer> {
    world.resource::<BlockContainers>().live.get(&pos)
}

fn container_mut(world: &mut World, pos: IVec3) -> Option<Mut<'_, BlockContainer>> {
    let containers = world.resource_mut::<BlockContainers>();
    if !containers.live.contains_key(&pos) {
        return None;
    }
    Some(containers.map_unchanged(|c| c.live.get_mut(&pos).unwrap()))
}

fn pair_pos(container: &BlockContainer) -> Option<IVec3> {
    container
        .pair
        .map(|pair| IVec3::new(pair.x, container.position.y, pair.z))
}

/// A double chest's halves as (lead, other), after dropping a pair whose
/// partner no longer pairs back.
fn halves(world: &mut World, pos: IVec3) -> Option<(IVec3, Option<IVec3>)> {
    let kind = load(world, pos)?;
    let Some(other) = container(world, pos).and_then(pair_pos) else {
        return Some((pos, None));
    };
    let valid = kind.pairs()
        && load(world, other) == Some(kind)
        && container(world, other).and_then(pair_pos) == Some(pos);
    if !valid {
        container_mut(world, pos)?.pair = None;
        save(world, pos);
        send_client_state(world, pos);
        return Some((pos, None));
    }
    if container(world, pos)?.pair?.lead {
        Some((pos, Some(other)))
    } else {
        Some((other, Some(pos)))
    }
}

/// Where the lid animates and viewers are tracked.
fn lid(world: &mut World, pos: IVec3) -> Option<IVec3> {
    halves(world, pos).map(|(lead, _)| lead)
}

/// The slots a window on `pos` shows, `None` if the block cannot hold them.
fn view(world: &mut World, pos: IVec3) -> Option<(ContainerKind, Inventory)> {
    let (lead, other) = halves(world, pos)?;
    unpack_loot(world, lead);
    if let Some(other) = other {
        unpack_loot(world, other);
    }
    let first = container(world, lead)?;
    let kind = first.kind;
    let mut slots = first.items.slots().to_vec();
    if let Some(other) = other {
        slots.extend_from_slice(container(world, other)?.items.slots());
    }
    Some((kind, Inventory::from_slots(slots)))
}

/// Store a view back into the halves it came from and save them.
fn store_view(world: &mut World, pos: IVec3, slots: &Inventory) {
    let Some((lead, other)) = halves(world, pos) else {
        return;
    };
    let size = container(world, lead).map_or(0, |c| c.items.size());
    for (half, range) in [(Some(lead), 0..size), (other, size..slots.size())] {
        let Some(half) = half else {
            continue;
        };
        if let Some(mut container) = container_mut(world, half) {
            for (index, slot) in range.enumerate() {
                if let Some(stack) = slots.item(slot) {
                    let _ = container.items.set_item(index, stack.clone());
                }
            }
        }
        save(world, half);
    }
}

/// A generated container's loot table is unpacked the first time it is
/// opened, broken or reached by a hopper: items are rolled from the table
/// with the saved seed and spread over random empty slots, splitting stacks
/// while slots remain (the Java `LootTable.fill` algorithm; BDS's random
/// sequence is not reproduced, so the same seed gives different contents).
fn unpack_loot(world: &mut World, pos: IVec3) {
    let Some((path, seed)) = container_mut(world, pos).and_then(|mut c| c.take_loot_table()) else {
        return;
    };
    let name = path.strip_prefix("loot_tables/").unwrap_or(&path);
    let name = name.strip_suffix(".json").unwrap_or(name);
    let mut rng = <rand::rngs::StdRng as rand::SeedableRng>::seed_from_u64(seed as u64);
    let stacks = match unastar_data::loot::get(name) {
        Some(table) => match unastar_rules::loot::roll(
            unastar_data::loot::vanilla(),
            table,
            &super::loot::LootContext::default(),
            &mut rng,
        ) {
            Ok(drops) => super::block_drops::loot_stacks(world, drops),
            Err(error) => {
                warn!(table = name, %error, "container loot table cannot be evaluated");
                Vec::new()
            }
        },
        None => {
            warn!(table = name, ?pos, "container names an unknown loot table");
            Vec::new()
        }
    };
    if let Some(mut container) = container_mut(world, pos) {
        fill(&mut container.items, stacks, &mut rng);
    }
    save(world, pos);
}

/// Spread `items` over random empty slots of `slots`, splitting stacks in
/// two at random while more empty slots remain than items.
fn fill(slots: &mut Inventory, items: Vec<ItemStack>, rng: &mut impl Rng) {
    use rand::seq::SliceRandom;
    let mut free: Vec<usize> = (0..slots.size())
        .filter(|&slot| slots.item(slot).is_some_and(ItemStack::is_empty))
        .collect();
    free.shuffle(rng);
    let (mut splittable, mut placed): (Vec<_>, Vec<_>) = items
        .into_iter()
        .filter(|stack| !stack.is_empty())
        .partition(|stack| stack.count > 1);
    while free.len() > placed.len() + splittable.len() && !splittable.is_empty() {
        let mut stack = splittable.swap_remove(rng.gen_range(0..splittable.len()));
        let take = rng.gen_range(1..=stack.count / 2);
        let (part, rest) = stack.split(take);
        stack = rest;
        for piece in [stack, part] {
            if piece.count > 1 && rng.r#gen::<bool>() {
                splittable.push(piece);
            } else {
                placed.push(piece);
            }
        }
    }
    placed.extend(splittable);
    placed.shuffle(rng);
    for stack in placed {
        let Some(slot) = free.pop() else {
            warn!("loot does not fit the container; the rest is discarded (Java fill)");
            break;
        };
        let _ = slots.set_item(slot, stack.with_net_id(0).into_slot());
    }
}

// ── Packets ─────────────────────────────────────────────────────────────────

fn block_pos(pos: IVec3) -> BlockPos {
    BlockPos {
        x: pos.x,
        y: pos.y,
        z: pos.z,
    }
}

/// Send to every player that has the chunk of `pos` in view.
fn broadcast(world: &World, pos: IVec3, packet: McpePacket) {
    let Some(chunk) = chunk_entity(world, pos) else {
        return;
    };
    let Some(viewers) = world.get::<ChunkViewers>(chunk) else {
        return;
    };
    for viewer in viewers.iter() {
        if let Some(session) = world.get::<PlayerSession>(viewer) {
            let _ = session.send(packet.clone());
        }
    }
}

fn send(world: &World, player: Entity, packet: impl Into<McpePacket>) {
    if let Some(session) = world.get::<PlayerSession>(player) {
        let _ = session.send(packet.into());
    }
}

fn window_content(
    window: u8,
    slots: &Inventory,
    block_ids: crate::registry::block::BlockNetworkIds,
) -> Option<InventoryContentPacket> {
    Some(InventoryContentPacket {
        container_id: window.into(),
        slots: slots
            .slots()
            .iter()
            .map(|stack| super::inventory::wire_item(stack, block_ids))
            .collect::<Option<Vec<_>>>()?,
        full_container_name: super::inventory::UNNAMED_CONTAINER,
        storage_item: Default::default(),
    })
}

/// `BlockActorData` with the client-side state BDS sends after pair changes.
fn send_client_state(world: &World, pos: IVec3) {
    let Some(container) = container(world, pos) else {
        return;
    };
    let actor = match BlockEntity::from_bedrock_compound(container.client_state()) {
        Ok(actor) => actor,
        Err(_) => return,
    };
    broadcast(
        world,
        pos,
        McpePacket::from(BlockActorDataPacket {
            block_position: block_pos(pos),
            actor_data_tags: actor.network_nbt().clone(),
        }),
    );
}

fn play_sound(world: &World, at: DVec3, name: &str, block: u32) {
    let pos = at.floor().as_ivec3();
    broadcast(
        world,
        pos,
        McpePacket::from(LevelSoundEventPacket {
            sound_event: name.into(),
            position: Vec3 {
                x: at.x as f32,
                y: at.y as f32,
                z: at.z as f32,
            },
            data: block as i32,
            actor_identifier: "".into(),
            is_baby: false,
            is_global: false,
            actor_unique_id: -1,
            fire_at_position: None,
        }),
    );
}

/// Lid open/close for the first/last viewer.
fn set_lid(world: &mut World, kind: ContainerKind, lid: IVec3, open: bool) {
    let Some((_, runtime_id)) = kind_at(world, lid) else {
        return;
    };
    if kind.has_lid() {
        broadcast(
            world,
            lid,
            McpePacket::from(BlockEventPacket {
                block_position: block_pos(lid),
                event_type: CHEST_LID_EVENT,
                event_value: i32::from(open),
            }),
        );
    } else if kind == ContainerKind::Barrel {
        let state = world
            .resource::<BlockRegistryResource>()
            .0
            .get_by_runtime_id(runtime_id)
            .and_then(|entry| {
                entry.with_state_value(runtime_id, "open_bit", StateValue::Byte(u8::from(open)))
            });
        if let Some(state) = state.filter(|state| *state != runtime_id) {
            super::blocks::set_block(world, lid, state, false);
            save(world, lid);
        }
    }
    let Some((open_sound, close_sound)) = kind.viewer_sounds() else {
        return;
    };
    let mut centre = lid.as_dvec3() + DVec3::splat(0.5);
    if let Some(other) = container(world, lid).and_then(pair_pos) {
        centre = (lid.as_dvec3() + other.as_dvec3()) * 0.5 + DVec3::splat(0.5);
    }
    play_sound(
        world,
        centre,
        if open { open_sound } else { close_sound },
        runtime_id,
    );
}

// ── Windows ─────────────────────────────────────────────────────────────────

/// Open the container block at `clicked` for `player`. Returns whether the
/// click was a container use (consumed even when nothing could open).
pub(super) fn open(world: &mut World, player: Entity, clicked: IVec3) -> bool {
    let Some((kind, _)) = kind_at(world, clicked) else {
        return false;
    };
    if world
        .get::<ContainerWindows>(player)
        .is_none_or(|windows| windows.open_id().is_some())
    {
        return true;
    }
    let slots = if kind == ContainerKind::EnderChest {
        if load(world, clicked).is_none() {
            return true;
        }
        match world.get::<EnderChestInventory>(player) {
            Some(ender) => ender.0.clone(),
            None => return true,
        }
    } else {
        match view(world, clicked) {
            Some((_, slots)) => slots,
            None => return true,
        }
    };
    let screen = BlockScreen {
        kind,
        at: clicked,
        size: slots.size() as u8,
    };
    let window = world
        .get_mut::<ContainerWindows>(player)
        .expect("checked above")
        .open(ScreenKind::Block(screen));
    send(
        world,
        player,
        ContainerOpenPacket {
            container_id: window,
            container_type: kind.wire_type() as u8,
            position: block_pos(clicked),
            target_actor_id: ActorUniqueId {
                actor_unique_id: -1,
            },
        },
    );
    if let Some(content) = window_content(window, &slots, super::types::block_network_ids(world)) {
        send(world, player, content);
    }
    let Some(lid) = lid(world, clicked) else {
        return true;
    };
    let mut viewer = Viewer {
        player,
        window,
        sent: [0; 4],
    };
    if kind.smelter().is_some()
        && let Some(container) = container(world, clicked)
    {
        let values = furnace_values(container);
        for (id, value) in values.into_iter().enumerate() {
            if value != 0 {
                send(world, player, set_data(window, id, value));
            }
        }
        viewer.sent = values;
    }
    let first = {
        let mut containers = world.resource_mut::<BlockContainers>();
        let viewers = containers.viewers.entry(lid).or_default();
        viewers.retain(|v| v.player != player);
        viewers.push(viewer);
        viewers.len() == 1
    };
    if first && kind.viewer_sounds().is_some() {
        set_lid(world, kind, lid, true);
    }
    true
}

/// The player closed (or lost) a block window.
pub(super) fn stop_viewing(world: &mut World, player: Entity, screen: BlockScreen) {
    let Some(lid) = lid(world, screen.at) else {
        return;
    };
    let last = {
        let mut containers = world.resource_mut::<BlockContainers>();
        let Some(viewers) = containers.viewers.get_mut(&lid) else {
            return;
        };
        let before = viewers.len();
        viewers.retain(|viewer| viewer.player != player);
        let last = before > 0 && viewers.is_empty();
        if viewers.is_empty() {
            containers.viewers.remove(&lid);
        }
        last
    };
    if last && screen.kind.viewer_sounds().is_some() {
        set_lid(world, screen.kind, lid, false);
    }
}

/// Slots of the open block window for an ItemStackRequest, `None` when the
/// window no longer matches its block (a chest paired or unpaired meanwhile).
pub(super) fn window_slots(
    world: &mut World,
    player: Entity,
    screen: BlockScreen,
) -> Option<Inventory> {
    let slots = if screen.kind == ContainerKind::EnderChest {
        world.get::<EnderChestInventory>(player)?.0.clone()
    } else {
        let (kind, slots) = view(world, screen.at)?;
        (kind == screen.kind).then_some(slots)?
    };
    (slots.size() == usize::from(screen.size)).then_some(slots)
}

fn same_stack(a: &ItemStack, b: &ItemStack) -> bool {
    a.is_empty() && b.is_empty()
        || (a.item_id() == b.item_id()
            && a.count == b.count
            && a.damage == b.damage
            && a.nbt == b.nbt
            && a.net_id == b.net_id)
}

/// Commit what a request did to an open block window.
pub(super) fn commit_window(
    world: &mut World,
    player: Entity,
    screen: BlockScreen,
    before: &Inventory,
    after: Inventory,
) {
    if before
        .slots()
        .iter()
        .zip(after.slots())
        .all(|(a, b)| same_stack(a, b))
    {
        return;
    }
    if screen.kind == ContainerKind::EnderChest {
        if let Some(mut ender) = world.get_mut::<EnderChestInventory>(player) {
            ender.0 = after;
        }
        return;
    }
    if screen.kind.smelter().is_some() {
        let (input_before, input_after) = (before.item(0).unwrap(), after.item(0).unwrap());
        // A different (or emptied/filled) input restarts cooking.
        let input_changed = !(input_after.comparable(input_before)
            || input_after.is_empty() && input_before.is_empty());
        let taken = taken_output(before.item(2).unwrap(), after.item(2).unwrap());
        if input_changed && let Some(mut container) = container_mut(world, screen.at) {
            container.furnace.cook_time = 0;
        }
        if let Some(taken) = taken {
            pay_smelting_xp(world, player, screen.at, &taken);
        }
    }
    store_view(world, screen.at, &after);
    refresh_viewers(world, screen.at, Some(player));
}

/// The stack a request removed from a furnace's result slot, if any.
fn taken_output(before: &ItemStack, after: &ItemStack) -> Option<ItemStack> {
    if before.is_empty() {
        return None;
    }
    let remaining = if after.is_empty() {
        0
    } else if after.item_id() == before.item_id() && after.damage == before.damage {
        after.count
    } else {
        0
    };
    (remaining < before.count).then(|| before.clone().with_count(before.count - remaining))
}

/// Send each viewer (except `skip`) the window's current content.
fn refresh_viewers(world: &mut World, pos: IVec3, skip: Option<Entity>) {
    let Some(lid) = lid(world, pos) else {
        return;
    };
    let viewers: Vec<Viewer> = world
        .resource::<BlockContainers>()
        .viewers
        .get(&lid)
        .cloned()
        .unwrap_or_default();
    if viewers.is_empty() {
        return;
    }
    let Some((_, slots)) = view(world, lid) else {
        return;
    };
    for viewer in viewers {
        if Some(viewer.player) == skip {
            continue;
        }
        if let Some(content) = window_content(
            viewer.window,
            &slots,
            super::types::block_network_ids(world),
        ) {
            send(world, viewer.player, content);
        }
    }
}

/// Close every window on the container at `pos` from the server side.
fn close_viewers(world: &mut World, pos: IVec3, kind: ContainerKind) {
    let Some(lid) = lid(world, pos) else {
        return;
    };
    let viewers = world
        .resource_mut::<BlockContainers>()
        .viewers
        .remove(&lid)
        .unwrap_or_default();
    for viewer in viewers {
        let Some(mut windows) = world.get_mut::<ContainerWindows>(viewer.player) else {
            continue;
        };
        if windows.open_id() != Some(viewer.window) {
            continue;
        }
        windows.close();
        send(
            world,
            viewer.player,
            ContainerClosePacket {
                container_id: viewer.window,
                container_type: kind.wire_type() as u8,
                server_initiated_close: true,
            },
        );
    }
}

// ── Furnace XP ──────────────────────────────────────────────────────────────

/// XP per smelted item: fixed values for terracotta (0.35), sponge (0.15) and
/// the stone, glass, sandstone, quartz and deepslate results below (0.1),
/// else the result item's furnace XP multiplier.
fn xp_multiplier(stack: &ItemStack) -> f32 {
    const TENTH: &[&str] = &[
        "minecraft:stone",
        "minecraft:stone_bricks",
        "minecraft:mossy_stone_bricks",
        "minecraft:cracked_stone_bricks",
        "minecraft:chiseled_stone_bricks",
        "minecraft:glass",
        "minecraft:smooth_stone",
        "minecraft:sandstone",
        "minecraft:chiseled_sandstone",
        "minecraft:cut_sandstone",
        "minecraft:smooth_sandstone",
        "minecraft:red_sandstone",
        "minecraft:chiseled_red_sandstone",
        "minecraft:cut_red_sandstone",
        "minecraft:smooth_red_sandstone",
        "minecraft:quartz_block",
        "minecraft:chiseled_quartz_block",
        "minecraft:quartz_pillar",
        "minecraft:smooth_quartz",
        "minecraft:deepslate",
        "minecraft:smooth_basalt",
        "minecraft:cracked_deepslate_tiles",
        "minecraft:cracked_deepslate_bricks",
        "minecraft:leaf_litter",
    ];
    match stack.item_id() {
        "minecraft:hardened_clay" => 0.35,
        "minecraft:sponge" => 0.15,
        id if TENTH.contains(&id) => 0.1,
        _ => stack
            .item()
            .map_or(0.0, |item| item.data().furnace_xp_multiplier),
    }
}

/// `floor(value)`, plus one with probability equal to the fraction.
fn probabilistic_round(world: &mut World, value: f32) -> i32 {
    let floor = value.floor();
    let fraction = value - floor;
    let up = fraction > 0.0
        && world
            .get_resource_or_init::<EntityRandom>()
            .0
            .r#gen::<f32>()
            < fraction;
    floor as i32 + i32::from(up)
}

fn pay_smelting_xp(world: &mut World, player: Entity, pos: IVec3, taken: &ItemStack) {
    let mut points = probabilistic_round(world, f32::from(taken.count) * xp_multiplier(taken));
    let stored = container_mut(world, pos).map_or(0, |mut container| {
        std::mem::take(&mut container.furnace.stored_xp)
    });
    points += probabilistic_round(world, stored as f32 / 100.0);
    grant_experience(world, player, points);
}

/// XP points needed to go from `level` to the next (Bedrock curve).
fn level_cost(level: i32) -> i32 {
    match level {
        ..16 => 2 * level + 7,
        16..31 => 5 * level - 38,
        _ => 9 * level - 158,
    }
}

/// Add XP points and resend the level and progress attributes.
pub(super) fn grant_experience(world: &mut World, player: Entity, points: i32) {
    if points <= 0 {
        return;
    }
    let Some(mut experience) = world.get_mut::<Experience>(player) else {
        return;
    };
    let mut total = experience.progress * level_cost(experience.level) as f32 + points as f32;
    while total >= level_cost(experience.level) as f32 {
        total -= level_cost(experience.level) as f32;
        experience.level += 1;
    }
    experience.progress = (total / level_cost(experience.level) as f32).clamp(0.0, 1.0);
    let (level, progress) = (experience.level, experience.progress);
    let Some(runtime_id) = world.get::<RuntimeEntityId>(player).map(|id| id.0) else {
        return;
    };
    let attribute = |name: &str, current: f32, max: f32| jolyne::valentine::AttributeData {
        min_value: 0.0,
        max_value: max,
        current_value: current,
        default_min_value: 0.0,
        default_max_value: max,
        default_value: 0.0,
        name: name.into(),
        modifiers: vec![],
    };
    let tick = world
        .get_resource::<crate::ecs::resources::TickCounter>()
        .map_or(0, |tick| tick.current);
    send(
        world,
        player,
        jolyne::valentine::UpdateAttributesPacket {
            target_runtime_id: jolyne::valentine::ActorRuntimeId {
                actor_runtime_id: runtime_id as u64,
            },
            attribute_list: vec![
                attribute("minecraft:player.level", level as f32, i32::MAX as f32),
                attribute("minecraft:player.experience", progress, 1.0),
            ],
            tick: jolyne::valentine::PlayerInputTick { inputtick: tick },
        },
    );
}

// ── Placement and breaking ──────────────────────────────────────────────────

/// Cardinal neighbours a chest facing `direction` may pair with: along x for
/// north/south, along z for east/west.
fn pair_candidates(pos: IVec3, direction: &str) -> [IVec3; 2] {
    match direction {
        "east" | "west" => [pos - IVec3::Z, pos + IVec3::Z],
        _ => [pos - IVec3::X, pos + IVec3::X],
    }
}

/// A block was placed at `pos`: create its container actor and pair chests.
pub(super) fn placed(world: &mut World, pos: IVec3) {
    let Some((kind, _)) = kind_at(world, pos) else {
        return;
    };
    world.resource_mut::<BlockContainers>().live.remove(&pos);
    // A fresh actor replaces whatever stale actor data the position had.
    if let Some(chunk) = chunk_entity(world, pos)
        && let Some(mut data) = world.get_mut::<ChunkData>(chunk)
    {
        data.inner.remove_block_entity(pos.x, pos.y, pos.z);
    }
    if load(world, pos).is_none() {
        return;
    }
    // BDS announces a new container actor with its client-side state
    // (`{BlockEntityVersion, id, x, y, z}` after `/setblock chest`).
    send_client_state(world, pos);
    if kind.pairs() {
        try_pair(world, pos, kind);
    }
    save(world, pos);
}

fn try_pair(world: &mut World, pos: IVec3, kind: ContainerKind) {
    let Some(StateValue::Str(direction)) = state_value(world, pos, "minecraft:cardinal_direction")
    else {
        return;
    };
    for other in pair_candidates(pos, direction) {
        if kind_at(world, other).map(|(k, _)| k) != Some(kind)
            || state_value(world, other, "minecraft:cardinal_direction")
                != Some(StateValue::Str(direction))
            || load(world, other).is_none()
            || halves(world, other).is_none_or(|(_, paired)| paired.is_some())
        {
            continue;
        }
        // The other chest's window grows from 27 to 54 slots.
        close_viewers(world, other, kind);
        if let Some(mut existing) = container_mut(world, other) {
            existing.pair = Some(Pair {
                x: pos.x,
                z: pos.z,
                lead: true,
            });
        }
        if let Some(mut placed) = container_mut(world, pos) {
            placed.pair = Some(Pair {
                x: other.x,
                z: other.z,
                lead: false,
            });
        }
        save(world, other);
        save(world, pos);
        send_client_state(world, other);
        send_client_state(world, pos);
        return;
    }
}

/// What breaking a container block leaves behind.
pub(super) struct BrokenContainer {
    /// Stacks to scatter, and whether they split into random 10..30 chunks.
    pub drops: Vec<ItemStack>,
    pub randomize: bool,
}

/// Before the block at `pos` is removed: close its windows, unpair a double
/// chest and take its contents out.
pub(super) fn breaking(world: &mut World, pos: IVec3) -> Option<BrokenContainer> {
    let kind = load(world, pos)?;
    unpack_loot(world, pos);
    close_viewers(world, pos, kind);
    if let Some((_, Some(_))) = halves(world, pos) {
        let other = container(world, pos).and_then(pair_pos)?;
        if let Some(mut partner) = container_mut(world, other) {
            partner.pair = None;
        }
        save(world, other);
        send_client_state(world, other);
    }
    let mut containers = world.resource_mut::<BlockContainers>();
    containers.ticking.remove(&pos.to_array());
    let container = containers.live.remove(&pos)?;
    Some(BrokenContainer {
        drops: container
            .items
            .slots()
            .iter()
            .filter(|stack| !stack.is_empty())
            .cloned()
            .collect(),
        randomize: kind.smelter().is_none(),
    })
}

/// Scatter a broken container's contents: random 10..30 item chunks, or whole
/// stacks for furnaces.
pub(super) fn drop_contents(world: &mut World, pos: IVec3, broken: BrokenContainer) {
    for mut stack in broken.drops {
        while !stack.is_empty() {
            let chunk = if broken.randomize {
                let size = world
                    .get_resource_or_init::<EntityRandom>()
                    .0
                    .gen_range(10..=30);
                stack.count.min(size)
            } else {
                stack.count
            };
            let (part, rest) = stack.split(chunk);
            super::block_drops::pop_resource(world, pos, part.with_net_id(0).into_slot());
            stack = rest;
        }
    }
}

// ── Ticking ─────────────────────────────────────────────────────────────────

fn furnace_values(container: &BlockContainer) -> [i32; 4] {
    let f = container.furnace;
    [f.cook_time, f.lit_time, f.lit_duration, f.stored_xp]
}

fn set_data(window: u8, id: usize, value: i32) -> ContainerSetDataPacket {
    ContainerSetDataPacket {
        container_id: window,
        id: id as i32,
        value,
    }
}

/// Register furnaces and hoppers of a chunk that just loaded.
pub fn register_chunk_tickers(
    add: On<Add, ChunkData>,
    chunks: Query<&ChunkData>,
    blocks: Option<Res<BlockRegistryResource>>,
    containers: Option<ResMut<BlockContainers>>,
) {
    let (Ok(chunk), Some(blocks), Some(mut containers)) =
        (chunks.get(add.entity), blocks, containers)
    else {
        return;
    };
    for actor in chunk.inner.block_entities() {
        let [x, y, z] = actor.position();
        let (lx, ly, lz) = world_to_local_coords(x, y, z);
        let runtime_id = chunk.inner.get_block(lx, ly, lz);
        let kind = blocks
            .0
            .get_by_runtime_id(runtime_id)
            .and_then(|entry| ContainerKind::from_block(&entry.string_id));
        if kind.is_some_and(|kind| kind.smelter().is_some() || kind == ContainerKind::Hopper) {
            containers.ticking.insert([x, y, z]);
        }
    }
}

/// Exclusive per-tick system: furnaces, hoppers, viewer upkeep.
pub fn tick_block_containers(world: &mut World) {
    let tick = {
        let mut containers = world.resource_mut::<BlockContainers>();
        containers.tick += 1;
        containers.tick
    };
    let ticking: Vec<IVec3> = world
        .resource::<BlockContainers>()
        .ticking
        .iter()
        .map(|&pos| IVec3::from_array(pos))
        .collect();
    for pos in ticking {
        match load(world, pos) {
            Some(kind) if kind.smelter().is_some() => tick_furnace(world, pos, kind),
            Some(ContainerKind::Hopper) => tick_hopper(world, pos),
            _ => {
                let mut containers = world.resource_mut::<BlockContainers>();
                containers.ticking.remove(&pos.to_array());
                containers.live.remove(&pos);
            }
        }
    }
    upkeep_viewers(world, tick);
    if tick.is_multiple_of(200) {
        evict_unloaded(world);
    }
}

/// Drop viewers whose window closed without a close packet (disconnects),
/// send furnace value changes, and repeat the open-lid event.
fn upkeep_viewers(world: &mut World, tick: u64) {
    let lids: Vec<IVec3> = world
        .resource::<BlockContainers>()
        .viewers
        .keys()
        .copied()
        .collect();
    for lid in lids {
        let viewers = world.resource::<BlockContainers>().viewers[&lid].clone();
        let kind = container(world, lid).map(|c| c.kind);
        let mut kept = Vec::with_capacity(viewers.len());
        for mut viewer in viewers {
            let open = world
                .get::<ContainerWindows>(viewer.player)
                .is_some_and(|windows| {
                    windows.open_id() == Some(viewer.window)
                        && matches!(windows.screen(), ScreenKind::Block(_))
                });
            if !open {
                continue;
            }
            if let Some(container) = container(world, lid).filter(|c| c.kind.smelter().is_some()) {
                let values = furnace_values(container);
                for (id, (now, sent)) in values.iter().zip(viewer.sent).enumerate() {
                    if *now != sent {
                        send(world, viewer.player, set_data(viewer.window, id, *now));
                    }
                }
                viewer.sent = values;
            }
            kept.push(viewer);
        }
        let closed = kept.is_empty();
        if closed {
            world.resource_mut::<BlockContainers>().viewers.remove(&lid);
        } else {
            world
                .resource_mut::<BlockContainers>()
                .viewers
                .insert(lid, kept);
        }
        match kind {
            Some(kind) if closed && kind.viewer_sounds().is_some() => {
                set_lid(world, kind, lid, false)
            }
            Some(kind) if !closed && kind.has_lid() && tick.is_multiple_of(LID_KEEPALIVE_TICKS) => {
                broadcast(
                    world,
                    lid,
                    McpePacket::from(BlockEventPacket {
                        block_position: block_pos(lid),
                        event_type: CHEST_LID_EVENT,
                        event_value: 1,
                    }),
                );
            }
            _ => {}
        }
    }
}

fn evict_unloaded(world: &mut World) {
    let stale: Vec<IVec3> = world
        .resource::<BlockContainers>()
        .live
        .keys()
        .copied()
        .filter(|&pos| chunk_entity(world, pos).is_none())
        .collect();
    let mut containers = world.resource_mut::<BlockContainers>();
    for pos in stale {
        containers.live.remove(&pos);
        containers.ticking.remove(&pos.to_array());
    }
}

fn result_stack(
    world: &World,
    station: unastar_data::recipes::SmeltingStation,
    input: &ItemStack,
) -> Option<ItemStack> {
    let recipes = &world.get_resource::<RecipeRegistryResource>()?.0;
    let result = recipes.smelting_result(station, input.item_id(), input.damage)?;
    let data = if result.data == 0x7fff {
        0
    } else {
        result.data
    };
    super::items::world_stack(world, &result.name, result.count)
        .map(|stack| stack.with_damage(data))
}

/// Whether the furnace can smelt now: the input has a smelting result that
/// differs from it, and the output slot is empty or holds the same item with
/// room for one more result.
fn can_burn(world: &World, container: &BlockContainer) -> bool {
    let smelter = container.kind.smelter().expect("furnace kinds smelt");
    let input = container.items.item(0).unwrap();
    if input.is_empty() {
        return false;
    }
    let Some(result) = result_stack(world, smelter.station, input) else {
        return false;
    };
    if result.item_id() == input.item_id() && result.damage == input.damage {
        return false;
    }
    let output = container.items.item(2).unwrap();
    if output.is_empty() {
        return true;
    }
    if output.item_id() != result.item_id()
        || output.damage != result.damage
        || output.nbt != result.nbt
    {
        return false;
    }
    if output.count < 64 && output.count < output.max_stack_size() {
        return true;
    }
    output.count < result.max_stack_size()
}

fn tick_furnace(world: &mut World, pos: IVec3, kind: ContainerKind) {
    let smelter = kind.smelter().expect("furnace kinds smelt");
    let cook = i32::from(smelter.cook_ticks);
    let Some(snapshot) = container(world, pos).cloned() else {
        return;
    };
    let mut furnace = snapshot.clone();
    let was_lit = furnace.furnace.lit_time > 0;
    let mut can = can_burn(world, &furnace);
    let mut slots_changed = false;
    if furnace.furnace.lit_time > 0 {
        furnace.furnace.lit_time -= 1;
    }
    if furnace.furnace.lit_time == 0 && can {
        let fuel = furnace.items.item(1).unwrap().clone();
        let duration = burn_ticks(&fuel, cook as u32) as i32;
        furnace.furnace.lit_time = duration;
        furnace.furnace.lit_duration = duration;
        if duration >= 1 && !fuel.is_empty() {
            let used = if fuel.item_id() == "minecraft:lava_bucket" {
                super::items::world_stack(world, "minecraft:bucket", 1)
                    .map_or_else(ItemStack::empty, ItemStack::into_slot)
            } else {
                fuel.grow(-1)
            };
            let _ = furnace.items.set_item(1, used);
            slots_changed = true;
        }
    }
    if furnace.furnace.lit_time > 0 && can {
        furnace.furnace.cook_time += 1;
        if furnace.furnace.cook_time == cook {
            furnace.furnace.cook_time = 0;
            can = can_burn(world, &furnace);
            if can {
                let input = furnace.items.item(0).unwrap().clone();
                let result =
                    result_stack(world, smelter.station, &input).expect("can_burn found a result");
                let output = furnace.items.item(2).unwrap().clone();
                let output = if output.is_empty() {
                    result.into_slot()
                } else {
                    output.grow(result.count as i16)
                };
                let _ = furnace.items.set_item(2, output);
                let fuel = furnace.items.item(1).unwrap();
                if input.item_id() == "minecraft:wet_sponge"
                    && fuel.item_id() == "minecraft:bucket"
                    && fuel.count == 1
                    && fuel.damage == 0
                    && let Some(water) =
                        super::items::world_stack(world, "minecraft:water_bucket", 1)
                {
                    let _ = furnace.items.set_item(1, water.into_slot());
                }
                let _ = furnace.items.set_item(0, input.grow(-1));
                slots_changed = true;
            }
        }
    } else if furnace.furnace.cook_time > 0 {
        furnace.furnace.cook_time = (furnace.furnace.cook_time - 2).clamp(0, cook);
    }
    let lit = furnace.furnace.lit_time > 0;
    if furnace.furnace == snapshot.furnace && !slots_changed && was_lit == lit {
        return;
    }
    if let Some(mut live) = container_mut(world, pos) {
        *live = furnace;
    }
    if was_lit != lit {
        set_lit(world, pos, smelter, lit);
    }
    save(world, pos);
    if slots_changed {
        refresh_viewers(world, pos, None);
    }
}

/// Swap between the lit and unlit block, keeping the facing.
fn set_lit(world: &mut World, pos: IVec3, smelter: crate::block_entity::Smelter, lit: bool) {
    let blocks = world.resource::<BlockRegistryResource>().0.clone();
    let target = if lit { smelter.lit } else { smelter.unlit };
    let Some(entry) = blocks.get_by_name(target) else {
        return;
    };
    let mut state = entry.default_state_id;
    if let Some(direction) = state_value(world, pos, "minecraft:cardinal_direction") {
        state = entry
            .with_state_value(state, "minecraft:cardinal_direction", direction)
            .unwrap_or(state);
    }
    super::blocks::set_block(world, pos, state, false);
}

/// Hopper output direction from `facing_direction` (0 down, 2..5 horizontal).
fn hopper_target(world: &World, pos: IVec3) -> IVec3 {
    let facing = match state_value(world, pos, "facing_direction") {
        Some(StateValue::Int(value)) => value,
        _ => 0,
    };
    pos + match facing {
        2 => IVec3::NEG_Z,
        3 => IVec3::Z,
        4 => IVec3::NEG_X,
        5 => IVec3::X,
        _ => IVec3::NEG_Y,
    }
}

/// Insert one item of `stack` into `slots[range]`, merging first.
fn insert_one(slots: &mut Inventory, range: std::ops::Range<usize>, stack: &ItemStack) -> bool {
    let one = stack.clone().with_count(1);
    for slot in range.clone() {
        let target = slots.item(slot).unwrap();
        if !target.is_empty() && target.comparable(&one) && target.count < target.max_stack_size() {
            let grown = target.grow(1);
            let _ = slots.set_item(slot, grown);
            return true;
        }
    }
    for slot in range {
        if slots.item(slot).unwrap().is_empty() {
            let _ = slots.set_item(slot, one.with_net_id(0).into_slot());
            return true;
        }
    }
    false
}

/// Slots of a container a hopper may insert `stack` into, entering from
/// above (`from_above`) or a side.
fn insert_range(
    kind: ContainerKind,
    size: usize,
    stack: &ItemStack,
    from_above: bool,
) -> Option<std::ops::Range<usize>> {
    match kind {
        ContainerKind::EnderChest => None,
        _ if kind.smelter().is_some() => {
            if from_above {
                Some(0..1)
            } else {
                (burn_ticks(stack, 200) > 0).then_some(1..2)
            }
        }
        _ => Some(0..size),
    }
}

/// Slots a hopper below may pull from.
fn extract_slots(kind: ContainerKind, slots: &Inventory) -> Vec<usize> {
    match kind {
        ContainerKind::EnderChest => Vec::new(),
        _ if kind.smelter().is_some() => {
            let mut out = vec![2];
            if slots
                .item(1)
                .is_some_and(|fuel| fuel.item_id() == "minecraft:bucket")
            {
                out.push(1);
            }
            out
        }
        _ => (0..slots.size()).collect(),
    }
}

fn tick_hopper(world: &mut World, pos: IVec3) {
    let disabled = matches!(
        state_value(world, pos, "toggle_bit"),
        Some(StateValue::Byte(1))
    );
    let Some(mut cooldown) = container(world, pos).map(|c| c.transfer_cooldown) else {
        return;
    };
    if cooldown > 0 {
        cooldown -= 1;
        if let Some(mut hopper) = container_mut(world, pos) {
            hopper.transfer_cooldown = cooldown;
        }
        if cooldown > 0 || disabled {
            if cooldown == 0 {
                save(world, pos);
            }
            return;
        }
    }
    if disabled {
        return;
    }
    let pushed = hopper_push(world, pos);
    let pulled = hopper_pull(world, pos);
    if pushed || pulled {
        if let Some(mut hopper) = container_mut(world, pos) {
            hopper.transfer_cooldown = HOPPER_COOLDOWN;
        }
        save(world, pos);
        refresh_viewers(world, pos, None);
    }
}

fn hopper_push(world: &mut World, pos: IVec3) -> bool {
    let target = hopper_target(world, pos);
    let Some((target_kind, mut target_slots)) =
        load(world, target).and_then(|_| view(world, target))
    else {
        return false;
    };
    let Some(mut hopper) = container(world, pos).map(|c| c.items.clone()) else {
        return false;
    };
    let from_above = target == pos + IVec3::NEG_Y;
    for slot in 0..hopper.size() {
        let stack = hopper.item(slot).unwrap().clone();
        if stack.is_empty() {
            continue;
        }
        let Some(range) = insert_range(target_kind, target_slots.size(), &stack, from_above) else {
            continue;
        };
        if insert_one(&mut target_slots, range, &stack) {
            let _ = hopper.set_item(slot, stack.grow(-1));
            if let Some(mut live) = container_mut(world, pos) {
                live.items = hopper;
            }
            store_view(world, target, &target_slots);
            refresh_viewers(world, target, None);
            return true;
        }
    }
    false
}

fn hopper_pull(world: &mut World, pos: IVec3) -> bool {
    let above = pos + IVec3::Y;
    let Some(mut hopper) = container(world, pos).map(|c| c.items.clone()) else {
        return false;
    };
    let size = hopper.size();
    if let Some((source_kind, mut source)) = load(world, above).and_then(|_| view(world, above)) {
        for slot in extract_slots(source_kind, &source) {
            let stack = source.item(slot).unwrap().clone();
            if stack.is_empty() || !insert_one(&mut hopper, 0..size, &stack) {
                continue;
            }
            let _ = source.set_item(slot, stack.grow(-1));
            if source_kind.smelter().is_some() && slot == 2 {
                let reward = (xp_multiplier(&stack) * 100.0) as i32;
                if let Some(mut furnace) = container_mut(world, above) {
                    furnace.furnace.stored_xp += reward;
                }
            }
            if let Some(mut live) = container_mut(world, pos) {
                live.items = hopper;
            }
            store_view(world, above, &source);
            refresh_viewers(world, above, None);
            return true;
        }
        return false;
    }
    pull_item_entities(world, pos, hopper)
}

/// Collect dropped items resting in the block space above the hopper.
fn pull_item_entities(world: &mut World, pos: IVec3, mut hopper: Inventory) -> bool {
    let min = pos.as_dvec3() + DVec3::new(0.0, 0.5, 0.0);
    let max = pos.as_dvec3() + DVec3::new(1.0, 2.0, 1.0);
    let mut query =
        world.query_filtered::<(Entity, &Position, &ItemEntityStack), With<DroppedItem>>();
    let candidates: Vec<(Entity, ItemStack)> = query
        .iter(world)
        .filter(|(_, position, stack)| {
            !stack.0.is_empty() && position.0.cmpge(min).all() && position.0.cmplt(max).all()
        })
        .map(|(entity, _, stack)| (entity, stack.0.clone()))
        .collect();
    let mut changed = false;
    for (entity, stack) in candidates {
        let (added, leftover) = hopper.add_item(stack);
        if added == 0 {
            continue;
        }
        changed = true;
        if leftover.is_empty() {
            world.despawn(entity);
        } else if let Some(mut item) = world.get_mut::<ItemEntityStack>(entity) {
            item.0 = leftover;
        }
    }
    if changed && let Some(mut live) = container_mut(world, pos) {
        live.items = hopper;
    }
    changed
}

/// Forget the cached actor when another path replaced a container block with
/// a different kind (plugins, falling blocks); its chunk actor is gone too.
pub fn forget_replaced(
    changed: On<crate::world::ecs::BlockChanged>,
    blocks: Option<Res<BlockRegistryResource>>,
    containers: Option<ResMut<BlockContainers>>,
) {
    let (Some(blocks), Some(mut containers)) = (blocks, containers) else {
        return;
    };
    let event = changed.event();
    let kind = |runtime_id: u32| {
        blocks
            .0
            .get_by_runtime_id(runtime_id)
            .and_then(|entry| ContainerKind::from_block(&entry.string_id))
    };
    if kind(event.old_block) != kind(event.new_block) {
        containers.live.remove(&event.block_pos);
        containers.ticking.remove(&event.block_pos.to_array());
    }
}

#[cfg(test)]
mod tests;
