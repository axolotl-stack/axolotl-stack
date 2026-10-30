//! A player whose whole view changes at once (a respawn far away, a `/tp`
//! back) into a fully stored world is streamed without ever filling the
//! RakNet session queue that disconnects a player, even over a slow link.
//!
//! Headless: the client and its link are simulated (`support/stream_sim.rs`);
//! the world's storage returns dense, poorly compressible terrain for every
//! column, so a radius-12 view is several megabytes on the wire.

#[path = "support/stream_sim.rs"]
mod stream_sim;

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use glam::DVec3;
use stream_sim::{SESSION_QUEUE_LIMIT, SimPlayer};
use unastar::storage::{ChunkColumn, StorageResult, WorldProvider};
use unastar::world::chunk::{BlockStateSpace, blocks};
use unastar::world::ecs::ChunkManager;
use unastar::world::{Chunk, ChunkPos, SUBCHUNK_COUNT, WorldConfig, WorldGenerator};
use unastar::{CommandSource, GameServer, ServerConfig};

const RADIUS: i32 = 12;
const A: DVec3 = DVec3::new(0.5, 80.0, 0.5);
const B: DVec3 = DVec3::new(5000.5, 80.0, 0.5);

/// Every column is stored: eight sections of mixed stone variants and ores.
struct DenseWorld;

#[async_trait]
impl WorldProvider for DenseWorld {
    async fn load_column(&self, pos: ChunkPos, _: i32) -> StorageResult<Option<ChunkColumn>> {
        let palette = [
            *blocks::STONE,
            *blocks::DIRT,
            *blocks::GRAVEL,
            *blocks::GRANITE,
            *blocks::DIORITE,
            *blocks::ANDESITE,
            *blocks::COAL_ORE,
            *blocks::IRON_ORE,
        ];
        let mut state = (pos.x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ pos.z as u64;
        let mut dense = vec![*blocks::AIR; usize::from(SUBCHUNK_COUNT) * 4096];
        for block in dense.iter_mut().take(8 * 4096) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            *block = palette[(state % 8) as usize];
        }
        let chunk = Chunk::from_dense(pos.x, pos.z, &dense, BlockStateSpace::Bedrock);
        Ok(Some(ChunkColumn::new(chunk)))
    }
    async fn save_column(&self, _: ChunkPos, _: i32, _: &ChunkColumn) -> StorageResult<()> {
        Ok(())
    }
    async fn flush(&self) -> StorageResult<()> {
        Ok(())
    }
    async fn close(&self) -> StorageResult<()> {
        Ok(())
    }
}

/// Ticks (at most `limit`) until the view around `at` is complete; returns
/// how many it took.
fn load_view(server: &mut GameServer, player: &mut SimPlayer, at: DVec3, limit: usize) -> usize {
    for tick in 1..=limit {
        let started = Instant::now();
        player.send_requests(server);
        server.tick();
        player.receive();
        assert!(
            player.link.peak < SESSION_QUEUE_LIMIT,
            "the send queue reached {} bytes, over the session limit",
            player.link.peak
        );
        if player.missing(at, RADIUS) == 0 {
            return tick;
        }
        std::thread::sleep(Duration::from_millis(50).saturating_sub(started.elapsed()));
    }
    panic!(
        "the view never completed: {} columns missing",
        player.missing(at, RADIUS)
    );
}

#[test]
fn respawn_and_teleport_into_a_stored_view_stay_under_the_session_queue_limit() {
    let mut server = GameServer::with_config(ServerConfig {
        world: WorldConfig {
            generator: WorldGenerator::SuperFlat,
            ..WorldConfig::default()
        },
        max_chunk_radius: RADIUS,
        ..ServerConfig::default()
    });
    server
        .ecs
        .world_mut()
        .resource_mut::<ChunkManager>()
        .set_provider(Arc::new(DenseWorld));
    // A slow link: 100 KiB per tick, 2 MB/s.
    let mut player = SimPlayer::join(&mut server, 1, A, RADIUS, 100 * 1024);
    load_view(&mut server, &mut player, A, 2000);

    // Die and respawn far away.
    {
        let world = server.ecs.world_mut();
        world
            .entity_mut(player.entity)
            .insert(unastar::entity::player_life::respawn::PlayerSpawnPoint(B));
        assert!(unastar::entity::player_life::kill_player(
            world,
            player.entity
        ));
        unastar::entity::player_life::respawn::respawn(world, player.entity);
    }
    player.forget_terrain();
    load_view(&mut server, &mut player, B, 2000);

    // Teleport back while the first view is still in memory.
    let output = unastar::command::execute(
        server.ecs.world_mut(),
        CommandSource::server(),
        "tp SimPlayer1 0.5 80 0.5",
    );
    assert!(output.success_count > 0, "{:?}", output.messages);
    player.forget_terrain();
    load_view(&mut server, &mut player, A, 2000);
    assert!(player.link.peak > 0);
}
