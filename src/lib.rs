#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks)]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![feature(const_mut_refs)]
#![feature(const_maybe_uninit_assume_init, maybe_uninit_slice)]
#![test_runner(crate::test_runner)]
#![reexport_test_harness_main = "test_main"]

#[cfg(test)]
use bootloader::entry_point;
use bootloader::BootInfo;
use core::panic::PanicInfo;
use x86_64::{structures::port::PortWrite, VirtAddr};

extern crate alloc;

pub mod acpi;
pub mod allocator;
pub mod apic;
pub mod gdt;
pub mod interrupts;
pub mod logger;
pub mod memory;
pub mod pci;
pub mod sata;
pub mod serial;
pub mod vga_buffer;

pub fn init(boot_info: &'static BootInfo) {
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().init() };
    x86_64::instructions::interrupts::enable();

    // Setup the pit for 1ms tick
    pit_init();

    // Setup logger
    log::set_logger(&logger::Logger).unwrap();
    log::set_max_level(log::LevelFilter::Debug);

    // Setup memory and heap
    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);

    unsafe { memory::init(phys_mem_offset, &boot_info.memory_map) };

    allocator::init_heap().expect("heap initialization failed");
}

fn pit_init() {
    const DIVISOR: u16 = 1193; // 1193182 / 1193 ≃ 1000
    unsafe {
        u8::write_to_port(0x43, 0b00110100);
        u8::write_to_port(0x40, DIVISOR as u8);
        u8::write_to_port(0x40, (DIVISOR >> 8) as u8);
    }
}

pub fn sleep(miliseconds: u64) {
    for _ in 0..miliseconds {
        x86_64::instructions::hlt()
    }
}

pub trait Testable {
    fn run(&self);
}

impl<T> Testable for T
where
    T: Fn(),
{
    fn run(&self) {
        serial_print!("{}...\t", core::any::type_name::<T>());
        self();
        serial_println!("[ok]");
    }
}

pub fn test_runner(tests: &[&dyn Testable]) {
    serial_println!("Running {} tests", tests.len());
    for test in tests {
        test.run();
    }
    exit_qemu(QemuExitCode::Success);
}

pub fn test_panic_handler(info: &PanicInfo) -> ! {
    serial_println!("[failed]\n");
    serial_println!("Error: {}\n", info);
    exit_qemu(QemuExitCode::Failed);
    hlt_loop();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x10,
    Failed = 0x11,
}

pub fn exit_qemu(exit_code: QemuExitCode) {
    use x86_64::instructions::port::Port;

    unsafe {
        let mut port = Port::new(0xf4);
        port.write(exit_code as u32);
    }
}

pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

#[cfg(test)]
entry_point!(test_kernel_main);

/// Entry point for `cargo test`
#[cfg(test)]
fn test_kernel_main(boot_info: &'static BootInfo) -> ! {
    init(boot_info);
    test_main();
    hlt_loop();
}

#[cfg(test)]
#[panic_handler]
fn panic(info: &PanicInfo) -> ! { test_panic_handler(info) }
