#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(asm)]
#![test_runner(capucho_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use bootloader::{entry_point, BootInfo};
use capucho_os::{
    acpi::SleepState, apic, memory::identity_map_mmap_dev, println, sata::HBAMemoryRegisters,
};
use core::panic::PanicInfo;
use pci_types::{device_type::DeviceType, Bar, EndpointHeader};
use x86_64::{structures::paging::PhysFrame, PhysAddr};

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    println!("Hello World!");

    capucho_os::init(boot_info);

    let mut acpi = unsafe { capucho_os::acpi::bios_get_acpi() };
    let platform_info = acpi.platform_info();

    if unsafe { !acpi.enable() } {
        panic!("Failed to init the acpi")
    }

    log::debug!("Apic handover start");

    match platform_info.interrupt_model {
        acpi::InterruptModel::Unknown => (),
        acpi::InterruptModel::Apic(apic) => apic::apic_init(&mut acpi, apic),
        _ => unreachable!(),
    }

    log::debug!("Apic handover end");

    let access = capucho_os::pci::ConfigSpaceMechanism1;

    let devices = capucho_os::pci::brute_force_find(&access);

    let mut sata_controller = None;

    for (address, header) in devices {
        let (_, class, subclass, interface) = header.revision_and_class(&access);

        log::info!(
            "{} {:?} class: {} subclass: {} interface: {} header: {:#X}",
            address,
            DeviceType::from((class, subclass)),
            class,
            subclass,
            interface,
            header.header_type(&access)
        );

        if class == 0x01 && subclass == 0x06 && interface == 0x01 {
            sata_controller = Some(EndpointHeader::from_header(header, &access).unwrap())
        }
    }

    let sata_controller = sata_controller.expect("There's no sata controller :(");
    let (abar_address, abar_size) = {
        let bar = sata_controller
            .bar(5, &access)
            .expect("There's no ABAR -_-");

        log::info!("{:#X?}", bar);

        match bar {
            Bar::Memory32 { address, size, .. } => (address as u64, size as u64),
            Bar::Memory64 { address, size, .. } => (address, size),
            Bar::Io { .. } => panic!("ABAR is in port space o_O"),
        }
    };

    let start = PhysFrame::containing_address(PhysAddr::new(abar_address as u64));
    let end = PhysFrame::containing_address(PhysAddr::new((abar_address + abar_size - 1) as u64));

    for frame in PhysFrame::range_inclusive(start, end) {
        unsafe { identity_map_mmap_dev(frame).expect("Failed to mmap the sata device") }
    }

    let hba_mem_reg = unsafe { &mut *(abar_address as *mut HBAMemoryRegisters) };

    unsafe {
        log::info!(
            "{:?} {} {} {:?}",
            hba_mem_reg.cap,
            hba_mem_reg.cap.number_of_ports(),
            hba_mem_reg.cap.number_of_cmd_slots(),
            hba_mem_reg.cap.if_speed(),
        );

        log::info!("{:?}", hba_mem_reg.ghc);
    }

    for port in hba_mem_reg.port_slice_mut() {
        unsafe {
            log::info!("{:#X}", port.sig);
        }
    }

    #[cfg(test)]
    test_main();

    log::info!("Now perish");

    if !acpi.set_sleep_state(SleepState::S5) {
        panic!("Failed to shutdown")
    }

    unreachable!()
}

/// This function is called on panic.
#[cfg(not(test))]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    log::error!("{}", info);
    capucho_os::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! { capucho_os::test_panic_handler(info) }
