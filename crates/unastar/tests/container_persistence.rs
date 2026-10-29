//! Container block actors (chest contents and pairing, furnace progress,
//! hopper cooldown) survive a save and reopen through both world providers.

use std::path::Path;
use std::sync::Arc;

use glam::IVec3;
use unastar::block_entity::{BlockContainer, ContainerKind, FurnaceState, Pair};
use unastar::registry::{BlockRegistry, ItemRegistry};
use unastar::storage::{BlazeDBProvider, ChunkColumn, LevelDBWorldProvider, WorldProvider};
use unastar::world::block_entity::BlockEntity;
use unastar::world::{Chunk, ChunkPos};

fn items() -> ItemRegistry {
    let mut items = ItemRegistry::new();
    items.load_vanilla();
    items
}

fn containers(items: &ItemRegistry) -> Vec<BlockContainer> {
    let mut chest = BlockContainer::new(ContainerKind::Chest, IVec3::new(-30, 64, 20));
    chest
        .items
        .set_item(
            0,
            items
                .stack("minecraft:cobblestone", 64)
                .unwrap()
                .into_slot(),
        )
        .unwrap();
    chest
        .items
        .set_item(
            13,
            items
                .stack("minecraft:iron_pickaxe", 1)
                .unwrap()
                .with_damage(31)
                .into_slot(),
        )
        .unwrap();
    chest.pair = Some(Pair {
        x: -29,
        z: 20,
        lead: true,
    });
    let mut furnace = BlockContainer::new(ContainerKind::Smoker, IVec3::new(-25, 65, 18));
    furnace
        .items
        .set_item(0, items.stack("minecraft:beef", 9).unwrap().into_slot())
        .unwrap();
    furnace.furnace = FurnaceState {
        lit_time: 700,
        lit_duration: 800,
        cook_time: 37,
        stored_xp: 135,
    };
    let mut hopper = BlockContainer::new(ContainerKind::Hopper, IVec3::new(-20, 70, 30));
    hopper
        .items
        .set_item(4, items.stack("minecraft:stick", 3).unwrap().into_slot())
        .unwrap();
    hopper.transfer_cooldown = 5;
    vec![chest, furnace, hopper]
}

fn summary(container: &BlockContainer) -> String {
    let items: Vec<String> = container
        .items
        .items()
        .filter(|(_, stack)| !stack.is_empty())
        .map(|(slot, stack)| {
            format!(
                "{slot}:{}x{}@{}",
                stack.item_id(),
                stack.count,
                stack.damage
            )
        })
        .collect();
    format!(
        "{:?} {:?} {:?} {:?} {} {items:?}",
        container.kind,
        container.position,
        container.pair,
        container.furnace,
        container.transfer_cooldown
    )
}

enum Provider {
    LevelDB(LevelDBWorldProvider),
    BlazeDB(Arc<BlazeDBProvider>),
}

impl Provider {
    fn open(path: &Path, blaze: bool) -> Self {
        if blaze {
            Self::BlazeDB(BlazeDBProvider::open(path, None).unwrap())
        } else {
            Self::LevelDB(LevelDBWorldProvider::open(path, 0).unwrap())
        }
    }

    fn storage(&self) -> &dyn WorldProvider {
        match self {
            Self::LevelDB(provider) => provider,
            Self::BlazeDB(provider) => provider.as_ref(),
        }
    }
}

#[tokio::test]
async fn container_actors_survive_both_provider_reopens() {
    let items = items();
    let blocks = BlockRegistry::vanilla();
    let pos = ChunkPos::new(-2, 1);
    for blaze in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let mut chunk = Chunk::new(pos.x, pos.z);
        let saved = containers(&items);
        for container in &saved {
            let name = match container.kind {
                ContainerKind::Chest => "minecraft:chest",
                ContainerKind::Smoker => "minecraft:smoker",
                _ => "minecraft:hopper",
            };
            let p = container.position;
            chunk
                .set_block(
                    p.x.rem_euclid(16) as u8,
                    p.y as i16,
                    p.z.rem_euclid(16) as u8,
                    blocks.default_state_id_by_name(name).unwrap(),
                )
                .unwrap();
            assert!(
                chunk.set_block_entity(
                    BlockEntity::from_bedrock_compound(container.encode()).unwrap()
                )
            );
        }
        let provider = Provider::open(directory.path(), blaze);
        provider
            .storage()
            .save_column(pos, 0, &ChunkColumn::new(chunk))
            .await
            .unwrap();
        provider.storage().close().await.unwrap();
        drop(provider);

        let provider = Provider::open(directory.path(), blaze);
        let restored = provider
            .storage()
            .load_column(pos, 0)
            .await
            .unwrap()
            .unwrap();
        provider.storage().close().await.unwrap();
        for container in &saved {
            let actor = restored
                .chunk
                .block_entities()
                .iter()
                .find(|actor| actor.position() == container.position.to_array())
                .expect("actor persisted");
            let loaded = BlockContainer::decode(
                container.kind,
                container.position,
                actor.bedrock_compound().unwrap().unwrap(),
                &items,
                blocks,
            );
            assert_eq!(summary(&loaded), summary(container), "blaze={blaze}");
        }
    }
}
