use super::LockedHandler;
use crate::pci;
use acpi::{AcpiHandler, PhysicalMapping};
use aml::Handler as AmlHandler;
use core::ptr::NonNull;
use pci_types::PciAddress;
use x86_64::{
    structures::{
        paging::{Page, PhysFrame},
        port::{PortRead, PortWrite},
    },
    PhysAddr, VirtAddr,
};

impl AcpiHandler for LockedHandler {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        log::trace!("Map: {:#X}:{:#X}", physical_address, size);

        let start = PhysFrame::containing_address(PhysAddr::new(physical_address as u64));
        let end = PhysFrame::containing_address(PhysAddr::new((physical_address + size) as u64));

        for frame in PhysFrame::range_inclusive(start, end) {
            self.map(frame)
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
        log::trace!(
            "Unmap: {:#X}:{:#X}",
            region.physical_start,
            region.mapped_length
        );

        let start = Page::from_start_address(VirtAddr::new(region.physical_start as u64)).unwrap();
        let end = Page::from_start_address(VirtAddr::new(
            (region.physical_start + region.mapped_length) as u64,
        ))
        .unwrap();

        for page in Page::range(start, end) {
            self.unmap(page)
        }
    }
}

impl AmlHandler for LockedHandler {
    fn read_u8(&self, address: usize) -> u8 { unsafe { self.read(address) } }

    fn read_u16(&self, address: usize) -> u16 { unsafe { self.read(address) } }

    fn read_u32(&self, address: usize) -> u32 { unsafe { self.read(address) } }

    fn read_u64(&self, address: usize) -> u64 { unsafe { self.read(address) } }

    fn write_u8(&mut self, address: usize, value: u8) { unsafe { self.write(address, value) } }

    fn write_u16(&mut self, address: usize, value: u16) { unsafe { self.write(address, value) } }

    fn write_u32(&mut self, address: usize, value: u32) { unsafe { self.write(address, value) } }

    fn write_u64(&mut self, address: usize, value: u64) { unsafe { self.write(address, value) } }

    fn read_io_u8(&self, port: u16) -> u8 { unsafe { u8::read_from_port(port) } }

    fn read_io_u16(&self, port: u16) -> u16 { unsafe { u16::read_from_port(port) } }

    fn read_io_u32(&self, port: u16) -> u32 { unsafe { u32::read_from_port(port) } }

    fn write_io_u8(&self, port: u16, value: u8) { unsafe { u8::write_to_port(port, value) } }

    fn write_io_u16(&self, port: u16, value: u16) { unsafe { u16::write_to_port(port, value) } }

    fn write_io_u32(&self, port: u16, value: u32) { unsafe { u32::write_to_port(port, value) } }

    fn read_pci_u8(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u8 {
        let address = PciAddress::new(segment, bus, device, function);
        unsafe { (pci::read(address, offset & 0xFFFC) >> (offset % 4) * 8) as u8 }
    }

    fn read_pci_u16(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u16 {
        let address = PciAddress::new(segment, bus, device, function);
        unsafe { (pci::read(address, offset & 0xFFFC) >> (offset % 2) * 16) as u16 }
    }

    fn read_pci_u32(&self, segment: u16, bus: u8, device: u8, function: u8, offset: u16) -> u32 {
        let address = PciAddress::new(segment, bus, device, function);
        unsafe { pci::read(address, offset) }
    }

    fn write_pci_u8(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u8,
    ) {
        let address = PciAddress::new(segment, bus, device, function);
        unsafe { pci::write(address, offset & 0xFFFC, (value as u32) << (offset % 4) * 8) }
    }

    fn write_pci_u16(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u16,
    ) {
        let address = PciAddress::new(segment, bus, device, function);
        unsafe {
            pci::write(
                address,
                offset & 0xFFFC,
                (value as u32) << (offset % 2) * 16,
            )
        }
    }

    fn write_pci_u32(
        &self,
        segment: u16,
        bus: u8,
        device: u8,
        function: u8,
        offset: u16,
        value: u32,
    ) {
        let address = PciAddress::new(segment, bus, device, function);
        unsafe { pci::write(address, offset, value) }
    }
}
