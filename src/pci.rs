use alloc::vec::Vec;
use pci_types::{ConfigRegionAccess, PciAddress, PciHeader};
use x86_64::instructions::port::{PortRead, PortWrite};

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

pub unsafe fn read(address: pci_types::PciAddress, offset: u16) -> u32 {
    fn read_inner(address: pci_types::PciAddress, offset: u16) -> u32 {
        if (offset & 0b11) != 0 {
            panic!("Try to read pci with unaligned offset")
        }

        let config_address: ConfigAddress = address.into();

        unsafe { u32::write_to_port(CONFIG_ADDRESS, config_address.0 | (offset as u32) & 0xff) };

        unsafe { u32::read_from_port(CONFIG_DATA) }
    }

    read_inner(address, offset)
}

pub unsafe fn write(address: pci_types::PciAddress, offset: u16, value: u32) {
    fn write_inner(address: pci_types::PciAddress, offset: u16, value: u32) {
        if (offset & 0b11) != 0 {
            panic!("Try to write pci with unaligned offset")
        }

        let config_address: ConfigAddress = address.into();

        unsafe { u32::write_to_port(CONFIG_ADDRESS, config_address.0 | (offset as u32) & 0xff) };

        unsafe { u32::write_to_port(CONFIG_DATA, value) }
    }

    write_inner(address, offset, value)
}

pub struct ConfigSpaceMechanism1;

impl ConfigRegionAccess for ConfigSpaceMechanism1 {
    fn function_exists(&self, address: pci_types::PciAddress) -> bool {
        let vendor = unsafe { self.read(address, 0) & 0xFFFF };

        vendor != 0xFFFF
    }

    unsafe fn read(&self, address: pci_types::PciAddress, offset: u16) -> u32 {
        read(address, offset)
    }

    unsafe fn write(&self, address: pci_types::PciAddress, offset: u16, value: u32) {
        write(address, offset, value)
    }
}

struct ConfigAddress(u32);

impl From<PciAddress> for ConfigAddress {
    fn from(address: PciAddress) -> Self {
        let mut result = 0;

        result |= (address.function() as u32) << 8;
        result |= (address.device() as u32) << 11;
        result |= (address.bus() as u32) << 16;
        result |= 1 << 31;

        Self(result)
    }
}

pub fn brute_force_find(access: &impl ConfigRegionAccess) -> Vec<(PciAddress, PciHeader)> {
    let mut results = Vec::new();

    for bus in 0..=255 {
        for device in 0..32 {
            for function in 0..8 {
                let address = PciAddress::new(0, bus, device, function);

                if access.function_exists(address) {
                    results.push((address, PciHeader::new(address)));
                }
            }
        }
    }

    results
}
