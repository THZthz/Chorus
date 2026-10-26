//! The `--profile` timing and memory face: a tracing layer that accumulates
//! wall time and memory allocation per span name and prints a sorted table
//! to stderr when the run ends. Peak heap and OS peak resident set size (RSS)
//! are tracked globally.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::Registry;

/// A global tracking allocator that records cumulative allocations,
/// deallocations, peak heap usage, and allocation call counts.
pub struct TrackingAllocator {
    alloc_bytes: AtomicUsize,
    dealloc_bytes: AtomicUsize,
    alloc_count: AtomicUsize,
    peak_bytes: AtomicUsize,
}

impl TrackingAllocator {
    pub const fn new() -> Self {
        Self {
            alloc_bytes: AtomicUsize::new(0),
            dealloc_bytes: AtomicUsize::new(0),
            alloc_count: AtomicUsize::new(0),
            peak_bytes: AtomicUsize::new(0),
        }
    }

    #[inline]
    pub fn total_allocated(&self) -> usize {
        self.alloc_bytes.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn total_deallocated(&self) -> usize {
        self.dealloc_bytes.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn current_allocated(&self) -> usize {
        self.total_allocated().saturating_sub(self.total_deallocated())
    }

    #[inline]
    pub fn peak_allocated(&self) -> usize {
        self.peak_bytes.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn allocation_count(&self) -> usize {
        self.alloc_count.load(Ordering::Relaxed)
    }
}

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            let size = layout.size();
            let prev_alloc = self.alloc_bytes.fetch_add(size, Ordering::Relaxed);
            self.alloc_count.fetch_add(1, Ordering::Relaxed);
            let prev_dealloc = self.dealloc_bytes.load(Ordering::Relaxed);
            let current = (prev_alloc + size).saturating_sub(prev_dealloc);
            self.peak_bytes.fetch_max(current, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        self.dealloc_bytes.fetch_add(layout.size(), Ordering::Relaxed);
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc_zeroed(layout);
        if !ptr.is_null() {
            let size = layout.size();
            let prev_alloc = self.alloc_bytes.fetch_add(size, Ordering::Relaxed);
            self.alloc_count.fetch_add(1, Ordering::Relaxed);
            let prev_dealloc = self.dealloc_bytes.load(Ordering::Relaxed);
            let current = (prev_alloc + size).saturating_sub(prev_dealloc);
            self.peak_bytes.fetch_max(current, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = System.realloc(ptr, layout, new_size);
        if !new_ptr.is_null() {
            self.alloc_count.fetch_add(1, Ordering::Relaxed);
            let old_size = layout.size();
            if new_size > old_size {
                let diff = new_size - old_size;
                let prev_alloc = self.alloc_bytes.fetch_add(diff, Ordering::Relaxed);
                let prev_dealloc = self.dealloc_bytes.load(Ordering::Relaxed);
                let current = (prev_alloc + diff).saturating_sub(prev_dealloc);
                self.peak_bytes.fetch_max(current, Ordering::Relaxed);
            } else {
                let diff = old_size - new_size;
                self.dealloc_bytes.fetch_add(diff, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

#[global_allocator]
pub static ALLOCATOR: TrackingAllocator = TrackingAllocator::new();

/// OS Peak Resident Set Size (RSS).
#[cfg(unix)]
pub fn get_peak_rss() -> usize {
    #[repr(C)]
    struct RUsage {
        ru_utime: [u64; 2],
        ru_stime: [u64; 2],
        ru_maxrss: i64,
        _rest: [i64; 14],
    }
    extern "C" {
        fn getrusage(who: i32, usage: *mut RUsage) -> i32;
    }
    let mut usage = std::mem::MaybeUninit::<RUsage>::uninit();
    let res = unsafe { getrusage(0, usage.as_mut_ptr()) };
    if res == 0 {
        let usage = unsafe { usage.assume_init() };
        #[cfg(target_os = "macos")]
        {
            usage.ru_maxrss as usize
        }
        #[cfg(not(target_os = "macos"))]
        {
            (usage.ru_maxrss as usize) * 1024
        }
    } else {
        0
    }
}

#[cfg(not(unix))]
pub fn get_peak_rss() -> usize {
    0
}

/// One span name's accumulated wall time and allocation metrics.
#[derive(Default, Clone)]
struct Acc {
    calls: u64,
    total: Duration,
    alloc_bytes: usize,
    alloc_count: usize,
    net_bytes: i64,
}

#[derive(Clone)]
struct SpanEnter {
    name: String,
    instant: Instant,
    total_alloc: usize,
    current_alloc: usize,
    alloc_count: usize,
}

#[derive(Default)]
struct State {
    /// Open spans by id: display name and entrance measurements.
    open: HashMap<tracing::span::Id, SpanEnter>,
    acc: BTreeMap<String, Acc>,
}

/// The timing and memory profiling layer.
#[derive(Clone)]
pub struct TimingLayer {
    state: Arc<Mutex<State>>,
}

static LAYER: OnceLock<TimingLayer> = OnceLock::new();

/// Install the layer as the global subscriber; a no-op when already
/// installed. Call `report` at the end of the run to print the table.
pub fn install() {
    let layer = LAYER.get_or_init(|| TimingLayer {
        state: Arc::new(Mutex::new(State::default())),
    });
    let _ = tracing::subscriber::set_global_default(Registry::default().with(layer.clone()));
}

impl tracing_subscriber::Layer<Registry> for TimingLayer {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        _ctx: Context<'_, Registry>,
    ) {
        let name = attrs.metadata().name().to_string();
        let enter = SpanEnter {
            name,
            instant: Instant::now(),
            total_alloc: ALLOCATOR.total_allocated(),
            current_alloc: ALLOCATOR.current_allocated(),
            alloc_count: ALLOCATOR.allocation_count(),
        };
        self.state.lock().unwrap().open.insert(id.clone(), enter);
    }

    fn on_enter(&self, id: &tracing::span::Id, _ctx: Context<'_, Registry>) {
        // Re-enters restart the clock and memory baselines.
        if let Some(entry) = self.state.lock().unwrap().open.get_mut(id) {
            entry.instant = Instant::now();
            entry.total_alloc = ALLOCATOR.total_allocated();
            entry.current_alloc = ALLOCATOR.current_allocated();
            entry.alloc_count = ALLOCATOR.allocation_count();
        }
    }

    fn on_exit(&self, id: &tracing::span::Id, _ctx: Context<'_, Registry>) {
        let now = Instant::now();
        let total_alloc = ALLOCATOR.total_allocated();
        let current_alloc = ALLOCATOR.current_allocated();
        let alloc_count = ALLOCATOR.allocation_count();

        let mut state = self.state.lock().unwrap();
        if let Some(entry) = state.open.get(id) {
            let elapsed = now.saturating_duration_since(entry.instant);
            let bytes_alloc = total_alloc.saturating_sub(entry.total_alloc);
            let count_alloc = alloc_count.saturating_sub(entry.alloc_count);
            let net = current_alloc as i64 - entry.current_alloc as i64;
            let name = entry.name.clone();

            let acc = state.acc.entry(name).or_default();
            acc.calls += 1;
            acc.total += elapsed;
            acc.alloc_bytes += bytes_alloc;
            acc.alloc_count += count_alloc;
            acc.net_bytes += net;
        }
    }

    fn on_close(&self, id: tracing::span::Id, _ctx: Context<'_, Registry>) {
        self.state.lock().unwrap().open.remove(&id);
    }
}

fn format_bytes(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn format_signed_bytes(bytes: i64) -> String {
    let sign = if bytes < 0 { "-" } else { "+" };
    let abs = bytes.unsigned_abs() as usize;
    if abs == 0 {
        "0 B".to_string()
    } else if abs < 1024 {
        format!("{sign}{abs} B")
    } else if abs < 1024 * 1024 {
        format!("{sign}{:.1} KB", abs as f64 / 1024.0)
    } else if abs < 1024 * 1024 * 1024 {
        format!("{sign}{:.1} MB", abs as f64 / (1024.0 * 1024.0))
    } else {
        format!("{sign}{:.2} GB", abs as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Print the accumulated span times and memory stats to stderr, sorted by total descending.
pub fn report() {
    let Some(layer) = LAYER.get() else { return };
    let state = layer.state.lock().unwrap();
    let root = state.acc.get("analyze").map_or(Duration::ZERO, |a| a.total);
    if root.is_zero() {
        return;
    }
    let mut rows: Vec<(&String, &Acc)> = state.acc.iter().collect();
    rows.sort_by_key(|(_, acc)| std::cmp::Reverse(acc.total));

    let peak_heap = ALLOCATOR.peak_allocated();
    let total_heap = ALLOCATOR.total_allocated();
    let total_alloc_count = ALLOCATOR.allocation_count();
    let peak_rss = get_peak_rss();

    eprintln!(
        "profile — analyze wall {}ms | peak heap {} | peak rss {} | total alloc {} ({} allocs)",
        root.as_millis(),
        format_bytes(peak_heap),
        format_bytes(peak_rss),
        format_bytes(total_heap),
        total_alloc_count,
    );
    eprintln!(
        "profile — {:>9}  {:>5}  {:>4}  {:>10}  {:>10}  {:>8}  span",
        "total", "calls", "%", "allocated", "retained", "allocs"
    );
    for (name, acc) in rows {
        if name == "analyze" {
            continue;
        }
        eprintln!(
            "profile — {:>8}ms  {:>5}  {:>3.0}%  {:>10}  {:>10}  {:>8}  {name}",
            acc.total.as_millis(),
            acc.calls,
            acc.total.as_secs_f64() / root.as_secs_f64() * 100.0,
            format_bytes(acc.alloc_bytes),
            format_signed_bytes(acc.net_bytes),
            acc.alloc_count,
        );
    }
}
