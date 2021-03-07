use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use x86_64::{
    structures::paging::{FrameAllocator, FrameDeallocator, Mapper, PhysFrame, Size4KiB},
    PhysAddr,
};

use self::bootstrap::BootStrapAllocator;

mod bootstrap;

pub const BITMAP_START: u64 = 0x_6666_6666_0000;

/// A FrameAllocator that returns usable frames from the bootloader's memory
/// map.
pub struct GlobalFrameAllocator<'a> {
    memory_map: &'static MemoryMap,
    next_usable: u64,
    bitmap: &'a mut [u32],
}

impl<'a> GlobalFrameAllocator<'a> {
    /// Create a FrameAllocator from the passed memory map.
    ///
    /// # Safety
    ///
    /// This function is unsafe because the caller must guarantee that the
    /// passed memory map is valid. The main requirement is that all frames
    /// that are marked as `USABLE` in it are really unused.
    pub unsafe fn init(memory_map: &'static MemoryMap, mapper: &mut impl Mapper<Size4KiB>) -> Self {
        // Calculate the number of required frames by getting the index of the
        // last frame and ceiling diving by the number of bits per frame
        //
        // Code for the fast ceiling division taken from:
        // https://stackoverflow.com/questions/2745074/fast-ceiling-of-an-integer-division-in-c-c
        const BITS_PER_FRAME: u64 = 0x1000 * 8;
        let end_frame = memory_map.last().unwrap().range.end_frame_number;
        let bitmap_frames = (end_frame + BITS_PER_FRAME as u64 - 1) / BITS_PER_FRAME as u64;

        // We need to map some frames for the bitmaps and since we need to map
        // them we might also need some frames for the page tables.
        //
        // So we use a basic allocator called `BootStrapAllocator` which also
        // stores which frames it has mapped by using an array with the sizes of
        // the range. The address is in the correspoding memory region of the
        // memory map
        let mut bootstrap = BootStrapAllocator {
            memory_map,
            next: 0,
            used: [0; 64],
        };

        // Allocate the bitmaps and store a pointer for the root bitmap
        for i in 0..bitmap_frames {
            bootstrap.allocate_bitmap_frame(mapper, BITMAP_START + i * 0x1000);
        }

        let bitmap =
            core::slice::from_raw_parts_mut(BITMAP_START as *mut _, end_frame as usize + 1);

        let mut this = GlobalFrameAllocator {
            memory_map,
            next_usable: 0,
            bitmap,
        };

        // Mark the frames that were used by the bootstrap allocator
        for (block, size) in bootstrap.used.iter().enumerate().filter(|(_, s)| **s != 0) {
            let start = memory_map[block].range.start_frame_number;

            for i in start..(start + size) {
                this.mark_used(i)
            }
        }

        // Mark frames that shouldn't be used as in use
        for region in bootstrap.memory_map.into_iter() {
            if let MemoryRegionType::Usable
            | MemoryRegionType::Reserved
            | MemoryRegionType::AcpiReclaimable
            | MemoryRegionType::FrameZero = region.region_type
            {
                continue;
            }

            let start = region.range.start_frame_number;
            let end = region.range.end_frame_number;

            for i in start..end {
                this.mark_used(i)
            }
        }

        this
    }

    /// Check if the frame is already in use
    pub fn frame_in_use(&self, frame: PhysFrame<Size4KiB>) -> bool {
        self.is_used(frame.start_address().as_u64() / 0x1000)
    }

    /// Check if the frame `idx` is used
    fn is_used(&self, idx: u64) -> bool {
        let (int, mask) = frame_idx_to_parts(idx);

        self.bitmap[int] & mask != 0
    }

    /// Set the frame `idx` as used
    fn mark_used(&mut self, idx: u64) {
        let (int, mask) = frame_idx_to_parts(idx);

        self.bitmap[int] |= mask;
    }

    /// Set the frame `idx` as unused
    fn mark_unused(&mut self, idx: u64) {
        let (int, mask) = frame_idx_to_parts(idx);

        self.bitmap[int] &= !mask;
    }

    /// Get the `MemoryRegionType` of a frame
    pub fn get_frame_ty(&self, frame: PhysFrame) -> Option<MemoryRegionType> {
        self.memory_map
            .into_iter()
            .find(|v| {
                let addr = frame.start_address().as_u64();

                v.range.start_addr() >= addr && addr < v.range.end_addr()
            })
            .map(|v| v.region_type)
    }

    /// Retuns true and sets `self.next_usable` to the index of the next usable
    /// frame if ther's one available otherwise returns false
    fn recalculate_next_usable(&mut self) -> bool {
        /// Helper function returns an iterator of indexes of all usable frames
        fn usable_frames(memory_map: &'static MemoryMap) -> impl Iterator<Item = u64> {
            let regions = memory_map.iter();
            let usable_regions = regions.filter(|r| r.region_type == MemoryRegionType::Usable);
            usable_regions.flat_map(|r| r.range.start_frame_number..r.range.end_frame_number)
        }

        // Get an iterator over the indices of all usable frames that are after
        // the previous `self.next_usable`
        let iter = usable_frames(self.memory_map).skip_while(|r| *r < self.next_usable);

        // Try to find a frame that isn't used
        for i in iter {
            if !self.is_used(i) {
                self.next_usable = i;
                return true;
            }
        }

        // There are no usable frames
        false
    }
}

unsafe impl<'a> FrameAllocator<Size4KiB> for GlobalFrameAllocator<'a> {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        // See if there are frames available
        if self.recalculate_next_usable() {
            let i = self.next_usable;

            //Mark the frame as used
            self.mark_used(i);

            let addr = PhysAddr::new(i * 0x1000);
            let frame = unsafe { PhysFrame::from_start_address_unchecked(addr) };

            log::trace!("Allocating frame {:#X}", frame.start_address());

            Some(frame)
        } else {
            None
        }
    }
}

impl<'a> FrameDeallocator<Size4KiB> for GlobalFrameAllocator<'a> {
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame<Size4KiB>) {
        self.mark_unused(frame.start_address().as_u64() / 0x1000)
    }
}

/// Helper function translates a frame index to it's part in the bitmap
/// (int, mask) where int is the index on the `u32` array and mask is the mask
/// over the `u32`
fn frame_idx_to_parts(idx: u64) -> (usize, u32) {
    let int = idx as usize / 32;
    let bit = idx as u32 % 32;

    (int, 1 << bit)
}
