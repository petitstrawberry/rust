use crate::alloc::{GlobalAlloc, Layout, System};
use crate::ptr;
use crate::sys::pal::abi;

const PAGE_SIZE: usize = 4096;

#[stable(feature = "alloc_system_type", since = "1.28.0")]
// SAFETY: `System` delegates allocation and deallocation to Scarlet anonymous
// memory mappings. Returned pointers are page-aligned and valid for `layout`
// bytes when the kernel accepts the mapping.
unsafe impl GlobalAlloc for System {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if layout.align() > PAGE_SIZE {
            return ptr::null_mut();
        }

        let Some(length) = rounded_mapping_len(layout) else {
            return ptr::null_mut();
        };

        let prot = abi::mmap::PROT_READ | abi::mmap::PROT_WRITE;
        let flags = abi::mmap::MAP_PRIVATE | abi::mmap::MAP_ANONYMOUS;
        match abi::memory_map(0, 0, length, prot, flags, 0) {
            Ok(addr) => ptr::with_exposed_provenance_mut(addr),
            Err(()) => ptr::null_mut(),
        }
    }

    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Some(length) = rounded_mapping_len(layout) {
            let _ = abi::memory_unmap(ptr.addr(), length);
        }
    }

    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, old_layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: The `GlobalAlloc::realloc` contract requires `ptr` and
        // `old_layout` to describe a currently allocated block.
        unsafe { super::realloc_fallback(self, ptr, old_layout, new_size) }
    }
}

fn rounded_mapping_len(layout: Layout) -> Option<usize> {
    let size = layout.size().max(1);
    size.checked_add(PAGE_SIZE - 1).map(|len| len & !(PAGE_SIZE - 1))
}
