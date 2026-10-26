//! Opt-in, thread-local worldgen measurements. Disabled builds compile scopes away.
//!
//! Times and allocation counters are exclusive: a nested stage suspends its parent.
//! The benchmark's global allocator calls [`allocation`]; production uses its normal
//! allocator. Snapshot collection never allocates, including inside allocator callbacks.

/// Stable stage identities used by the benchmark's machine-readable ratchet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Stage {
    Pipeline,
    Noise,
    AquiferVeins,
    Biomes,
    Surface,
    Carvers,
    Packing,
    Region,
    StructureStarts,
    StructureReferences,
    StructurePieces,
    StructureTemplates,
    Decoration0,
    Decoration1,
    Decoration2,
    Decoration3,
    Decoration4,
    Decoration5,
    Decoration6,
    Decoration7,
    Decoration8,
    Decoration9,
    Decoration10,
    BedrockFirst,
    BedrockBeforeUnderground,
    BedrockUnderground,
    BedrockAfterUnderground,
    BedrockBeforeSurface,
    BedrockSurface,
    BedrockAfterSurface,
    BedrockFinal,
}

impl Stage {
    pub const ALL: [Self; 31] = [
        Self::Pipeline,
        Self::Noise,
        Self::AquiferVeins,
        Self::Biomes,
        Self::Surface,
        Self::Carvers,
        Self::Packing,
        Self::Region,
        Self::StructureStarts,
        Self::StructureReferences,
        Self::StructurePieces,
        Self::StructureTemplates,
        Self::Decoration0,
        Self::Decoration1,
        Self::Decoration2,
        Self::Decoration3,
        Self::Decoration4,
        Self::Decoration5,
        Self::Decoration6,
        Self::Decoration7,
        Self::Decoration8,
        Self::Decoration9,
        Self::Decoration10,
        Self::BedrockFirst,
        Self::BedrockBeforeUnderground,
        Self::BedrockUnderground,
        Self::BedrockAfterUnderground,
        Self::BedrockBeforeSurface,
        Self::BedrockSurface,
        Self::BedrockAfterSurface,
        Self::BedrockFinal,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Pipeline => "pipeline",
            Self::Noise => "noise",
            Self::AquiferVeins => "aquifer_veins",
            Self::Biomes => "biomes",
            Self::Surface => "surface",
            Self::Carvers => "carvers",
            Self::Packing => "packing",
            Self::Region => "region",
            Self::StructureStarts => "structure_starts",
            Self::StructureReferences => "structure_references",
            Self::StructurePieces => "structure_pieces",
            Self::StructureTemplates => "structure_templates",
            Self::Decoration0 => "decoration_0_raw_generation",
            Self::Decoration1 => "decoration_1_lakes",
            Self::Decoration2 => "decoration_2_local_modifications",
            Self::Decoration3 => "decoration_3_underground_structures",
            Self::Decoration4 => "decoration_4_surface_structures",
            Self::Decoration5 => "decoration_5_strongholds",
            Self::Decoration6 => "decoration_6_underground_ores",
            Self::Decoration7 => "decoration_7_underground_decoration",
            Self::Decoration8 => "decoration_8_fluid_springs",
            Self::Decoration9 => "decoration_9_vegetal_decoration",
            Self::Decoration10 => "decoration_10_top_layer_modification",
            Self::BedrockFirst => "bedrock_first",
            Self::BedrockBeforeUnderground => "bedrock_before_underground",
            Self::BedrockUnderground => "bedrock_underground",
            Self::BedrockAfterUnderground => "bedrock_after_underground",
            Self::BedrockBeforeSurface => "bedrock_before_surface",
            Self::BedrockSurface => "bedrock_surface",
            Self::BedrockAfterSurface => "bedrock_after_surface",
            Self::BedrockFinal => "bedrock_final",
        }
    }

    pub fn decoration(step: usize) -> Self {
        Self::ALL[Self::Decoration0 as usize + step]
    }
}

/// Allocation requests (including reallocations), requested bytes, and exclusive time.
#[derive(Clone, Copy, Debug, Default)]
pub struct Counters {
    pub calls: u64,
    pub allocations: u64,
    pub bytes: u64,
    pub elapsed: std::time::Duration,
}

pub const ENABLED: bool = cfg!(feature = "worldgen-profile");

#[cfg(feature = "worldgen-profile")]
mod enabled {
    use super::{Counters, Stage};
    use std::cell::RefCell;
    use std::marker::PhantomData;
    use std::rc::Rc;
    use std::time::Instant;

    struct State {
        counters: [Counters; Stage::ALL.len()],
        current: Option<Stage>,
        since: Option<Instant>,
    }

    impl State {
        const fn new() -> Self {
            Self {
                counters: [Counters {
                    calls: 0,
                    allocations: 0,
                    bytes: 0,
                    elapsed: std::time::Duration::ZERO,
                }; Stage::ALL.len()],
                current: None,
                since: None,
            }
        }

        fn switch(&mut self, stage: Option<Stage>) -> Option<Stage> {
            let now = Instant::now();
            if let (Some(current), Some(since)) = (self.current, self.since) {
                self.counters[current as usize].elapsed += now.duration_since(since);
            }
            self.since = Some(now);
            std::mem::replace(&mut self.current, stage)
        }
    }

    thread_local! { static STATE: RefCell<State> = const { RefCell::new(State::new()) }; }

    /// Thread-bound RAII scope. Nested scopes must be dropped in stack order.
    pub struct Scope {
        previous: Option<Stage>,
        _thread_bound: PhantomData<Rc<()>>,
    }

    impl Drop for Scope {
        fn drop(&mut self) {
            STATE.with(|state| {
                state.borrow_mut().switch(self.previous);
            });
        }
    }

    #[inline]
    pub fn scope(stage: Stage) -> Scope {
        let previous = STATE.with(|state| {
            let mut state = state.borrow_mut();
            state.counters[stage as usize].calls += 1;
            state.switch(Some(stage))
        });
        Scope {
            previous,
            _thread_bound: PhantomData,
        }
    }

    /// Attribute one successful allocation request to the current stage.
    #[inline]
    pub fn allocation(bytes: usize) {
        // `try_with` permits allocator calls during thread-local destruction.
        let _ = STATE.try_with(|state| {
            let mut state = state.borrow_mut();
            if let Some(stage) = state.current {
                let counters = &mut state.counters[stage as usize];
                counters.allocations += 1;
                counters.bytes += bytes as u64;
            }
        });
    }

    /// Reset outside a profiling scope, before running a cold workload.
    pub fn reset() {
        STATE.with(|state| {
            assert!(
                state.borrow().current.is_none(),
                "reset inside profiling scope"
            );
            *state.borrow_mut() = State::new();
        });
    }

    pub fn snapshot() -> [Counters; Stage::ALL.len()] {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let current = state.current;
            state.switch(current);
            state.counters
        })
    }
}

#[cfg(feature = "worldgen-profile")]
pub use enabled::*;

#[cfg(not(feature = "worldgen-profile"))]
mod disabled {
    use super::{Counters, Stage};
    pub struct Scope;
    #[inline(always)]
    pub fn scope(_: Stage) -> Scope {
        Scope
    }
    #[inline(always)]
    pub fn allocation(_: usize) {}
    pub fn reset() {}
    pub fn snapshot() -> [Counters; Stage::ALL.len()] {
        [Counters::default(); Stage::ALL.len()]
    }
}

#[cfg(not(feature = "worldgen-profile"))]
pub use disabled::*;

#[cfg(all(test, feature = "worldgen-profile"))]
mod tests {
    use super::*;

    #[test]
    fn nested_allocations_are_exclusive_and_thread_local() {
        reset();
        {
            let _outer = scope(Stage::Noise);
            allocation(17);
            {
                let _inner = scope(Stage::AquiferVeins);
                allocation(29);
            }
            std::thread::spawn(|| {
                let _other = scope(Stage::Noise);
                allocation(1000);
            })
            .join()
            .unwrap();
            allocation(43);
        }
        let counts = snapshot();
        assert_eq!(counts[Stage::Noise as usize].allocations, 2);
        assert_eq!(counts[Stage::Noise as usize].bytes, 60);
        assert_eq!(counts[Stage::AquiferVeins as usize].allocations, 1);
        assert_eq!(counts[Stage::AquiferVeins as usize].bytes, 29);
    }
}
