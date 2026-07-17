#[cfg(feature = "allocation-counting")]
mod enabled {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

    pub struct CountingAllocator;
    static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
    static BYTES: AtomicU64 = AtomicU64::new(0);

    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCATIONS.fetch_add(1, Relaxed); BYTES.fetch_add(layout.size() as u64, Relaxed);
            unsafe { System.alloc(layout) }
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            ALLOCATIONS.fetch_add(1, Relaxed); BYTES.fetch_add(layout.size() as u64, Relaxed);
            unsafe { System.alloc_zeroed(layout) }
        }
        unsafe fn realloc(&self, ptr: *mut u8, old: Layout, new_size: usize) -> *mut u8 {
            ALLOCATIONS.fetch_add(1, Relaxed); BYTES.fetch_add(new_size as u64, Relaxed);
            unsafe { System.realloc(ptr, old, new_size) }
        }
        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { unsafe { System.dealloc(ptr, layout) } }
    }

    #[global_allocator]
    static GLOBAL: CountingAllocator = CountingAllocator;
    pub fn reset() { ALLOCATIONS.store(0, Relaxed); BYTES.store(0, Relaxed); }
    pub fn snapshot() -> Option<(u64,u64)> { Some((ALLOCATIONS.load(Relaxed), BYTES.load(Relaxed))) }
}

#[cfg(feature = "allocation-counting")]
pub use enabled::{reset, snapshot};
#[cfg(not(feature = "allocation-counting"))]
pub fn reset() {}
#[cfg(not(feature = "allocation-counting"))]
pub fn snapshot() -> Option<(u64,u64)> { None }
