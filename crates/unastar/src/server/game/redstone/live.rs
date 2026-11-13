//! The circuit in the live world: block writes note outside changes, and an
//! exclusive system steps the circuit once per game tick.

use bevy_ecs::prelude::*;
use glam::IVec3;

use super::{Circuit, Redstone};
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::server::game::types::BlockRegistryResource;

/// The live world as a circuit: reads and writes go through the chunk store
/// (writes reach viewers and run no block updates).
struct Live<'w>(&'w mut World);

impl BlockSource for Live<'_> {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        self.0.block_at(pos)
    }

    fn registry(&self) -> &BlockRegistry {
        self.0.registry()
    }
}

impl Circuit for Live<'_> {
    fn write(&mut self, pos: IVec3, state: u32) {
        crate::server::game::blocks::write_circuit_block(self.0, pos, state);
    }

    fn container_signal(&mut self, pos: IVec3) -> Option<u8> {
        crate::server::game::containers::comparator_signal(self.0, pos)
    }
}

/// Outside changes made while the circuit steps (block updates its own
/// changes caused), handed to it when the step ends.
#[derive(Resource, Default)]
struct Deferred(Vec<(IVec3, u32)>);

/// A block at `pos` changed from `old` outside the circuit (no-op in a world
/// without one). The circuit's own writes are not noted
/// ([`crate::server::game::blocks::write_circuit_block`]); a change made
/// while the circuit steps (it is out of the world then) lands in
/// [`Deferred`].
pub(crate) fn note_change(world: &mut World, pos: IVec3, old: u32, new: u32) {
    if old == new {
        return;
    }
    if let Some(mut redstone) = world.get_resource_mut::<Redstone>() {
        redstone.outside_change(pos, old);
    } else if let Some(mut deferred) = world.get_resource_mut::<Deferred>() {
        deferred.0.push((pos, old));
    }
}

/// Step the circuit one game tick (exclusive: edits chunks).
pub(crate) fn tick_redstone(world: &mut World) {
    let Some(registry) = world
        .get_resource::<BlockRegistryResource>()
        .map(|registry| registry.0.clone())
    else {
        return;
    };
    if !world.contains_resource::<Redstone>() {
        world.insert_resource(Redstone::default());
    }
    // The block updates of the circuit's changes of the last tick run now,
    // as outside changes (a dust on a trapdoor the circuit opened pops).
    let written = world.resource_mut::<Redstone>().take_written();
    for pos in written {
        crate::server::game::block_update::world_changed(world, pos, false);
    }
    world.resource_scope(|world, mut redstone: Mut<Redstone>| {
        world.insert_resource(Deferred::default());
        redstone.step(&registry, &mut Live(world));
        // Anything else written during the step is an outside change of
        // this tick.
        if let Some(deferred) = world.remove_resource::<Deferred>() {
            for (pos, old) in deferred.0 {
                redstone.outside_change(pos, old);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use bevy_ecs::message::Messages;
    use bevy_ecs::prelude::*;
    use glam::IVec3;

    use super::tick_redstone;
    use crate::registry::BlockRegistry;
    use crate::registry::block::StateValue;
    use crate::server::game::blocks::{block_at, set_block};
    use crate::world::ecs::{BlockBroadcastEvent, ChunkData, ChunkManager, ChunkPosition};

    fn id(name: &str) -> u32 {
        BlockRegistry::vanilla()
            .default_state_id_by_name(name)
            .unwrap()
    }

    fn name(world: &World, pos: IVec3) -> String {
        let state = block_at(world, (pos.x, pos.y, pos.z)).unwrap();
        BlockRegistry::vanilla()
            .get_by_runtime_id(state)
            .unwrap()
            .string_id
            .clone()
    }

    /// The live world: a block set through the game's block path reaches the
    /// circuit, which lights dust and a lamp in its first update.
    #[test]
    fn redstone_block_lights_dust_and_lamp_in_the_live_world() {
        let mut world = World::new();
        world.insert_resource(ChunkManager::new(
            crate::world::WorldConfig::void_spawn_platform(1),
        ));
        world.insert_resource(crate::server::game::types::BlockRegistryResource(
            std::sync::Arc::new(BlockRegistry::vanilla().clone()),
        ));
        world.insert_resource(Messages::<BlockBroadcastEvent>::default());
        let mut chunk = crate::world::chunk::Chunk::new(0, 0);
        for x in 0..8 {
            chunk.set_block(x, 64, 1, id("minecraft:stone"));
        }
        let chunk = world.spawn(ChunkData::new(chunk)).id();
        world
            .resource_mut::<ChunkManager>()
            .insert(ChunkPosition::new(0, 0), chunk);
        let (dust, lamp) = (IVec3::new(3, 65, 1), IVec3::new(4, 65, 1));
        set_block(&mut world, dust, id("minecraft:redstone_wire"), false);
        set_block(&mut world, lamp, id("minecraft:redstone_lamp"), false);
        tick_redstone(&mut world);
        tick_redstone(&mut world);
        set_block(
            &mut world,
            IVec3::new(2, 65, 1),
            id("minecraft:redstone_block"),
            false,
        );
        // Tick 3 hides the change; tick 4 is the next update.
        tick_redstone(&mut world);
        assert_eq!(name(&world, lamp), "minecraft:redstone_lamp");
        tick_redstone(&mut world);
        assert_eq!(name(&world, lamp), "minecraft:lit_redstone_lamp");
        let state = block_at(&world, (dust.x, dust.y, dust.z)).unwrap();
        let entry = BlockRegistry::vanilla().get_by_runtime_id(state).unwrap();
        assert_eq!(
            entry.state_value(state, "redstone_signal"),
            Some(StateValue::Int(15))
        );
    }
}
