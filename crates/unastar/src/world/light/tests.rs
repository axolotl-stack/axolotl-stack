use std::sync::Arc;

use super::*;
use crate::world::WorldConfig;
use crate::world::ecs::ChunkPosition;
use crate::world::rules::{GameRules, WorldTime};

/// The torch's emission.
const TORCH: u8 = 14;

fn state(name: &str) -> u32 {
    BlockRegistry::vanilla()
        .default_state_id_by_name(name)
        .unwrap()
}

/// An empty world holding no columns yet.
fn world() -> World {
    let mut world = World::new();
    world.insert_resource(ChunkManager::new(WorldConfig::void_spawn_platform(0)));
    world.insert_resource(BlockRegistryResource(Arc::new(
        BlockRegistry::vanilla().clone(),
    )));
    world.insert_resource(WorldTime::default());
    world.insert_resource(GameRules::default());
    world
}

/// Add an all-air column at `(cx, cz)` with `blocks` written into it.
fn column(world: &mut World, cx: i32, cz: i32, blocks: &[(IVec3, &str)]) {
    let mut chunk = Chunk::new(cx, cz);
    for &(pos, name) in blocks {
        let (x, y, z) = world_to_local_coords(pos.x, pos.y, pos.z);
        assert_eq!(world_to_chunk_coords(pos.x, pos.z), (cx, cz));
        chunk.set_block(x, y, z, state(name));
    }
    let entity = world.spawn(ChunkData::new(chunk)).id();
    world
        .resource_mut::<ChunkManager>()
        .insert(ChunkPosition::new(cx, cz), entity);
}

/// Write a block into a loaded column, as gameplay does.
fn put(world: &mut World, pos: IVec3, name: &str) {
    let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
    let entity = world
        .resource::<ChunkManager>()
        .get_by_coords(cx, cz)
        .unwrap();
    let (x, y, z) = world_to_local_coords(pos.x, pos.y, pos.z);
    world
        .get_mut::<ChunkData>(entity)
        .unwrap()
        .inner
        .set_block(x, y, z, state(name));
}

/// Torch light at `pos` from torches at `torches` through open air.
fn open_air(pos: IVec3, torches: &[IVec3]) -> u8 {
    torches
        .iter()
        .map(|torch| {
            let d = (pos - *torch).abs();
            TORCH.saturating_sub((d.x + d.y + d.z).min(15) as u8)
        })
        .max()
        .unwrap_or(0)
}

/// Every cell within 16 blocks of `center` holds the open-air light of
/// `torches`.
fn assert_open_air(world: &World, center: IVec3, torches: &[IVec3]) {
    for dx in -16..=16 {
        for dy in -16..=16 {
            for dz in -16..=16 {
                let pos = center + IVec3::new(dx, dy, dz);
                assert_eq!(
                    block_light(world, pos),
                    open_air(pos, torches),
                    "{pos} with torches {torches:?}"
                );
            }
        }
    }
}

#[test]
fn a_torch_lights_14_at_its_cell_and_one_less_per_block_through_air() {
    let mut world = world();
    let torch = IVec3::new(8, 64, 8);
    column(&mut world, 0, 0, &[(torch, "minecraft:torch")]);
    update_block_light(&mut world);
    assert_eq!(block_light(&world, torch), 14);
    assert_eq!(block_light(&world, torch + IVec3::X), 13);
    assert_eq!(block_light(&world, torch + IVec3::new(0, 5, 0)), 9);
    assert_eq!(block_light(&world, torch + IVec3::new(1, 1, 1)), 11);
    assert_eq!(block_light(&world, torch + IVec3::new(0, -13, 0)), 1);
    assert_eq!(block_light(&world, torch + IVec3::new(0, 14, 0)), 0);
    // Inside the column; the neighbouring columns are not loaded.
    for dx in -8..=7 {
        for dy in -16..=16 {
            for dz in -8..=7 {
                let pos = torch + IVec3::new(dx, dy, dz);
                assert_eq!(block_light(&world, pos), open_air(pos, &[torch]), "{pos}");
            }
        }
    }
}

#[test]
fn stone_blocks_torch_light() {
    let mut world = world();
    let torch = IVec3::new(8, 64, 8);
    let mut blocks = vec![(torch, "minecraft:torch")];
    blocks.extend(FACES.map(|face| (torch + face, "minecraft:stone")));
    column(&mut world, 0, 0, &blocks);
    update_block_light(&mut world);
    assert_eq!(block_light(&world, torch), TORCH);
    for face in FACES {
        assert_eq!(block_light(&world, torch + face), 0, "stone at {face}");
        assert_eq!(
            block_light(&world, torch + face * 2),
            0,
            "beyond the stone at {face}"
        );
    }
    assert_eq!(block_light(&world, torch + IVec3::new(1, 1, 0)), 0);

    // A wall in open air: light reaches behind it only around it.
    let mut world = self::world();
    let wall = [-1, 0, 1].map(|dy| (torch + IVec3::new(1, dy, 0), "minecraft:stone"));
    let mut blocks = vec![(torch, "minecraft:torch")];
    blocks.extend(wall);
    column(&mut world, 0, 0, &blocks);
    update_block_light(&mut world);
    // Around the wall's side: 1 north, 2 east, 1 south.
    assert_eq!(block_light(&world, torch + IVec3::new(2, 0, 0)), TORCH - 4);
    assert_eq!(block_light(&world, torch + IVec3::new(1, 0, 0)), 0);
}

#[test]
fn placing_and_removing_a_torch_updates_the_light() {
    let mut world = world();
    for cx in -1..=1 {
        for cz in -1..=1 {
            column(&mut world, cx, cz, &[]);
        }
    }
    update_block_light(&mut world);
    let first = IVec3::new(4, 64, 4);
    let second = IVec3::new(10, 66, 6);

    put(&mut world, first, "minecraft:torch");
    assert_eq!(block_light(&world, first), 0, "lit at the next update");
    update_block_light(&mut world);
    assert_open_air(&world, first, &[first]);

    put(&mut world, second, "minecraft:torch");
    update_block_light(&mut world);
    assert_open_air(&world, first, &[first, second]);

    // Removing one torch unlights its light and keeps the other's.
    put(&mut world, first, "minecraft:air");
    update_block_light(&mut world);
    assert_open_air(&world, first, &[second]);

    put(&mut world, second, "minecraft:air");
    update_block_light(&mut world);
    assert_open_air(&world, first, &[]);
}

#[test]
fn walls_placed_and_broken_shade_and_relight() {
    let mut world = world();
    let torch = IVec3::new(8, 64, 8);
    column(&mut world, 0, 0, &[(torch, "minecraft:torch")]);
    update_block_light(&mut world);
    // Enclose the torch: everything outside goes dark.
    for face in FACES {
        put(&mut world, torch + face, "minecraft:stone");
    }
    update_block_light(&mut world);
    assert_eq!(block_light(&world, torch), TORCH);
    assert_eq!(block_light(&world, torch + IVec3::new(2, 0, 0)), 0);
    assert_eq!(block_light(&world, torch + IVec3::new(1, 1, 0)), 0);
    // Open one side: light flows out through it again.
    put(&mut world, torch + IVec3::Y, "minecraft:air");
    update_block_light(&mut world);
    assert_eq!(block_light(&world, torch + IVec3::Y), TORCH - 1);
    assert_eq!(block_light(&world, torch + IVec3::new(1, 1, 0)), TORCH - 2);
    assert_eq!(block_light(&world, torch + IVec3::new(2, 0, 0)), TORCH - 4);
}

#[test]
fn light_crosses_column_borders() {
    let mut world = world();
    // A torch on the east edge of column (0, 0); column (1, 0) arrives
    // after it was lit and takes the light in.
    let torch = IVec3::new(15, 64, 8);
    column(&mut world, 0, 0, &[(torch, "minecraft:torch")]);
    update_block_light(&mut world);
    assert_eq!(block_light(&world, torch + IVec3::X), 0, "not loaded");
    column(&mut world, 1, 0, &[]);
    update_block_light(&mut world);
    assert_eq!(block_light(&world, torch + IVec3::X), TORCH - 1);
    assert_eq!(block_light(&world, torch + IVec3::new(13, 0, 0)), 1);
    assert_eq!(block_light(&world, torch + IVec3::new(2, 3, -4)), TORCH - 9);

    // Placed on the other side of the border once both are lit, then
    // removed: both columns follow.
    let other = IVec3::new(16, 70, 8);
    put(&mut world, other, "minecraft:torch");
    update_block_light(&mut world);
    assert_eq!(block_light(&world, IVec3::new(15, 70, 8)), TORCH - 1);
    assert_eq!(block_light(&world, IVec3::new(10, 70, 8)), TORCH - 6);
    put(&mut world, other, "minecraft:air");
    update_block_light(&mut world);
    assert_eq!(
        block_light(&world, IVec3::new(15, 70, 8)),
        open_air(IVec3::new(15, 70, 8), &[torch])
    );
    let east = IVec3::new(20, 70, 8);
    assert_eq!(block_light(&world, east), open_air(east, &[torch]));

    // A diagonal column lit later takes the light that came round.
    column(&mut world, 1, 1, &[]);
    update_block_light(&mut world);
    let corner = IVec3::new(16, 64, 16);
    assert_eq!(block_light(&world, corner), open_air(corner, &[torch]));
}

#[test]
fn brightness_is_the_brighter_of_darkened_sky_and_block_light() {
    let mut world = world();
    let torch = IVec3::new(8, 64, 8);
    // A stone roof at y = 70 over the whole column.
    let mut blocks = vec![(torch, "minecraft:torch")];
    for x in 0..16 {
        for z in 0..16 {
            blocks.push((IVec3::new(x, 70, z), "minecraft:stone"));
        }
    }
    column(&mut world, 0, 0, &blocks);
    update_block_light(&mut world);
    let beside = torch + IVec3::new(3, 0, 0);
    // Midnight darkens the sky by 11.
    assert_eq!(brightness_with_darken(&world, beside, 11), TORCH - 3);
    assert_eq!(brightness_with_darken(&world, IVec3::new(8, 71, 8), 11), 4);
    assert_eq!(brightness_with_darken(&world, IVec3::new(8, 71, 8), 0), 15);
    // Under the roof far from the torch it is dark.
    assert_eq!(brightness_with_darken(&world, IVec3::new(0, 50, 0), 0), 0);
    // Inside the roof: 0 although light reaches the cell next to it.
    assert_eq!(brightness_with_darken(&world, IVec3::new(8, 70, 8), 0), 0);
    assert_eq!(
        brightness_with_darken(&world, IVec3::new(8, 69, 8), 0),
        TORCH - 5
    );
    // The time of day's darkening applies to the sky only.
    world.resource_mut::<WorldTime>().ticks = 18_000;
    assert_eq!(brightness(&world, beside), TORCH - 3);
    assert_eq!(brightness(&world, IVec3::new(8, 71, 8)), 4);
}

#[test]
fn writes_of_blocks_without_light_allocate_no_light() {
    let mut world = world();
    column(&mut world, 0, 0, &[]);
    update_block_light(&mut world);
    // Air to stone to cobblestone: none emits light.
    let pos = IVec3::new(3, 64, 3);
    put(&mut world, pos, "minecraft:stone");
    put(&mut world, pos, "minecraft:cobblestone");
    update_block_light(&mut world);
    let entity = world
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    assert_eq!(
        world
            .get::<ChunkBlockLight>(entity)
            .unwrap()
            .section_bytes(),
        0,
        "no section holds light"
    );
}

/// Memory and time of lighting columns: prints the cost of eight
/// emitter-free columns, then of one column holding 64 torches over a
/// stone floor that spreads into them (`--nocapture`).
#[test]
fn lighting_a_column_is_cheap() {
    let mut world = world();
    for cx in -1..=1 {
        for cz in -1..=1 {
            let mut blocks = Vec::new();
            for x in 0..16 {
                for z in 0..16 {
                    blocks.push((IVec3::new(cx * 16 + x, 63, cz * 16 + z), "minecraft:stone"));
                }
            }
            if (cx, cz) == (0, 0) {
                for x in (0..16).step_by(2) {
                    for z in (0..16).step_by(2) {
                        blocks.push((IVec3::new(x, 64, z), "minecraft:torch"));
                    }
                }
            }
            column(&mut world, cx, cz, &blocks);
        }
    }
    // Light the neighbours first so the centre column spreads into them.
    let center = world
        .resource::<ChunkManager>()
        .get_by_coords(0, 0)
        .unwrap();
    let data = world.entity_mut(center).take::<ChunkData>().unwrap();
    let started = std::time::Instant::now();
    update_block_light(&mut world);
    let dark = started.elapsed();
    world.entity_mut(center).insert(data);
    let started = std::time::Instant::now();
    update_block_light(&mut world);
    let elapsed = started.elapsed();
    let bytes = world
        .get::<ChunkBlockLight>(center)
        .unwrap()
        .section_bytes();
    println!(
        "lit 8 dark columns in {dark:?}; a 64-torch column in {elapsed:?}; \
         {bytes} B of light sections + {} B inline",
        std::mem::size_of::<ChunkBlockLight>()
    );
    assert_eq!(block_light(&world, IVec3::new(0, 64, 0)), TORCH);
    assert!(bytes <= 3 * SECTION_BYTES);
}
