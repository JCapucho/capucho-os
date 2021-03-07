pub use frame_allocator::GlobalFrameAllocator;

use bootloader::bootinfo::MemoryRegionType;
use spin::{Mutex, Once};
use x86_64::{
    structures::paging::{
        mapper::{MapToError, UnmapError},
        FrameAllocator, FrameDeallocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags,
        PhysFrame, Size4KiB,
    },
    VirtAddr,
};

mod frame_allocator;

pub struct PagingContext {
    pub mapper: OffsetPageTable<'static>,
    pub allocator: GlobalFrameAllocator<'static>,
}

pub static PAGING_CTX: Once<Mutex<PagingContext>> = Once::new();

/// Initialize a new OffsetPageTable.
///
/// # Safety
///
/// This function is unsafe because the caller must guarantee that the
/// complete physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once
/// to avoid aliasing `&mut` references (which is undefined behavior).
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = active_level_4_table(physical_memory_offset);
    OffsetPageTable::new(level_4_table, physical_memory_offset)
}

/// Returns a mutable reference to the active level 4 table.
///
/// # Safety
///
/// This function is unsafe because the caller must guarantee that the
/// complete physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once
/// to avoid aliasing `&mut` references (which is undefined behavior).
unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    use x86_64::registers::control::Cr3;

    let (level_4_table_frame, _) = Cr3::read();

    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    &mut *page_table_ptr // unsafe
}

/// Identity maps a frame for a memory mapped device
///
/// # Safety
///
/// This function is unsafe because the caller must guarantee that the
/// frame is free and is usable
#[track_caller]
pub unsafe fn mmap_dev(frame: PhysFrame, acpi: bool) -> Result<UnmapGuard, MapToError<Size4KiB>> {
    let ctx = &mut *PAGING_CTX.get().unwrap().lock();
    let ty = ctx
        .allocator
        .get_frame_ty(frame)
        .ok_or(MapToError::FrameAllocationFailed)?;

    let extra_flags = match ty {
        MemoryRegionType::Reserved | MemoryRegionType::FrameZero => PageTableFlags::WRITABLE,
        // Workaround acpi bios discovery
        MemoryRegionType::KernelStack if acpi => PageTableFlags::empty(),
        _ => panic!(
            "Tried to mmap a device on a {:?} frame {:#X}",
            ty,
            frame.start_address()
        ),
    };

    let page = Page::containing_address(VirtAddr::new(frame.start_address().as_u64()));

    let flusher = ctx.mapper.identity_map(
        frame,
        PageTableFlags::PRESENT
            | PageTableFlags::NO_CACHE
            | PageTableFlags::WRITE_THROUGH
            | extra_flags,
        &mut ctx.allocator,
    )?;

    flusher.flush();

    Ok(UnmapGuard {
        page,
        unmap_frame: !ctx.allocator.frame_in_use(frame),
    })
}

/// Unmaps and if a guard is provided deallocates the frame
pub fn unmap(guard: UnmapGuard) -> Result<(), UnmapError> {
    let mut ctx = PAGING_CTX.get().unwrap().lock();
    let (frame, flusher) = ctx.mapper.unmap(guard.page)?;

    flusher.flush();

    if guard.unmap_frame {
        unsafe { ctx.allocator.deallocate_frame(frame) }
    }

    Ok(())
}

/// Maps a page range
#[track_caller]
pub fn map_range(
    range: impl Iterator<Item = Page>,
    flags: PageTableFlags,
) -> Result<(), MapToError<Size4KiB>> {
    let ctx = &mut *PAGING_CTX.get().unwrap().lock();

    for page in range {
        let frame = ctx
            .allocator
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;

        unsafe {
            ctx.mapper
                .map_to(page, frame, flags, &mut ctx.allocator)?
                .flush()
        };
    }

    Ok(())
}

pub struct UnmapGuard {
    page: Page<Size4KiB>,
    unmap_frame: bool,
}
