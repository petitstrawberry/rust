use crate::alloc::{GlobalAlloc, Layout, System};
use crate::ptr;

#[stable(feature = "alloc_system_type", since = "1.28.0")]
unsafe impl GlobalAlloc for System {
    #[inline]
    unsafe fn alloc(&self, _layout: Layout) -> *mut u8 {
        ptr::null_mut()
    }

    #[inline]
    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}
