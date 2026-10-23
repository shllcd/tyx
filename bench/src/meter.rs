use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::time::{Duration, Instant};

pub struct Counting;

static ALLOCS: AtomicU64 = AtomicU64::new(0);
static ALLOC_BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn grow(size: usize) {
    let live = LIVE.fetch_add(size, Relaxed) + size;
    PEAK.fetch_max(live, Relaxed);
}

// SAFETY: every call forwards to `System` with the caller's layout unchanged; only counters are added.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        ALLOC_BYTES.fetch_add(layout.size() as u64, Relaxed);
        grow(layout.size());
        // SAFETY: same contract as `GlobalAlloc::alloc`.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        ALLOC_BYTES.fetch_add(layout.size() as u64, Relaxed);
        grow(layout.size());
        // SAFETY: same contract as `GlobalAlloc::alloc_zeroed`.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Relaxed);
        // SAFETY: same contract as `GlobalAlloc::dealloc`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCS.fetch_add(1, Relaxed);
        ALLOC_BYTES.fetch_add(new_size as u64, Relaxed);
        LIVE.fetch_sub(layout.size(), Relaxed);
        grow(new_size);
        // SAFETY: same contract as `GlobalAlloc::realloc`.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[derive(Clone, Copy, Default)]
pub struct Op {
    pub ns: f64,
    pub allocs: u64,
    pub alloc_bytes: u64,
}

const SAMPLES: usize = 7;
const SAMPLE_TIME: Duration = Duration::from_millis(12);

pub fn ns_per_op<R>(mut f: impl FnMut() -> R) -> f64 {
    let mut iters: u64 = 1;
    loop {
        let start = Instant::now();
        for _ in 0..iters {
            black_box(f());
        }
        let elapsed = start.elapsed();
        if elapsed >= SAMPLE_TIME / 4 {
            iters = (iters as f64 * SAMPLE_TIME.as_secs_f64() / elapsed.as_secs_f64()).ceil() as u64;
            break;
        }
        iters *= 2;
    }
    let mut samples: Vec<f64> = (0..SAMPLES)
        .map(|_| {
            let start = Instant::now();
            for _ in 0..iters {
                black_box(f());
            }
            start.elapsed().as_nanos() as f64 / iters as f64
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    samples[SAMPLES / 2]
}

pub fn allocations<R>(f: impl FnOnce() -> R) -> (u64, u64, R) {
    let allocs = ALLOCS.load(Relaxed);
    let bytes = ALLOC_BYTES.load(Relaxed);
    let result = black_box(f());
    (ALLOCS.load(Relaxed) - allocs, ALLOC_BYTES.load(Relaxed) - bytes, result)
}

pub fn op<R>(mut f: impl FnMut() -> R) -> Op {
    let ns = ns_per_op(&mut f);
    let (allocs, alloc_bytes, _) = allocations(&mut f);
    Op { ns, allocs, alloc_bytes }
}

pub fn peak_heap_above_baseline<R>(f: impl FnOnce() -> R) -> usize {
    let baseline = LIVE.load(Relaxed);
    PEAK.store(baseline, Relaxed);
    let result = black_box(f());
    let peak = PEAK.load(Relaxed);
    drop(result);
    peak - baseline
}
