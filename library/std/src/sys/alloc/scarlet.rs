use crate::alloc::{GlobalAlloc, Layout, System};
use crate::mem::{align_of, size_of};
use crate::ptr;
use crate::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use crate::sys::pal::abi;

const PAGE_SIZE: usize = 4096;
const MIN_EXTEND_SIZE: usize = 64 * 1024;
const HEADER_SIZE: usize = size_of::<Block>();
const BACK_PTR_SIZE: usize = size_of::<usize>();
const MIN_FREE_BLOCK_SIZE: usize = HEADER_SIZE + BACK_PTR_SIZE + 16;

#[repr(C)]
struct Block {
    size: usize,
    next: *mut Block,
}

static HEAP_LOCK: AtomicBool = AtomicBool::new(false);
static FREE_LIST: AtomicUsize = AtomicUsize::new(0);

struct HeapGuard;

impl Drop for HeapGuard {
    fn drop(&mut self) {
        HEAP_LOCK.store(false, Ordering::Release);
    }
}

#[stable(feature = "alloc_system_type", since = "1.28.0")]
// SAFETY: `System` manages process heap memory obtained from Scarlet `sbrk`.
// The heap lock serializes free-list metadata access, and returned pointers
// satisfy the requested `Layout` while the allocation remains live.
unsafe impl GlobalAlloc for System {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _guard = lock_heap();
        // SAFETY: the heap lock is held for the duration of allocator metadata
        // access.
        unsafe { alloc_locked(layout) }
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }

        let _guard = lock_heap();
        // SAFETY: `ptr` was returned by this allocator and has not yet been
        // deallocated by the caller.
        unsafe { dealloc_locked(ptr) };
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, old_layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: The `GlobalAlloc::realloc` contract requires `ptr` and
        // `old_layout` to describe a currently allocated block.
        unsafe { super::realloc_fallback(self, ptr, old_layout, new_size) }
    }
}

fn lock_heap() -> HeapGuard {
    while HEAP_LOCK
        .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
        .is_err()
    {
        core::hint::spin_loop();
    }
    HeapGuard
}

unsafe fn alloc_locked(layout: Layout) -> *mut u8 {
    let align = layout.align().max(align_of::<usize>());
    if !align.is_power_of_two() {
        return ptr::null_mut();
    }

    let size = layout.size().max(1);
    loop {
        // SAFETY: the heap lock is held.
        if let Some(ptr) = unsafe { try_alloc_from_free_list(size, align) } {
            return ptr;
        }

        if extend_heap(size, align).is_none() {
            return ptr::null_mut();
        }
    }
}

unsafe fn try_alloc_from_free_list(size: usize, align: usize) -> Option<*mut u8> {
    let mut prev: *mut Block = ptr::null_mut();
    let mut current = ptr::with_exposed_provenance_mut::<Block>(FREE_LIST.load(Ordering::Relaxed));

    while !current.is_null() {
        // SAFETY: `current` points to a block owned by the free list.
        let block_size = unsafe { (*current).size };
        let block_start = current.expose_provenance();

        if let Some((data_addr, used_size)) = placement(block_start, block_size, size, align) {
            // SAFETY: `current` is a valid free-list block.
            let next = unsafe { (*current).next };
            let remaining = block_size - used_size;

            if remaining >= MIN_FREE_BLOCK_SIZE {
                let next_block_addr = block_start + used_size;
                let next_block = ptr::with_exposed_provenance_mut::<Block>(next_block_addr);
                // SAFETY: the tail remains inside the original free block and
                // is large enough for allocator metadata.
                unsafe {
                    ptr::write(next_block, Block { size: remaining, next });
                    (*current).size = used_size;
                    (*current).next = ptr::null_mut();
                }
                set_next(prev, next_block);
            } else {
                // SAFETY: the whole current block becomes allocated.
                unsafe {
                    (*current).size = block_size;
                    (*current).next = ptr::null_mut();
                }
                set_next(prev, next);
            }

            let back_ptr = ptr::with_exposed_provenance_mut::<*mut Block>(
                data_addr - BACK_PTR_SIZE,
            );
            // SAFETY: `placement` reserved a word immediately before the user
            // pointer for this back-pointer.
            unsafe {
                ptr::write(back_ptr, current);
            }
            return Some(ptr::with_exposed_provenance_mut::<u8>(data_addr));
        }

        prev = current;
        // SAFETY: `current` is a valid free-list block.
        current = unsafe { (*current).next };
    }

    None
}

unsafe fn dealloc_locked(ptr: *mut u8) {
    let data_addr = ptr.expose_provenance();
    let Some(back_ptr_addr) = data_addr.checked_sub(BACK_PTR_SIZE) else {
        return;
    };
    let back_ptr = ptr::with_exposed_provenance::<*mut Block>(back_ptr_addr);
    // SAFETY: allocations store the owning block pointer in this word.
    let block = unsafe { ptr::read(back_ptr) };
    if block.is_null() {
        return;
    }
    // SAFETY: `block` is the allocation header for `ptr`.
    let size = unsafe { (*block).size };
    // SAFETY: the heap lock is held and `block` is returning to the free list.
    unsafe { insert_free_block(block, size) };
}

fn extend_heap(size: usize, align: usize) -> Option<()> {
    let metadata = HEADER_SIZE.checked_add(BACK_PTR_SIZE)?.checked_add(align)?;
    let needed = metadata.checked_add(size)?;
    let request = align_up(needed.max(MIN_EXTEND_SIZE), PAGE_SIZE)?;
    let raw = abi::sbrk(request).ok()?;
    let raw_end = raw.checked_add(request)?;
    let block_start = align_up(raw, align_of::<Block>())?;
    let block_size = raw_end.checked_sub(block_start)?;
    if block_size < MIN_FREE_BLOCK_SIZE {
        return None;
    }

    let block = ptr::with_exposed_provenance_mut::<Block>(block_start);
    // SAFETY: `sbrk` returned a new heap range owned by this process allocator.
    unsafe { insert_free_block(block, block_size) };
    Some(())
}

unsafe fn insert_free_block(block: *mut Block, size: usize) {
    // SAFETY: the caller guarantees `block` starts a heap range of `size` bytes.
    unsafe {
        (*block).size = size;
        (*block).next = ptr::null_mut();
    }

    let block_addr = block.expose_provenance();
    let mut prev: *mut Block = ptr::null_mut();
    let mut current = ptr::with_exposed_provenance_mut::<Block>(FREE_LIST.load(Ordering::Relaxed));

    while !current.is_null() && current.expose_provenance() < block_addr {
        prev = current;
        // SAFETY: `current` points to a free-list block.
        current = unsafe { (*current).next };
    }

    // SAFETY: all pointers here are free-list blocks protected by the heap lock.
    unsafe {
        (*block).next = current;
    }
    set_next(prev, block);

    // SAFETY: adjacent blocks in address order can be merged.
    unsafe {
        coalesce_with_next(block);
        if !prev.is_null() {
            coalesce_with_next(prev);
        }
    }
}

unsafe fn coalesce_with_next(block: *mut Block) {
    // SAFETY: `block` points to a free-list block.
    let next = unsafe { (*block).next };
    if next.is_null() {
        return;
    }

    let block_end = block.expose_provenance() + unsafe { (*block).size };
    if block_end == next.expose_provenance() {
        // SAFETY: adjacent free-list blocks can be represented as one block.
        unsafe {
            (*block).size += (*next).size;
            (*block).next = (*next).next;
        }
    }
}

fn set_next(prev: *mut Block, next: *mut Block) {
    if prev.is_null() {
        FREE_LIST.store(next.expose_provenance(), Ordering::Relaxed);
    } else {
        // SAFETY: the heap lock is held and `prev` is a valid free-list block.
        unsafe {
            (*prev).next = next;
        }
    }
}

fn placement(
    block_start: usize,
    block_size: usize,
    size: usize,
    align: usize,
) -> Option<(usize, usize)> {
    let block_end = block_start.checked_add(block_size)?;
    let data_min = block_start.checked_add(HEADER_SIZE)?.checked_add(BACK_PTR_SIZE)?;
    let data_addr = align_up(data_min, align)?;
    let alloc_end = data_addr.checked_add(size)?;
    if alloc_end > block_end {
        return None;
    }

    Some((data_addr, alloc_end - block_start))
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    Some(value.checked_add(align - 1)? & !(align - 1))
}
