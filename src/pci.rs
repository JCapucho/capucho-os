use alloc::vec::Vec;
use pci_types::{ConfigRegionAccess, PciAddress, PciHeader};
use x86_64::instructions::port::Port;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

pub struct ConfigSpaceMechanism1;

impl ConfigRegionAccess for ConfigSpaceMechanism1 {
    fn function_exists(&self, address: pci_types::PciAddress) -> bool {
        let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
        let mut data_port = Port::<u32>::new(CONFIG_DATA);

        let config_address: ConfigAddress = address.into();

        unsafe { address_port.write(config_address.0) };

        let vendor = unsafe { data_port.read() } & 0xffff;

        vendor != 0xFFFF
    }

    unsafe fn read(&self, address: pci_types::PciAddress, offset: u16) -> u32 {
        fn read_inner(address: pci_types::PciAddress, offset: u16) -> u32 {
            if (offset & 0b11) != 0 {
                panic!("Try to read pci with unaligned offset")
            }

            let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
            let mut data_port = Port::<u32>::new(CONFIG_DATA);

            let config_address: ConfigAddress = address.into();

            unsafe { address_port.write(config_address.0 | (offset as u32) & 0xff) };

            unsafe { data_port.read() }
        }

        read_inner(address, offset)
    }

    unsafe fn write(&self, address: pci_types::PciAddress, offset: u16, value: u32) {
        fn write_inner(address: pci_types::PciAddress, offset: u16, value: u32) {
            if (offset & 0b11) != 0 {
                panic!("Try to read pci with unaligned offset")
            }

            let mut address_port = Port::<u32>::new(CONFIG_ADDRESS);
            let mut data_port = Port::<u32>::new(CONFIG_DATA);

            let config_address: ConfigAddress = address.into();

            unsafe { address_port.write(config_address.0 | (offset as u32) & 0xff) };

            unsafe { data_port.write(value) };
        }

        write_inner(address, offset, value)
    }
}

pub struct ConfigAddress(u32);

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

pub fn brute_force_find() -> Vec<(PciAddress, PciHeader)> {
    let mut results = Vec::new();

    for bus in 0..=255 {
        for device in 0..32 {
            let address = PciAddress::new(0, bus, device, 0);

            if ConfigSpaceMechanism1.function_exists(address) {
                results.push((address, PciHeader::new(address)))
            }
        }
    }

    results
}
