#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(capucho_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use bootloader::{entry_point, BootInfo};
use capucho_os::println;
use core::panic::PanicInfo;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    use capucho_os::{
        allocator,
        memory::{self, BootInfoFrameAllocator},
    };
    use x86_64::VirtAddr;

    println!("Hello World{}", "!");
    capucho_os::init();

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");

    let devices = capucho_os::pci::brute_force_find();

    for (_, header) in devices {
        use pci_types::device_type::DeviceType;

        let access = capucho_os::pci::ConfigSpaceMechanism1;

        let (rev, class, subclass, interface) = header.revision_and_class(&access);
        let has_multiple_functions = header.has_multiple_functions(&access);

        println!(
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
    capucho_os::hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! { capucho_os::test_panic_handler(info) }

#[test_case]
fn trivial_assertion() {
    assert_eq!(1, 1);
}
