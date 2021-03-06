use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use x86_64::{
    structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB},
    PhysAddr, VirtAddr,
};

use super::{FrameAllocatorBitmap, BITMAP_START};

pub struct BootStrapAllocator {
    pub memory_map: &'static MemoryMap,
    pub next: usize,
    pub used: [u64; 64],
}

impl BootStrapAllocator {
    pub(super) fn allocate_bitmap(
        &mut self,
        mapper: &mut impl Mapper<Size4KiB>,
        bitmap_idx: u64,
    ) -> *mut FrameAllocatorBitmap {
        let addr = BITMAP_START + bitmap_idx * 0x1000;

        let frame = self.allocate_frame().expect("Failed to allocate frame");

        unsafe {
            mapper
                .map_to(
                    Page::from_start_address_unchecked(VirtAddr::new(addr)),
                    frame,
                    PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                    self,
                )
                .expect("Failed to map")
                .flush()
        }

        addr as *mut FrameAllocatorBitmap
    }

    fn usable_frames(&self) -> impl Iterator<Item = (PhysFrame, usize)> {
        let regions = self.memory_map.iter();
        let usable_regions = regions
            .enumerate()
            .filter(|(_, r)| r.region_type == MemoryRegionType::Usable);
        let addr_ranges =
            usable_regions.map(|(i, r)| (i, r.range.start_addr()..r.range.end_addr()));
        let frame_addresses =
            addr_ranges.flat_map(|(i, r)| r.step_by(4096).zip(core::iter::repeat(i)));
        frame_addresses.map(|(addr, i)| (PhysFrame::containing_address(PhysAddr::new(addr)), i))
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootStrapAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        let (frame, block) = self.usable_frames().nth(self.next)?;
        log::trace!("Bootstrap allocating {:#X}", frame.start_address());
        self.next += 1;
        self.used[block] += 1;
        Some(frame)
    }
}
