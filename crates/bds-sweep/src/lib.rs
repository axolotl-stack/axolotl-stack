//! The scripted sweep client's building blocks, shared by its binaries: the
//! movement simulation ([`physics`]), the blocks BDS sends ([`world`]) and
//! the movement scenarios of the three-way movement harness
//! (`tools/movement-diff`, [`scenario`]).

pub mod capture;
pub mod json;
pub mod physics;
pub mod random;
pub mod scenario;
pub mod validation;
pub mod world;
