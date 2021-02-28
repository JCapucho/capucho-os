use crate::{interrupts, memory::identity_map};
use core::fmt;
use x86_64::{
    structures::paging::{PageTableFlags, PhysFrame},
    PhysAddr,
};

/// # Safety
/// The provided `base_address` must be valid
pub unsafe fn lapic_handover(base_address: u64) {
    identity_map(
        PhysFrame::from_start_address(PhysAddr::new(base_address)).unwrap(),
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
    )
    .expect("Failed to identity map");

    interrupts::PICS.lock().apic_handover(base_address);
}

pub struct IOApic(u64);

impl IOApic {
    /// # Safety
    /// The provided `base_address` must be valid
    pub unsafe fn new(base_address: u64) -> Self {
        identity_map(
            PhysFrame::from_start_address(PhysAddr::new(base_address)).unwrap(),
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
        )
        .expect("Failed to identity map");

        IOApic(base_address)
    }

    pub fn id(&self) -> u8 {
        let res = unsafe { self.read_reg(0x00) };
        ((res >> 24) & 0xF) as u8
    }

    pub fn version(&self) -> u8 {
        let res = unsafe { self.read_reg(0x01) };
        (res & 0xff) as u8
    }

    pub fn redir_entry_count(&self) -> u8 {
        let res = unsafe { self.read_reg(0x01) };
        ((res >> 16) & 0xFF) as u8
    }

    pub fn arbitration_priority(&self) -> u8 {
        let res = unsafe { self.read_reg(0x02) };
        ((res >> 24) & 0xF) as u8
    }

    pub fn redir_entry(&self, idx: u8) -> RedirEntry {
        let low = unsafe { self.read_reg(0x10 + idx * 2) } as u64;
        let high = unsafe { self.read_reg(0x11 + idx * 2) } as u64;

        RedirEntry(high << 32 | low)
    }

    pub fn set_redir_entry(&self, idx: u8, entry: RedirEntry) {
        unsafe {
            self.write_reg(0x10 + idx * 2, entry.0 as u32);
            self.write_reg(0x11 + idx * 2, (entry.0 >> 32) as u32)
        }
    }

    pub fn redir_entry_iter(&self) -> RedirEntryIter {
        RedirEntryIter {
            idx: 0,
            ioapic: self,
        }
    }

    unsafe fn read_reg(&self, reg: u8) -> u32 {
        let address_reg = self.0 as *mut u32;
        let data_reg = (self.0 + 0x10) as *const u32;
        address_reg.write_volatile(reg as u32);
        data_reg.read_volatile()
    }

    unsafe fn write_reg(&self, reg: u8, val: u32) {
        let address_reg = self.0 as *mut u32;
        let data_reg = (self.0 + 0x10) as *mut u32;
        address_reg.write_volatile(reg as u32);
        data_reg.write_volatile(val)
    }
}

pub struct RedirEntryIter<'a> {
    idx: u8,
    ioapic: &'a IOApic,
}

impl<'a> Iterator for RedirEntryIter<'a> {
    type Item = RedirEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.idx == self.ioapic.redir_entry_count() {
            return None;
        }

        let res = self.ioapic.redir_entry(self.idx);

        self.idx += 1;

        Some(res)
    }
}

#[derive(Debug)]
pub enum DeliveryMode {
    Normal,
    LowPriority,
    SMInterrupt,
    NMInterrupt,
    Init,
    External,
    Reserved,
}

#[repr(C)]
pub struct RedirEntry(u64);

impl RedirEntry {
    pub fn new(vector: u8) -> Self { RedirEntry(vector as u64) }

    pub fn vector(&self) -> u8 { (self.0 & 0xFF) as u8 }

    pub fn set_vector(&mut self, vector: u8) { self.0 |= vector as u64 }

    pub fn delivery_mode(&self) -> DeliveryMode {
        let bits = (self.0 >> 8) & 0b111;
        match bits {
            0 => DeliveryMode::Normal,
            1 => DeliveryMode::LowPriority,
            2 => DeliveryMode::SMInterrupt,
            4 => DeliveryMode::NMInterrupt,
            5 => DeliveryMode::Init,
            7 => DeliveryMode::External,
            _ => DeliveryMode::Reserved,
        }
    }

    pub fn set_delivery_mode(&mut self, mode: DeliveryMode) {
        let bits = match mode {
            DeliveryMode::Normal => 0,
            DeliveryMode::LowPriority => 1,
            DeliveryMode::SMInterrupt => 2,
            DeliveryMode::NMInterrupt => 4,
            DeliveryMode::Init => 5,
            DeliveryMode::External => 7,
            DeliveryMode::Reserved => panic!("Cannot use a reserved mode"),
        };

        self.0 ^= 0b111 << 8;
        self.0 |= bits << 8;
    }

    /// true for logical, false for physical
    pub fn logical_mode(&self) -> bool {
        let bit = (self.0 >> 11) & 0b1;
        bit != 0
    }

    pub fn set_logical_mode(&mut self, mode: bool) {
        self.0 ^= 0b1 << 11;
        self.0 |= (mode as u64) << 11;
    }

    pub fn is_busy(&self) -> bool {
        let bit = (self.0 >> 12) & 0b1;
        bit != 0
    }

    /// true for Low is active, false for High is active
    pub fn low_is_active(&self) -> bool {
        let bit = (self.0 >> 13) & 0b1;
        bit != 0
    }

    /// true for Low is active, false for High is active
    pub fn set_low_is_active(&mut self, mode: bool) {
        self.0 ^= 0b1 << 13;
        self.0 |= (mode as u64) << 13;
    }

    /// Used for level triggered interrupts only to show if a local APIC
    /// has received the interrupt (false), or has sent an EOI (true).
    pub fn lapic_responded(&self) -> bool {
        let bit = (self.0 >> 14) & 0b1;
        bit == 0
    }

    /// true for level sensitive, false for edge sensitive
    pub fn level_sensitive(&self) -> bool {
        let bit = (self.0 >> 15) & 0b1;
        bit != 0
    }

    /// true for level sensitive, false for edge sensitive
    pub fn set_level_sensitive(&mut self, mode: bool) {
        self.0 ^= 0b1 << 15;
        self.0 |= (mode as u64) << 15;
    }

    pub fn masked(&self) -> bool {
        let bit = (self.0 >> 16) & 0b1;
        bit != 0
    }

    pub fn set_masked(&mut self, mode: bool) {
        self.0 ^= 0b1 << 16;
        self.0 |= (mode as u64) << 16;
    }

    pub fn destination_id(&self) -> u8 { ((self.0 >> 56) & 0xF) as u8 }
}

impl fmt::Debug for RedirEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedirEntry")
            .field("vector", &self.vector())
            .field("delivery_mode", &self.delivery_mode())
            .field(
                "destinantion_mode",
                &if self.logical_mode() {
                    "logical"
                } else {
                    "physical"
                },
            )
            .field(
                "polarity",
                &if self.low_is_active() {
                    "Low is active"
                } else {
                    "High is active"
                },
            )
            .field("lapic_responded", &self.lapic_responded())
            .field(
                "trigger_mode",
                &if self.level_sensitive() {
                    "level"
                } else {
                    "edge"
                },
            )
            .field("masked", &self.masked())
            .field("destination", &self.destination_id())
            .finish()
    }
}
