pub mod batch;
pub mod codec;
pub mod join;
pub mod micro;
pub mod nethernet;
pub mod stream;
pub mod transport;

use serde::Serialize;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    time::Instant,
};

pub struct CountingAllocator;
static ALLOCS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static COUNTING: AtomicBool = AtomicBool::new(false);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCS.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(size as u64, Ordering::Relaxed);
        }
        unsafe { System.realloc(ptr, layout, size) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[derive(Serialize)]
pub struct Row<'a> {
    pub area: &'a str,
    pub name: &'a str,
    pub operation: &'a str,
    pub implementation: &'a str,
    pub ns_per_op: f64,
    pub allocs_per_op: Option<f64>,
    pub bytes_per_op: Option<f64>,
    pub payload_bytes: usize,
    pub msgs_per_sec: f64,
    pub mb_per_sec: f64,
    pub p50_us: Option<f64>,
    pub p99_us: Option<f64>,
    pub max_us: Option<f64>,
}

impl Row<'_> {
    pub fn emit(&self) -> anyhow::Result<()> {
        println!("{}", serde_json::to_string(self)?);
        Ok(())
    }
}

pub fn measure(
    area: &str,
    name: &str,
    operation: &str,
    implementation: &str,
    payload_bytes: usize,
    iterations: usize,
    mut f: impl FnMut(),
) -> anyhow::Result<()> {
    anyhow::ensure!(iterations > 0, "iterations must be positive");
    for _ in 0..32 {
        f();
    }
    let started = Instant::now();
    for _ in 0..iterations {
        f();
    }
    let elapsed = started.elapsed().as_secs_f64();
    let allocs = ALLOCS.load(Ordering::Relaxed);
    let bytes = BYTES.load(Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    for _ in 0..iterations {
        f();
    }
    COUNTING.store(false, Ordering::Relaxed);
    let allocs = ALLOCS.load(Ordering::Relaxed) - allocs;
    let bytes = BYTES.load(Ordering::Relaxed) - bytes;
    let ns_per_op = elapsed * 1e9 / iterations as f64;
    Row {
        area,
        name,
        operation,
        implementation,
        ns_per_op,
        allocs_per_op: Some(allocs as f64 / iterations as f64),
        bytes_per_op: Some(bytes as f64 / iterations as f64),
        payload_bytes,
        msgs_per_sec: 1e9 / ns_per_op,
        mb_per_sec: payload_bytes as f64 * 1e3 / ns_per_op,
        p50_us: None,
        p99_us: None,
        max_us: None,
    }
    .emit()
}

/// Times an owned in-place operation without charging input preparation or verification.
pub fn measure_prepared<T, U>(
    name: &str,
    operation: &str,
    payload_bytes: usize,
    iterations: usize,
    mut prepare: impl FnMut() -> T,
    mut operation_fn: impl FnMut(T) -> U,
    mut check: impl FnMut(U),
) -> anyhow::Result<()> {
    anyhow::ensure!(iterations > 0, "iterations must be positive");
    for _ in 0..32 {
        check(operation_fn(prepare()));
    }
    let mut elapsed = std::time::Duration::ZERO;
    for _ in 0..iterations {
        let input = prepare();
        let start = Instant::now();
        let output = operation_fn(input);
        elapsed += start.elapsed();
        check(output);
    }
    let mut allocs = 0;
    let mut bytes = 0;
    for _ in 0..iterations {
        let input = prepare();
        let before_allocs = ALLOCS.load(Ordering::Relaxed);
        let before_bytes = BYTES.load(Ordering::Relaxed);
        COUNTING.store(true, Ordering::Relaxed);
        let output = operation_fn(input);
        COUNTING.store(false, Ordering::Relaxed);
        allocs += ALLOCS.load(Ordering::Relaxed) - before_allocs;
        bytes += BYTES.load(Ordering::Relaxed) - before_bytes;
        check(output);
    }
    let ns_per_op = elapsed.as_secs_f64() * 1e9 / iterations as f64;
    Row {
        area: "batch",
        name,
        operation,
        implementation: "rust-owned",
        ns_per_op,
        allocs_per_op: Some(allocs as f64 / iterations as f64),
        bytes_per_op: Some(bytes as f64 / iterations as f64),
        payload_bytes,
        msgs_per_sec: 1e9 / ns_per_op,
        mb_per_sec: payload_bytes as f64 * 1e3 / ns_per_op,
        p50_us: None,
        p99_us: None,
        max_us: None,
    }
    .emit()
}
