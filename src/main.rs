#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(asm)]
#![test_runner(capucho_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use bootloader::{entry_point, BootInfo};
use capucho_os::{acpi::SleepState, apic, println};
use core::panic::PanicInfo;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    println!("Hello World!");

    capucho_os::init(boot_info);

    let mut acpi = unsafe { capucho_os::acpi::bios_get_acpi() };
    let platform_info = acpi.platform_info();

    if unsafe { !acpi.enable() } {
        panic!("Failed to init the acpi")
    }

    match platform_info.interrupt_model {
        acpi::InterruptModel::Unknown => (),
        acpi::InterruptModel::Apic(apic) => apic::apic_init(&mut acpi, apic),
        _ => unreachable!(),
    }

    let devices = capucho_os::pci::brute_force_find();

    for (_, header) in devices {
        use pci_types::device_type::DeviceType;

        let access = capucho_os::pci::ConfigSpaceMechanism1;

        let (rev, class, subclass, interface) = header.revision_and_class(&access);
        let has_multiple_functions = header.has_multiple_functions(&access);

        log::info!(
            "{:?} rev: {} interface: {} functions?: {}",
            DeviceType::from((class, subclass)),
            rev,
            interface,
            has_multiple_functions
        )
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

#[test_case]
fn trivial_assertion() {
    assert_eq!(1, 1);
}
