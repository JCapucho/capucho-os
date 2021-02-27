use crate::memory::{identity_map, identity_unmap, BootInfoFrameAllocator};
use acpi::{AcpiHandler, PhysicalMapping};
use alloc::{collections::BTreeMap, rc::Rc};
use core::ptr::NonNull;
use spin::Mutex;
use x86_64::{
    structures::paging::{OffsetPageTable, PageTableFlags, PhysFrame},
    PhysAddr,
};

#[derive(Clone)]
pub struct LockedHandler<'a> {
    inner: Rc<Mutex<Handler<'a>>>,
}

impl<'a> LockedHandler<'a> {
    pub fn new(
        allocator: &'a mut BootInfoFrameAllocator,
        mapper: &'a mut OffsetPageTable<'static>,
    ) -> Self {
        let inner = Handler::new(allocator, mapper);

        LockedHandler {
            inner: Rc::new(Mutex::new(inner)),
        }
    }
}

struct Handler<'a> {
    allocator: &'a mut BootInfoFrameAllocator,
    mapper: &'a mut OffsetPageTable<'static>,
    mapping_refs: BTreeMap<u64, usize>,
}

impl<'a> Handler<'a> {
    fn new(
        allocator: &'a mut BootInfoFrameAllocator,
        mapper: &'a mut OffsetPageTable<'static>,
    ) -> Self {
        Handler {
            allocator,
            mapper,
            mapping_refs: BTreeMap::new(),
        }
    }
}

impl<'a> AcpiHandler for LockedHandler<'a> {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        log::debug!("Map: {:#X}:{:#X}", physical_address, size);

        let handler = &mut *self.inner.lock();

        let start = PhysFrame::containing_address(PhysAddr::new(physical_address as u64));
        let end = PhysFrame::containing_address(PhysAddr::new((physical_address + size) as u64));

        for frame in PhysFrame::range_inclusive(start, end) {
            let entry = handler
                .mapping_refs
                .entry(frame.start_address().as_u64())
                .and_modify(|e| *e += 1)
                .or_insert(1);

            if *entry != 1 {
                continue;
            }

            identity_map(
                frame,
                PageTableFlags::PRESENT,
                handler.mapper,
                handler.allocator,
            )
            .expect("Failed to identity map");
        }

        let mapped_length =
            (end.start_address().as_u64() + 0x1000 - start.start_address().as_u64()) as usize;

        PhysicalMapping {
            physical_start: start.start_address().as_u64() as usize,
            virtual_start: NonNull::new_unchecked(physical_address as *mut _),
            region_length: size,
            mapped_length,
            handler: self.clone(),
        }
    }

    fn unmap_physical_region<T>(&self, region: &PhysicalMapping<Self, T>) {
        log::debug!(
            "Unmap: {:#X}:{:#X}",
            region.physical_start,
            region.mapped_length
        );

        let handler = &mut *self.inner.lock();

        let start =
            PhysFrame::from_start_address(PhysAddr::new(region.physical_start as u64)).unwrap();
        let end = PhysFrame::from_start_address(PhysAddr::new(
            (region.physical_start + region.mapped_length) as u64,
        ))
        .unwrap();

        for frame in PhysFrame::range(start, end) {
            let entry = handler
                .mapping_refs
                .get_mut(&frame.start_address().as_u64())
                .expect("Trying to unmap non mapped frame");

            *entry -= 1;

            if *entry == 0 {
                unsafe {
                    identity_unmap(frame, handler.mapper).expect("Failed to identity map");
                }
            }
        }
    }
}
