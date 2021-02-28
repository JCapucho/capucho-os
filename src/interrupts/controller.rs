/// The chained pics code is heavily based on the `pic8259_simple` crate
use x86_64::structures::port::{PortRead, PortWrite};

const PIC1_CMD_PORT: u16 = 0x20;
const PIC1_DATA_PORT: u16 = 0x21;
const PIC2_CMD_PORT: u16 = 0xA0;
const PIC2_DATA_PORT: u16 = 0xA1;
const PICS_8086_MODE: u8 = 0x01;
const PICS_EOI_CMD: u8 = 0x20;
const PICS_INIT_CMD: u8 = 0x11;

pub enum InterruptController {
    Pics { pic1_offset: u8, pic2_offset: u8 },
    Apic { base_address: u64 },
}

impl InterruptController {
    pub const fn new(pic1_offset: u8, pic2_offset: u8) -> Self {
        InterruptController::Pics {
            pic1_offset,
            pic2_offset,
        }
    }

    pub unsafe fn init(&self) {
        unsafe fn init_pics(pic1_offset: u8, pic2_offset: u8) {
            // We need to add a delay between writes to our PICs.
            // But we don't have any timers yet, because they require
            // interrupts. Older versions of Linux and other OSes worked around
            // this by writing to port 0x80, which allegedly takes long enough
            // to make everything work on most hardware.
            let wait = || u8::write_to_port(0x80, 0);

            let pic1_mask = u8::read_from_port(PIC1_DATA_PORT);
            let pic2_mask = u8::read_from_port(PIC2_DATA_PORT);

            // Signal the PICS to start the initialization sequence
            u8::write_to_port(PIC1_CMD_PORT, PICS_INIT_CMD);
            wait();
            u8::write_to_port(PIC2_CMD_PORT, PICS_INIT_CMD);
            wait();

            // Set the offsets
            u8::write_to_port(PIC1_DATA_PORT, pic1_offset);
            wait();
            u8::write_to_port(PIC2_DATA_PORT, pic2_offset);
            wait();

            // Configure the chaining
            u8::write_to_port(PIC1_DATA_PORT, 2);
            wait();
            u8::write_to_port(PIC2_DATA_PORT, 4);
            wait();

            // Set the mode
            u8::write_to_port(PIC1_DATA_PORT, PICS_8086_MODE);
            wait();
            u8::write_to_port(PIC2_DATA_PORT, PICS_8086_MODE);
            wait();

            // Restore the mask
            u8::write_to_port(PIC1_DATA_PORT, pic1_mask);
            u8::write_to_port(PIC2_DATA_PORT, pic2_mask);
        }

        match self {
            InterruptController::Pics {
                pic1_offset,
                pic2_offset,
            } => init_pics(*pic1_offset, *pic2_offset),
            InterruptController::Apic { base_address } => {
                let siv_reg = read_apic_reg(*base_address, 0xF0);
                write_apic_reg(*base_address, 0xF0, siv_reg | 0x100);

                u8::write_to_port(PIC1_DATA_PORT, 0xFF);
                u8::write_to_port(PIC2_DATA_PORT, 0xFF);
            },
        }
    }

    pub unsafe fn notify_end_of_interrupt(&self, id: u8) {
        match self {
            InterruptController::Pics {
                pic1_offset,
                pic2_offset,
            } => {
                let pic_range = |offset| offset..(offset + 8);

                if pic_range(*pic2_offset).contains(&id) {
                    u8::write_to_port(PIC2_CMD_PORT, PICS_EOI_CMD);
                    u8::write_to_port(PIC1_CMD_PORT, PICS_EOI_CMD);
                } else if pic_range(*pic1_offset).contains(&id) {
                    u8::write_to_port(PIC1_CMD_PORT, PICS_EOI_CMD);
                }
            },
            InterruptController::Apic { base_address } => write_apic_reg(*base_address, 0xB0, 0),
        }
    }

    pub unsafe fn apic_handover(&mut self, base_address: u64) {
        *self = InterruptController::Apic { base_address };
        self.init()
    }
}

unsafe fn read_apic_reg(base_address: u64, offset: usize) -> u32 {
    let ptr = (base_address as usize + offset) as *mut u32;
    ptr.read_volatile()
}

unsafe fn write_apic_reg(base_address: u64, offset: usize, val: u32) {
    let ptr = (base_address as usize + offset) as *mut u32;
    ptr.write_volatile(val)
}
