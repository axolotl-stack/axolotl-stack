//! Opt-in registry coverage of compared generation (`worldgen-coverage`).
//!
//! Generation code marks what it is running with [`scope`] (placed and configured
//! features, structures, templates, processor lists, carvers) and reports block
//! writes with [`record_write`]. Every scope open at a write into one of the
//! [`set_targets`] chunks counts as exercised. Surface-rule leaves report through
//! [`surface_rule`] while [`chunk_scope`] names the chunk being surfaced. Only
//! writes into compared chunks count, so a feature that ran in a neighbour but
//! changed nothing the comparison saw is not reported as covered.
//!
//! Without the feature every function is an empty inline function and every
//! scope is zero-sized, so ordinary builds carry no state and no work.

/// Registry a coverage entry belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    PlacedFeature,
    ConfiguredFeature,
    Structure,
    Template,
    ProcessorList,
    Carver,
    SurfaceRule,
}

impl Kind {
    /// Stable report key.
    pub fn key(self) -> &'static str {
        match self {
            Self::PlacedFeature => "placed_feature",
            Self::ConfiguredFeature => "configured_feature",
            Self::Structure => "structure",
            Self::Template => "template",
            Self::ProcessorList => "processor_list",
            Self::Carver => "carver",
            Self::SurfaceRule => "surface_rule",
        }
    }
}

#[cfg(feature = "worldgen-coverage")]
mod imp {
    use super::Kind;
    use std::cell::{Cell, RefCell};
    use std::collections::{BTreeSet, HashSet};
    use std::sync::{LazyLock, Mutex};

    #[derive(Default)]
    struct Hits {
        targets: HashSet<(i32, i32)>,
        hits: BTreeSet<(Kind, &'static str)>,
    }

    static HITS: LazyLock<Mutex<Hits>> = LazyLock::new(Default::default);

    thread_local! {
        static STACK: RefCell<Vec<(Kind, &'static str)>> = const { RefCell::new(Vec::new()) };
        static CHUNK: Cell<Option<(i32, i32)>> = const { Cell::new(None) };
    }

    /// Open coverage scope; closes when dropped.
    #[must_use]
    pub struct Scope(());

    impl Drop for Scope {
        fn drop(&mut self) {
            STACK.with(|stack| stack.borrow_mut().pop());
        }
    }

    pub fn scope(kind: Kind, name: &'static str) -> Scope {
        STACK.with(|stack| stack.borrow_mut().push((kind, name)));
        Scope(())
    }

    /// Chunk being surfaced or carved; closes when dropped.
    #[must_use]
    pub struct ChunkScope(Option<(i32, i32)>);

    impl Drop for ChunkScope {
        fn drop(&mut self) {
            CHUNK.with(|chunk| chunk.set(self.0));
        }
    }

    pub fn chunk_scope(chunk_x: i32, chunk_z: i32) -> ChunkScope {
        ChunkScope(CHUNK.with(|chunk| chunk.replace(Some((chunk_x, chunk_z)))))
    }

    pub fn record_write(chunk_x: i32, chunk_z: i32) {
        let mut hits = HITS.lock().expect("coverage state poisoned");
        if !hits.targets.contains(&(chunk_x, chunk_z)) {
            return;
        }
        STACK.with(|stack| hits.hits.extend(stack.borrow().iter().copied()));
    }

    pub fn surface_rule(leaf: &'static str) {
        let Some(chunk) = CHUNK.with(Cell::get) else {
            panic!("surface rule {leaf} applied outside a coverage chunk scope");
        };
        let mut hits = HITS.lock().expect("coverage state poisoned");
        if hits.targets.contains(&chunk) {
            hits.hits.insert((Kind::SurfaceRule, leaf));
        }
    }

    pub fn set_targets(chunks: impl IntoIterator<Item = (i32, i32)>) {
        let mut hits = HITS.lock().expect("coverage state poisoned");
        hits.targets = chunks.into_iter().collect();
        hits.hits.clear();
    }

    pub fn take_hits() -> Vec<(Kind, &'static str)> {
        let mut hits = HITS.lock().expect("coverage state poisoned");
        std::mem::take(&mut hits.hits).into_iter().collect()
    }
}

#[cfg(not(feature = "worldgen-coverage"))]
mod imp {
    use super::Kind;

    pub struct Scope;

    #[inline(always)]
    pub fn scope(_: Kind, _: &'static str) -> Scope {
        Scope
    }

    pub struct ChunkScope;

    #[inline(always)]
    pub fn chunk_scope(_: i32, _: i32) -> ChunkScope {
        ChunkScope
    }

    #[inline(always)]
    pub fn record_write(_: i32, _: i32) {}

    #[inline(always)]
    pub fn surface_rule(_: &'static str) {}
}

pub use imp::*;

/// Whether this build records coverage.
pub const ENABLED: bool = cfg!(feature = "worldgen-coverage");
