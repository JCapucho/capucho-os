use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use spin::{Mutex, Once};
use x86_64::{
    structures::paging::{
        mapper::{MapToError, MapperFlush, UnmapError},
        FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame,
        Size4KiB,
    },
    PhysAddr, VirtAddr,
};

pub struct PagingContext {
    pub mapper: OffsetPageTable<'static>,
    pub allocator: BootInfoFrameAllocator,
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

/// Identity maps a frame
///
/// # Safety
///
/// This function is unsafe because the caller must guarantee that the
/// frame is free and is usable
#[track_caller]
pub unsafe fn identity_map(
    frame: PhysFrame,
    flags: PageTableFlags,
) -> Result<(), MapToError<Size4KiB>> {
    let ctx = &mut *PAGING_CTX.get().unwrap().lock();

    ctx.mapper
        .identity_map(frame, flags, &mut ctx.allocator)
        .map(|v| v.flush())
}

/// Identity unmaps a frame
pub fn identity_unmap(frame: PhysFrame) -> Result<(), UnmapError> {
    let mut ctx = PAGING_CTX.get().unwrap().lock();

    let page = Page::from_start_address(VirtAddr::new(frame.start_address().as_u64())).unwrap();
    ctx.mapper
        .unmap(page)
        .map(|v: (_, MapperFlush<Size4KiB>)| v.1.flush())
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

/// A FrameAllocator that returns usable frames from the bootloader's memory
/// map.
pub struct BootInfoFrameAllocator {
    memory_map: &'static MemoryMap,
    next: usize,
}

impl BootInfoFrameAllocator {
    /// Create a FrameAllocator from the passed memory map.
    ///
    /// # Safety
    ///
    /// This function is unsafe because the caller must guarantee that the
    /// passed memory map is valid. The main requirement is that all frames
    /// that are marked as `USABLE` in it are really unused.
    pub unsafe fn init(memory_map: &'static MemoryMap) -> Self {
        BootInfoFrameAllocator {
            memory_map,
            next: 0,
        }
    }

    /// Returns an iterator over the usable frames specified in the memory map.
    fn usable_frames(&self) -> impl Iterator<Item = PhysFrame> {
        // get usable regions from memory map
        let regions = self.memory_map.iter();
        let usable_regions = regions.filter(|r| r.region_type == MemoryRegionType::Usable);
        // map each region to its address range
        let addr_ranges = usable_regions.map(|r| r.range.start_addr()..r.range.end_addr());
        // transform to an iterator of frame start addresses
        let frame_addresses = addr_ranges.flat_map(|r| r.step_by(4096));
        // create `PhysFrame` types from the start addresses
        frame_addresses.map(|addr| PhysFrame::containing_address(PhysAddr::new(addr)))
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let frame = self.usable_frames().nth(self.next);
        self.next += 1;
        frame
    }
}
