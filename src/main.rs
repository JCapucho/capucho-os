#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(asm)]
#![test_runner(capucho_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use bootloader::{entry_point, BootInfo};
use capucho_os::{apic, println};
use core::panic::PanicInfo;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    println!("Hello World!");
    capucho_os::init(boot_info);

    let (acpi_tables, mut aml_context) = unsafe { capucho_os::acpi::bios_get_acpi() };
    let platform_info = acpi_tables.platform_info().unwrap();

    aml_context
        .namespace
        .traverse(|name, _| {
            log::info!("{}", name);
            Ok(true)
        })
        .unwrap();

    match platform_info.interrupt_model {
        acpi::InterruptModel::Unknown => (),
        acpi::InterruptModel::Apic(apic) => {
            unsafe { apic::lapic_handover(apic.local_apic_address) };

            let ioapic = unsafe { apic::IOApic::new(apic.io_apics[0].address as u64) };

            // Set keyboard interrupt
            let mut entry = ioapic.redir_entry(1);

            entry.set_vector(33);
            entry.set_masked(false);

            ioapic.set_redir_entry(1, entry);
        },
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

    println!("It did not crash!");
    capucho_os::hlt_loop();
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
