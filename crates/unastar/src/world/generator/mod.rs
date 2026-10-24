//! World generation.
//!
//! Every generator implements [`Generator`] and runs through the shared [`ChunkPipeline`]:
//! per-chunk terrain stages produce a [`ProtoChunk`], a 3x3 [`DecorationRegion`] stage
//! places features and structures, and the pipeline assembles finished chunks (see
//! [`pipeline`] for the dataflow and its purity invariant).
//!
//! | `kind` | Generator | Truth source |
//! |---|---|---|
//! | `java` | [`JavaGenerator`] | Java Edition 1.21.11 reference worlds |
//! | `super_flat` | [`FlatGenerator`] | classic superflat layers |
//! | `void_spawn_platform` | [`VoidGenerator`] | stone platform around the origin |

pub mod aquifer;
pub mod biome_source;
pub mod density;
pub mod flat;
pub mod noise;
pub mod ore_veinifier;
pub mod parity;
pub mod pipeline;
pub mod proto;
pub mod region;
mod structures;
pub mod surface;
mod terrain;
pub mod xoroshiro;

pub use flat::{FlatGenerator, VoidGenerator};
pub use pipeline::{ChunkPipeline, Generator, SpawnPoint};
pub use proto::{NativeBiome, ProtoChunk, QuartBiomes};
pub use region::{DecorationRegion, HeightmapKind, WorldGenRegion};
pub use structures::{
    StructureConfig, StructurePos, StructureType, find_structures_in_area, get_structure_pos,
};
pub use terrain::JavaGenerator;
