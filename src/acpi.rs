use crate::memory::identity_map;
use acpi::AcpiTables;
use alloc::{boxed::Box, collections::BTreeMap, rc::Rc};
use aml::AmlContext;
use spin::Mutex;
use x86_64::{
    structures::paging::{PageTableFlags, PhysFrame},
    PhysAddr,
};

mod handlers;

#[derive(Clone)]
pub struct LockedHandler {
    inner: Rc<Mutex<Handler>>,
}

impl LockedHandler {
    unsafe fn map(&self, frame: PhysFrame) {
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
pub unsafe fn bios_get_acpi() -> (AcpiTables<LockedHandler>, AmlContext) {
    fn inner() -> (AcpiTables<LockedHandler>, AmlContext) {
        let acpi_handler = LockedHandler::default();

        log::debug!("Reading the acpi tables");

        let acpi_tables =
            unsafe { acpi::AcpiTables::search_for_rsdp_bios(acpi_handler.clone()) }.unwrap();

        let mut aml_context = aml::AmlContext::new(
            Box::new(acpi_handler.clone()),
            false,
            aml::DebugVerbosity::All,
        );

        log::debug!("Reading the dsdt");

        if let Some(ref dsdt) = acpi_tables.dsdt {
            let start = PhysFrame::containing_address(PhysAddr::new(dsdt.address as u64));
            let end = PhysFrame::containing_address(PhysAddr::new(
                dsdt.address as u64 + dsdt.length as u64,
            ));

            for frame in PhysFrame::range_inclusive(start, end) {
                unsafe { acpi_handler.map(frame) }
            }
            let stream = unsafe {
                core::slice::from_raw_parts(dsdt.address as *const _, dsdt.length as usize)
            };

            aml_context
                .parse_table(stream)
                .expect("Failed to parse the dsdt");
        }

        for ssdt in acpi_tables.ssdts.iter() {
            log::debug!("Reading a ssdt");

            let start = PhysFrame::containing_address(PhysAddr::new(ssdt.address as u64));
            let end = PhysFrame::containing_address(PhysAddr::new(
                ssdt.address as u64 + ssdt.length as u64,
            ));

            for frame in PhysFrame::range_inclusive(start, end) {
                unsafe { acpi_handler.map(frame) }
            }

            let stream = unsafe {
                core::slice::from_raw_parts(ssdt.address as *const _, ssdt.length as usize)
            };

            aml_context
                .parse_table(stream)
                .expect("Failed to parse the dsdt");
        }

        (acpi_tables, aml_context)
    }

    inner()
}
