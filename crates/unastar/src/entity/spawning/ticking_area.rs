//! The chunks the spawn cycle runs in.
//!
//! Every player (spectators and dead players too) ticks the chunk it stands
//! in and the chunks at a fixed set of offsets around it, by the chunk tick
//! range `n` (the simulation distance): the diamond `|dx| + |dz| <= n + 1`
//! without its four tips, so `|dx|` and `|dz|` stay within `n`; a range of
//! 0 or less ticks nothing. The spawn cycle rolls in each loaded chunk some
//! player ticks, and from tick range 5 up a spawn position needs its chunk
//! and the 8 around it ticked (matches BDS 1.26.51).

use std::collections::HashSet;

use glam::{DVec3, IVec3};

use crate::world::ecs::systems::world_to_chunk_coords;

/// Offsets from a player's chunk to the chunks it ticks for tick range
/// `range` (see the module docs).
pub(super) fn tick_offsets(range: i32) -> impl Iterator<Item = (i32, i32)> {
    (-range..=range)
        .filter(move |_| range > 0)
        .flat_map(move |dz| {
            let half = range.min(range + 1 - dz.abs());
            (-half..=half).map(move |dx| (dx, dz))
        })
}

/// The loaded chunks players tick this tick.
#[derive(Debug, Default)]
pub(super) struct TickedChunks(HashSet<(i32, i32)>);

impl TickedChunks {
    /// The chunks the players at `players` tick for tick range `range`,
    /// among those `loaded` accepts.
    pub(super) fn around(
        players: impl IntoIterator<Item = DVec3>,
        range: i32,
        loaded: impl Fn((i32, i32)) -> bool,
    ) -> Self {
        let mut chunks = HashSet::new();
        for player in players {
            let at = player.floor().as_ivec3();
            let (cx, cz) = world_to_chunk_coords(at.x, at.z);
            for (dx, dz) in tick_offsets(range) {
                let chunk = (cx + dx, cz + dz);
                if loaded(chunk) {
                    chunks.insert(chunk);
                }
            }
        }
        Self(chunks)
    }

    pub(super) fn contains(&self, chunk: (i32, i32)) -> bool {
        self.0.contains(&chunk)
    }

    /// The chunk of `feet` and the 8 around it tick.
    pub(super) fn neighbourhood(&self, feet: IVec3) -> bool {
        let (cx, cz) = world_to_chunk_coords(feet.x, feet.z);
        (-1..=1).all(|dx| (-1..=1).all(|dz| self.contains((cx + dx, cz + dz))))
    }

    /// The ticked chunks in coordinate order.
    pub(super) fn sorted(&self) -> Vec<(i32, i32)> {
        let mut chunks: Vec<_> = self.0.iter().copied().collect();
        chunks.sort_unstable();
        chunks
    }
}
