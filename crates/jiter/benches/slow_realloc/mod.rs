//! A global allocator whose `realloc` always allocates, copies and frees, never grows in place.

use std::alloc::{GlobalAlloc, Layout, System};

pub struct SlowRealloc;

unsafe impl GlobalAlloc for SlowRealloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller upholds the `GlobalAlloc::realloc` contract, so `ptr` is a live block
        // of `layout` and `new_size` is a valid size for `layout.align()`; the copy is bounded by
        // the smaller of the two sizes.
        unsafe {
            let new_layout = Layout::from_size_align_unchecked(new_size, layout.align());
            let new_ptr = System.alloc(new_layout);
            if !new_ptr.is_null() {
                std::ptr::copy_nonoverlapping(ptr, new_ptr, layout.size().min(new_size));
                System.dealloc(ptr, layout);
            }
            new_ptr
        }
    }
}
