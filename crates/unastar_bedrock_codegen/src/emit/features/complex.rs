//! Block-placing feature types whose algorithm lives in the runtime
//! (`unastar::world::generator::bedrock::features::<type>`). Each emitter
//! turns the strict JSON into a parameter literal and a `place` call; the
//! expression it returns evaluates to `Option<BlockPos>` with `ctx` and `pos`
//! in scope.

use super::super::Emitter;
use crate::error::{Error, Result};
use crate::ir::Obj;
use proc_macro2::TokenStream;

pub fn emit(em: &mut Emitter<'_>, kind: &str, o: &Obj<'_>) -> Result<TokenStream> {
    let _ = em;
    Err(Error::data(o.origin, format!("feature type {kind} has no emitter")))
}
