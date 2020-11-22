use crate::{
    memory::{identity_map, identity_unmap, BootInfoFrameAllocator},
    println,
};
use acpi::{AcpiHandler, PhysicalMapping};
use alloc::rc::Rc;
use core::ptr::NonNull;
use spin::Mutex;
use x86_64::structures::paging::{mapper::MapToError, OffsetPageTable, PageTableFlags};

#[derive(Clone)]
pub struct Handler {
    pub allocator: Rc<Mutex<BootInfoFrameAllocator>>,
    pub mapper: Rc<Mutex<OffsetPageTable<'static>>>,
}

impl AcpiHandler for Handler {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        let mut allocator = self.allocator.lock();
        let mut mapper = self.mapper.lock();

        println!("Map: {:#X}:{:#X}", physical_address, size);

        match identity_map(
            physical_address as u64,
            size as u64,
            PageTableFlags::PRESENT,
            &mut *mapper,
            &mut *allocator,
        ) {
            Ok(()) => (),
            // TODO: Emit warning
            Err(MapToError::PageAlreadyMapped(_)) => (),
            Err(e) => panic!("{:#?}", e),
        }

        PhysicalMapping {
            physical_start: physical_address,
            virtual_start: NonNull::new_unchecked(physical_address as *mut _),
            region_length: size,
            mapped_length: size,
            handler: self.clone(),
        }
    }

    fn unmap_physical_region<T>(&self, _region: &PhysicalMapping<Self, T>) {
        // TODO: Reenable this
        // The acpi crate doesn't worry about pages and will map addresses that
        // contain other mapped addresses invalidating them and causing
        // a page fault
        /* let mut mapper = self.mapper.lock();

        println!(
            "Unmap: {:#X}:{:#X}",
            region.physical_start, region.mapped_length
        );

        unsafe {
            identity_unmap(
                region.virtual_start.as_ptr() as u64,
                region.mapped_length as u64,
                &mut *mapper,
            );
        } */
    }
}
