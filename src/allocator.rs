use core::sync::atomic::{AtomicBool, Ordering};

use buddy_system_allocator::LockedHeap;
use x86_64::{
    structures::paging::{mapper::MapToError, Page, PageTableFlags, Size4KiB},
    VirtAddr,
};

use crate::memory;

pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 500 * 1024; // 500 KiB

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::new();

pub static INITIALIZED: AtomicBool = AtomicBool::new(false);

pub fn init_heap() -> Result<(), MapToError<Size4KiB>> {
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = heap_start + HEAP_SIZE - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        Page::range_inclusive(heap_start_page, heap_end_page)
    };

    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    memory::map_range(page_range, flags)?;

    unsafe {
        ALLOCATOR.lock().init(HEAP_START, HEAP_SIZE);
    }

    INITIALIZED.store(true, Ordering::SeqCst);

    Ok(())
}

pub fn stats() -> usize { ALLOCATOR.lock().stats_alloc_actual() }

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    if INITIALIZED.load(Ordering::Relaxed) {
        panic!("Allocation error: {:?}", layout)
    } else {
        panic!("Allocator not initialized")
    }
}
