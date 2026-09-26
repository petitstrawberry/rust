//! Scarlet's system allocator. Size-indexed bins avoid walking every free
//! allocation, and the native futex mutex parks contending threads.

use crate::alloc::{GlobalAlloc, Layout, System};
use crate::cell::UnsafeCell;
use crate::ptr;
use crate::sys::pal::abi;
use crate::sys::sync::Mutex;

const PAGE_SIZE: usize = 4096;

struct Heap(UnsafeCell<dlmalloc::Dlmalloc<Scarlet>>);

// SAFETY: every access to the allocator is serialized by HEAP_LOCK.
unsafe impl Sync for Heap {}

static HEAP: Heap = Heap(UnsafeCell::new(dlmalloc::Dlmalloc::new_with_allocator(Scarlet)));
static HEAP_LOCK: Mutex = Mutex::new();

struct HeapGuard;

impl Drop for HeapGuard {
    fn drop(&mut self) {
        // SAFETY: the guard is created only after acquiring HEAP_LOCK.
        unsafe { HEAP_LOCK.unlock() };
    }
}

/// Keep the heap consistent across process cloning. The caller must drop the
/// guard in both parent and child before either can allocate or free memory.
pub(crate) fn lock_for_fork() -> impl Drop {
    lock_heap()
}

#[inline]
fn lock_heap() -> HeapGuard {
    HEAP_LOCK.lock();
    HeapGuard
}

#[stable(feature = "alloc_system_type", since = "1.28.0")]
// SAFETY: dlmalloc owns the mapped regions and satisfies GlobalAlloc's layout
// and lifetime requirements. The mutex serializes access without allocating.
unsafe impl GlobalAlloc for System {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _guard = lock_heap();
        // SAFETY: the lock grants exclusive access; the caller supplies a valid layout.
        unsafe { (*HEAP.0.get()).malloc(layout.size(), layout.align()) }
    }

    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let _guard = lock_heap();
        // SAFETY: the lock grants exclusive access; calloc also clears reused blocks.
        unsafe { (*HEAP.0.get()).calloc(layout.size(), layout.align()) }
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _guard = lock_heap();
        // SAFETY: the caller supplies a live allocation and its original layout.
        unsafe { (*HEAP.0.get()).free(ptr, layout.size(), layout.align()) };
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _guard = lock_heap();
        // SAFETY: the caller supplies a live allocation and valid new size.
        // dlmalloc preserves the original allocation if growth fails.
        unsafe { (*HEAP.0.get()).realloc(ptr, layout.size(), layout.align(), new_size) }
    }
}

struct Scarlet;

// SAFETY: anonymous mappings provide disjoint, writable, page-aligned memory.
// Only dlmalloc-owned mappings are returned to the kernel by free/free_part.
unsafe impl dlmalloc::Allocator for Scarlet {
    fn alloc(&self, size: usize) -> (*mut u8, usize, u32) {
        match abi::memory_map(
            0,
            0,
            size,
            abi::mmap::PROT_READ | abi::mmap::PROT_WRITE,
            abi::mmap::MAP_PRIVATE | abi::mmap::MAP_ANONYMOUS,
            0,
        ) {
            Ok(address) => (ptr::with_exposed_provenance_mut(address), size, 0),
            Err(()) => (ptr::null_mut(), 0, 0),
        }
    }

    fn remap(&self, _ptr: *mut u8, _old_size: usize, _new_size: usize, _can_move: bool) -> *mut u8 {
        // There is no native remap syscall. dlmalloc can allocate and copy.
        ptr::null_mut()
    }

    fn free_part(&self, ptr: *mut u8, old_size: usize, new_size: usize) -> bool {
        // SAFETY: dlmalloc passes the page-aligned tail of an owned mapping.
        let tail = unsafe { ptr.add(new_size) };
        abi::memory_unmap(tail.expose_provenance(), old_size - new_size).is_ok()
    }

    fn free(&self, ptr: *mut u8, size: usize) -> bool {
        abi::memory_unmap(ptr.expose_provenance(), size).is_ok()
    }

    fn can_release_part(&self, _flags: u32) -> bool {
        true
    }

    fn allocates_zeros(&self) -> bool {
        true
    }

    fn page_size(&self) -> usize {
        PAGE_SIZE
    }
}
