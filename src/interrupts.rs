use crate::{gdt, hlt_loop, memory, print, println};
use core::fmt::{self, Display};
use lazy_static::lazy_static;
use x86_64::structures::{
    idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode},
    paging::Translate,
};

use self::controller::InterruptController;

mod controller;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
}

pub static PICS: spin::Mutex<InterruptController> =
    spin::Mutex::new(InterruptController::new(PIC_1_OFFSET, PIC_2_OFFSET));

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt[InterruptIndex::Timer as usize].set_handler_fn(timer_interrupt_handler);
        idt[InterruptIndex::Keyboard as usize].set_handler_fn(keyboard_interrupt_handler);
        idt
    };
}

pub fn init_idt() { IDT.load(); }

extern "x86-interrupt" fn breakpoint_handler(stack_frame: &mut InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;

    let addr = Cr2::read();

    println!("EXCEPTION: PAGE FAULT");
    println!("Accessed Address: {:?}", addr);
    println!("Error Code: {:?}", error_code);
    println!("{}", stack_frame_display(stack_frame));

    if let Some(ctx) = memory::PAGING_CTX.get().and_then(|ctx| ctx.try_lock()) {
        match ctx.mapper.translate(addr) {
            x86_64::structures::paging::mapper::TranslateResult::Mapped {
                frame, flags, ..
            } => {
                println!("FRAME: {:#X} ", frame.start_address());
                println!("FLAGS: {:?} ", flags);
            },
            x86_64::structures::paging::mapper::TranslateResult::NotMapped => {
                println!("NOT MAPPED");
            },
            x86_64::structures::paging::mapper::TranslateResult::InvalidFrameAddress(_) => {
                println!("INVALID PAGE TABLE");
            },
        }
    }

    hlt_loop();
}

fn stack_frame_display(frame: &InterruptStackFrame) -> impl Display + '_ {
    struct FrameDisplay<'a>(&'a InterruptStackFrame);

    impl<'a> Display for FrameDisplay<'a> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            use x86_64::registers::rflags::RFlags;

            writeln!(f, "IP: {:#X}", self.0.instruction_pointer.as_u64())?;
            writeln!(f, "CS: {:#X}", self.0.code_segment)?;
            writeln!(f, "SP: {:#X}", self.0.stack_pointer.as_u64())?;
            writeln!(f, "SS: {:#X}", self.0.stack_segment)?;
            write!(
                f,
                "RFLAGS: {:?}",
                RFlags::from_bits_truncate(self.0.cpu_flags)
            )
        }
    }

    FrameDisplay(frame)
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: &mut InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: &mut InterruptStackFrame) {
    // print!(".");
    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Timer as u8);
    }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: &mut InterruptStackFrame) {
    use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
    use spin::Mutex;
    use x86_64::instructions::port::Port;

    lazy_static! {
        static ref KEYBOARD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = Mutex::new(
            Keyboard::new(layouts::Us104Key, ScancodeSet1, HandleControl::Ignore)
        );
    }

    let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);

    let scancode: u8 = unsafe { port.read() };
    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                DecodedKey::Unicode(character) => print!("{}", character),
                DecodedKey::RawKey(key) => print!("{:?}", key),
            }
        }
    }

    unsafe {
        PICS.lock()
            .notify_end_of_interrupt(InterruptIndex::Keyboard as u8);
    }
}
