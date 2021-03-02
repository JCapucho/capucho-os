use crate::memory::identity_map;
use acpi::{fadt::Fadt, sdt::Signature, AcpiTables, PlatformInfo};
use alloc::{boxed::Box, collections::BTreeMap, rc::Rc};
use aml::{value::Args, AmlContext, AmlName, AmlValue};
use spin::Mutex;
use x86_64::{
    structures::{
        paging::{PageTableFlags, PhysFrame},
        port::{PortRead, PortWrite},
    },
    PhysAddr,
};

mod handlers;

const SLP_EN: u16 = 1 << 13;

#[derive(Clone)]
pub struct LockedHandler {
    inner: Rc<Mutex<Handler>>,
}

impl LockedHandler {
    /// Identity maps a frame
    ///
    /// # Safety
    ///
    /// This function is unsafe because the caller must guarantee that the
    /// frame is free and is usable
    pub unsafe fn map(&self, frame: PhysFrame) {
        let inner = &mut *self.inner.lock();

        inner.map(frame)
    }

    unsafe fn read<T>(&self, address: usize) -> T {
        self.map(PhysFrame::containing_address(PhysAddr::new(address as u64)));
        (address as *const T).read_volatile()
    }

    unsafe fn write<T>(&self, address: usize, value: T) {
        self.map(PhysFrame::containing_address(PhysAddr::new(address as u64)));
        (address as *mut T).write_volatile(value)
    }
}

impl Default for LockedHandler {
    fn default() -> Self {
        LockedHandler {
            inner: Rc::new(Mutex::new(Handler::new())),
        }
    }
}

struct Handler {
    mapping_refs: BTreeMap<u64, usize>,
}

impl Handler {
    fn new() -> Self {
        Handler {
            mapping_refs: BTreeMap::new(),
        }
    }

    fn map(&mut self, frame: PhysFrame) {
        let entry = self
            .mapping_refs
            .entry(frame.start_address().as_u64())
            .and_modify(|e| *e += 1)
            .or_insert(1);

        if *entry != 1 {
            return;
        }

        unsafe {
            identity_map(
                frame,
                PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::NO_CACHE
                    | PageTableFlags::WRITE_THROUGH,
            )
            .expect("Failed to identity map")
        };
    }
}

/// # Safety
/// The system must be using bios
pub unsafe fn bios_get_acpi() -> Acpi {
    fn inner() -> Acpi {
        let handler = LockedHandler::default();

        log::debug!("Reading the acpi tables");

        let tables = unsafe { acpi::AcpiTables::search_for_rsdp_bios(handler.clone()) }.unwrap();

        let mut aml_context =
            aml::AmlContext::new(Box::new(handler.clone()), false, aml::DebugVerbosity::All);

        log::debug!("Reading the dsdt");

        if let Some(ref dsdt) = tables.dsdt {
            let start = PhysFrame::containing_address(PhysAddr::new(dsdt.address as u64));
            let end = PhysFrame::containing_address(PhysAddr::new(
                dsdt.address as u64 + dsdt.length as u64,
            ));

            for frame in PhysFrame::range_inclusive(start, end) {
                unsafe { handler.map(frame) }
            }
            let stream = unsafe {
                core::slice::from_raw_parts(dsdt.address as *const _, dsdt.length as usize)
            };

            aml_context
                .parse_table(stream)
                .expect("Failed to parse the dsdt");
        }

        for ssdt in tables.ssdts.iter() {
            log::debug!("Reading a ssdt");

            let start = PhysFrame::containing_address(PhysAddr::new(ssdt.address as u64));
            let end = PhysFrame::containing_address(PhysAddr::new(
                ssdt.address as u64 + ssdt.length as u64,
            ));

            for frame in PhysFrame::range_inclusive(start, end) {
                unsafe { handler.map(frame) }
            }

            let stream = unsafe {
                core::slice::from_raw_parts(ssdt.address as *const _, ssdt.length as usize)
            };

            aml_context
                .parse_table(stream)
                .expect("Failed to parse the dsdt");
        }

        log::trace!("Starting the aml objects init");

        aml_context
            .initialize_objects()
            .expect("Failed to init the aml objects");

        log::trace!("Finished the aml objects init");

        let fadt: &Fadt = unsafe {
            &tables
                .get_sdt::<Fadt>(Signature::FADT)
                .expect("Error when serching for the FADT")
                .expect("Couldn't find the FADT")
        };

        // Allow unused unsafe blocks because packed struct accesses are
        // unsafe and throw a warning and later become a hard error
        #[allow(unused_unsafe)]
        let pm1a_cnt = unsafe { fadt.pm1a_control_block } as u16;
        #[allow(unused_unsafe)]
        let pm1b_cnt = unsafe { fadt.pm1b_control_block } as u16;

        Acpi {
            tables,
            aml_context,

            acpi_enable: fadt.acpi_enable,
            smi_cmd_port: fadt.smi_cmd_port as u16,
            pm1a_cnt,
            pm1b_cnt,
        }
    }

    inner()
}

#[derive(Debug)]
pub enum SleepState {
    S1,
    S2,
    S3,
    S4,
    S5,
}

impl SleepState {
    pub fn as_aml_name(&self) -> AmlName {
        let name = match self {
            SleepState::S1 => "\\_S1",
            SleepState::S2 => "\\_S2",
            SleepState::S3 => "\\_S3",
            SleepState::S4 => "\\_S4",
            SleepState::S5 => "\\_S5",
        };

        AmlName::from_str(name).unwrap()
    }
}

pub struct Acpi {
    tables: AcpiTables<LockedHandler>,
    aml_context: AmlContext,

    smi_cmd_port: u16,
    pm1a_cnt: u16,
    pm1b_cnt: u16,
    acpi_enable: u8,
}

impl Acpi {
    /// Transfers control from the SMI to the OS
    ///
    /// # Safety
    ///
    /// This function is unsafe because the OS must be prepared to handle the
    /// acpi events
    pub unsafe fn enable(&self) -> bool {
        if self.smi_cmd_port == 0 || self.acpi_enable == 0 {
            return false;
        }

        u8::write_to_port(self.smi_cmd_port, self.acpi_enable);

        let mut finished = false;

        for _ in 0..300 {
            if u16::read_from_port(self.pm1a_cnt) & 1 == 1 && self.pm1b_cnt == 0
                || u16::read_from_port(self.pm1b_cnt) & 1 == 1
            {
                finished = true;
                break;
            }

            crate::sleep(10);
        }

        finished
    }

    pub fn set_sleep_state(&mut self, state: SleepState) -> bool {
        let (slp_typa, slp_typb) = if let Some(val) = self.get_sleep_state(state) {
            val
        } else {
            return false;
        };

        unsafe {
            u16::write_to_port(self.pm1a_cnt, SLP_EN | slp_typa << 10);

            if self.pm1b_cnt != 0 {
                u16::write_to_port(self.pm1b_cnt, SLP_EN | slp_typb << 10);
            }
        }

        true
    }

    pub fn platform_info(&self) -> PlatformInfo {
        self.tables
            .platform_info()
            .expect("Failed to get platform info")
    }

    fn get_sleep_state(&mut self, state: SleepState) -> Option<(u16, u16)> {
        if let AmlValue::Package(items) = self
            .aml_context
            .invoke_method(&state.as_aml_name(), Args::default())
            .ok()?
        {
            let res = items[0].as_integer(&self.aml_context).unwrap();

            return Some(((res as u16) & 0b111, (res >> 8) as u16 & 0b111));
        }

        None
    }

    pub fn aml_context(&mut self) -> &mut AmlContext { &mut self.aml_context }
}
