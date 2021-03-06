use bootloader::bootinfo::{MemoryMap, MemoryRegionType};
use core::ptr;
use x86_64::{
    structures::paging::{FrameAllocator, FrameDeallocator, Mapper, PhysFrame, Size4KiB},
    PhysAddr,
};

use self::bootstrap::BootStrapAllocator;

mod bootstrap;

pub const BITMAP_START: u64 = 0x_6666_6666_0000;

// Only use 4088 bytes so that with the pointer to the next block we only
// consume one 4k frame
const STORAGE_PER_BITMAP: usize = 1022;
const BITS_PER_BITMAP: usize = STORAGE_PER_BITMAP * 32;

#[repr(C, packed)]
struct FrameAllocatorBitmap {
    bits: [u32; STORAGE_PER_BITMAP],
    next: *mut Self,
}

impl FrameAllocatorBitmap {
    fn next_block(&self) -> Option<*const FrameAllocatorBitmap> {
        if !self.next.is_null() {
            Some(self.next)
        } else {
            None
        }
    }

    fn next_block_mut(&mut self) -> Option<*mut FrameAllocatorBitmap> {
        if !self.next.is_null() {
            Some(self.next)
        } else {
            None
        }
    }
}

impl Default for FrameAllocatorBitmap {
    fn default() -> Self {
        FrameAllocatorBitmap {
            bits: [Default::default(); 1022],
            next: ptr::null_mut(),
        }
    }
}

unsafe impl Send for FrameAllocatorBitmap {}

/// A FrameAllocator that returns usable frames from the bootloader's memory
/// map.
pub struct GlobalFrameAllocator {
    memory_map: &'static MemoryMap,
    root: *mut FrameAllocatorBitmap,
    next_usable: u64,
}

impl GlobalFrameAllocator {
    /// Create a FrameAllocator from the passed memory map.
    ///
    /// # Safety
    ///
    /// This function is unsafe because the caller must guarantee that the
    /// passed memory map is valid. The main requirement is that all frames
    /// that are marked as `USABLE` in it are really unused.
    pub unsafe fn init(memory_map: &'static MemoryMap, mapper: &mut impl Mapper<Size4KiB>) -> Self {
        // Calculate the number of required bitmaps by getting the index of the
        // last frame and ceiling diving by the number of bits per bitmap
        //
        // Code for the fast ceiling division taken from:
        // https://stackoverflow.com/questions/2745074/fast-ceiling-of-an-integer-division-in-c-c
        let end_frame = memory_map.last().unwrap().range.end_frame_number;
        let required_bitmaps = (end_frame + BITS_PER_BITMAP as u64 - 1) / BITS_PER_BITMAP as u64;

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
        let root = bootstrap.allocate_bitmap(mapper, 0);
        let mut last = &mut *root;

        for i in 1..required_bitmaps {
            let next = bootstrap.allocate_bitmap(mapper, i);

            last.next = next;

            last = &mut *next;
        }

        let mut this = GlobalFrameAllocator {
            memory_map,
            root,
            next_usable: 0,
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
        let (bitmap, int, mask) = frame_idx_to_parts(idx);

        let block = unsafe { &*self.get_bitmap(bitmap).expect("There's no next block") };

        block.bits[int] & mask != 0
    }

    /// Set the frame `idx` as used
    fn mark_used(&mut self, idx: u64) {
        let (bitmap, int, mask) = frame_idx_to_parts(idx);

        let block = unsafe { &mut *self.get_bitmap_mut(bitmap).expect("There's no next block") };

        block.bits[int] |= mask;
    }

    /// Set the frame `idx` as unused
    fn mark_unused(&mut self, idx: u64) {
        let (bitmap, int, mask) = frame_idx_to_parts(idx);

        let block = unsafe { &mut *self.get_bitmap_mut(bitmap).expect("There's no next block") };

        block.bits[int] &= !mask;
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

    /// Get a pointer to a bitmap at the specified `level` depth
    fn get_bitmap(&self, level: u64) -> Option<*const FrameAllocatorBitmap> {
        let mut ptr = unsafe { &*self.root };

        for _ in 0..level {
            ptr = unsafe { &*ptr.next_block()? };
        }

        Some(ptr)
    }

    /// Get a mutable pointer to a bitmap at the specified `level` depth
    fn get_bitmap_mut(&mut self, level: u64) -> Option<*mut FrameAllocatorBitmap> {
        let mut ptr = unsafe { &mut *self.root };

        for _ in 0..level {
            ptr = unsafe { &mut *ptr.next_block_mut()? };
        }

        Some(ptr)
    }
}

unsafe impl FrameAllocator<Size4KiB> for GlobalFrameAllocator {
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

impl FrameDeallocator<Size4KiB> for GlobalFrameAllocator {
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame<Size4KiB>) {
        self.mark_unused(frame.start_address().as_u64() / 0x1000)
    }
}

unsafe impl Send for GlobalFrameAllocator {}

/// Helper function translates a frame index to it's part in the bitmap
/// (bitmap, int, mask) where bitmap is the depth level of the bitmap
/// int is the index on the `u32` array and mask is the mask over the `u32`
fn frame_idx_to_parts(idx: u64) -> (u64, usize, u32) {
    let bitmap = idx / BITS_PER_BITMAP as u64;
    let int = (idx as usize % BITS_PER_BITMAP) / 32;
    let bit = idx as u32 % 32;

    (bitmap, int, 1 << bit)
}
