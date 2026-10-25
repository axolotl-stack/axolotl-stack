//! Java feature implementations, one module per feature type (or family),
//! each generic over [`super::FeatureWorld`] and driven by a
//! [`super::FeatureRandom`] seeded by the decoration driver.
//!
//! Signature convention: `place(world, rng, cfg, origin) -> bool`, where the
//! return value is Java's `Feature.place` result. Features that place nested
//! placed features take them as `FnMut(&mut W, &mut FeatureRandom, BlockPos) -> bool`
//! closures supplied by the generated code.

pub mod bamboo;
pub mod block_column;
pub mod block_pile;
pub mod bonus_chest;
pub mod desert_well;
pub mod disk;
pub mod dripstone;
pub mod end;
pub mod fallen_tree;
pub mod forest_rock;
pub mod fossil;
pub mod geode;
pub mod huge_mushroom;
pub mod ice;
pub mod lake;
pub mod monster_room;
pub mod multiface_growth;
pub mod nether;
// Boolean and float tests are written as in Java so draw order and NaN
// behaviour stay evident.
#[allow(clippy::nonminimal_bool, clippy::neg_cmp_op_on_partial_ord)]
pub mod ocean;
pub mod ore;
pub mod root_system;
// Boolean and float tests are written as in Java so draw order and NaN
// behaviour stay evident.
#[allow(clippy::nonminimal_bool, clippy::neg_cmp_op_on_partial_ord)]
pub mod sculk_patch;
pub mod simple_block;
pub mod spring;
pub mod tree;
pub mod underwater_magma;
// Boolean and float tests are written as in Java so draw order and NaN
// behaviour stay evident.
#[allow(clippy::nonminimal_bool, clippy::neg_cmp_op_on_partial_ord)]
pub mod vegetation_patch;
pub mod vines;
