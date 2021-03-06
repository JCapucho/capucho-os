use bitflags::bitflags;
use core::{
    fmt::{self, Debug},
    mem::MaybeUninit,
};

pub const ATA_SIGNATURE: u32 = 0x00000101;
pub const ATAPI_SIGNATURE: u32 = 0xEB140101;
pub const SEMB_SIGNATURE: u32 = 0xC33C0101;
pub const PM_SIGNATURE: u32 = 0x96690101;

bitflags! {
    #[repr(C)]
    pub struct HBACapabilities: u32 {
        const SXS_SUPPORT = 1 << 5;
        const EMS_SUPPORT = 1 << 6;
        const CCC_SUPPORT = 1 << 7;
        const PS_CAPABLE = 1 << 13;
        const SS_CAPABLE = 1 << 14;
        const PIO_MULTI_DRQ_SUPPORT = 1 << 15;
        const FBSS_SUPPORT = 1 << 16;
        const PM_SUPPORT = 1 << 17;
        const AHCI_ONLY = 1 << 18;
        const CLO_SUPPORT = 1 << 24;
        const AL_SUPPORT = 1 << 25;
        const ALP_SUPPORT = 1 << 26;
        const SS_SUPPORT = 1 << 27;
        const MPS_SUPPORT = 1 << 28;
        const SNTF_SUPPORT = 1 << 29;
        const NCQ_SUPPORT = 1 << 30;
        const SUPPORTS_64_ADDRESSES = 1 << 31;
    }
}

#[derive(Debug)]
pub enum InterfaceSpeed {
    Gen1,
    Gen2,
    Gen3,
    Reserved,
}

impl From<u32> for InterfaceSpeed {
    fn from(bits: u32) -> Self {
        match bits {
            0 => InterfaceSpeed::Reserved,
            0b0001 => InterfaceSpeed::Gen1,
            0b0010 => InterfaceSpeed::Gen2,
            0b0011 => InterfaceSpeed::Gen3,
            _ => InterfaceSpeed::Reserved,
        }
    }
}

#[derive(Debug)]
pub enum InterfacePowerManagement {
    NoDevice,
    Active,
    Partial,
    Slumber,
    DevSleep,
    Reserved,
}

impl From<u32> for InterfacePowerManagement {
    fn from(bits: u32) -> Self {
        match bits {
            0 => InterfacePowerManagement::NoDevice,
            0b0001 => InterfacePowerManagement::Active,
            0b0010 => InterfacePowerManagement::Partial,
            0b0110 => InterfacePowerManagement::Slumber,
            0b1000 => InterfacePowerManagement::DevSleep,
            _ => InterfacePowerManagement::Reserved,
        }
    }
}

#[derive(Debug)]
pub enum DeviceDetection {
    NoDevice,
    DeviceNoPhy,
    Device,
    InterfaceDisabled,
    Reserved,
}

impl DeviceDetection {
    pub fn has_device(&self) -> bool {
        matches!(self, DeviceDetection::Device | DeviceDetection::DeviceNoPhy)
    }
}

impl From<u32> for DeviceDetection {
    fn from(bits: u32) -> Self {
        match bits {
            0 => DeviceDetection::NoDevice,
            0b0001 => DeviceDetection::DeviceNoPhy,
            0b0011 => DeviceDetection::Device,
            0b0100 => DeviceDetection::InterfaceDisabled,
            _ => DeviceDetection::Reserved,
        }
    }
}

impl HBACapabilities {
    pub fn number_of_ports(&self) -> u8 { (self.bits() & 0b11111) as u8 }

    pub fn number_of_cmd_slots(&self) -> u8 { ((self.bits() >> 8) & 0b11111) as u8 }

    pub fn if_speed(&self) -> InterfaceSpeed { InterfaceSpeed::from((self.bits() >> 20) & 0b1111) }
}

bitflags! {
    pub struct GlobalHBAControl: u32 {
        const HBA_RESET = 1;
        const INT_ENABLE = 1 << 1;
        const MRSM = 1 << 2;
        const AHCI_ENABLE = 1 << 31;
    }
}

#[repr(C)]
pub struct StatusPort(u32);

impl StatusPort {
    pub fn if_speed(&self) -> InterfaceSpeed { InterfaceSpeed::from((self.0 >> 4) & 0b1111) }

    pub fn if_power_management(&self) -> InterfacePowerManagement {
        InterfacePowerManagement::from((self.0 >> 8) & 0b1111)
    }

    pub fn detection(&self) -> DeviceDetection { DeviceDetection::from(self.0 & 0b1111) }
}

impl fmt::Debug for StatusPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.detection().fmt(f)?;
        write!(f, " | ")?;
        self.if_power_management().fmt(f)?;
        write!(f, " | ")?;
        self.if_speed().fmt(f)
    }
}

bitflags! {
    #[repr(C)]
    pub struct PortInterrupt: u32 {
        const D2H_REG_FIS_INT = 1;
        const PIO_SETUP_FIS_INT = 1 << 1;
        const DMA_SETUP_FIS_INT = 1 << 2;
        const SET_DEV_BITS_FIS_INT = 1 << 3;
        const UNKNOWN_FIS_INT = 1 << 4;
        const PRD_PROCESSED_INT = 1 << 5;
        const PORT_CONNECT_CHANGE = 1 << 6;
        const MECHANICAL_PRESENCE = 1 << 7;
        const PHY_READY_CHANGE = 1 << 22;
        const BAD_PORT_MUL_STATUS = 1 << 23;
        const OVERFLOW = 1 << 24;
        const IF_NON_FATAL_ERROR = 1 << 26;
        const IF_FATAL_ERROR = 1 << 27;
        const HOST_DATA_ERROR = 1 << 28;
        const HOST_FATAL_ERROR = 1 << 29;
        const TASK_FILE_ERROR = 1 << 30;
        const COLD_PORT_DETECT = 1 << 31;
    }
}

#[repr(C, packed)]
pub struct HBAPortRegisters {
    clb: u32,
    clbu: u32,
    fb: u32,
    fbu: u32,
    pub int_status: PortInterrupt,
    pub int_enable: PortInterrupt,
    pub cmd: u32,

    reserved_0: u32,

    pub tfd: u32,
    pub sig: u32,
    pub ssts: StatusPort,
    pub sctl: u32,
    pub serr: u32,
    pub sact: u32,
    pub cmd_issue: u32,
    pub sntf: u32,
    pub fbs: u32,

    reserved_1: [u32; 11],
    vendor: [u32; 4],
}

impl HBAPortRegisters {
    pub fn cmd_list_addr(&self) -> u64 { (self.clbu as u64) << 32 | self.clb as u64 }

    /// # Safety
    /// The caller must assure that the address is only 64bit
    /// if the ahci supports it and points to usable memory
    pub unsafe fn set_cmd_list_addr(&mut self, addr: u64) {
        assert_eq!(addr & 0x3FF, 0, "Address must be 1K aligned");

        self.clb = addr as u32;
        self.clbu = (addr >> 32) as u32;
    }

    pub fn fis_addr(&self) -> u64 { (self.fbu as u64) << 32 | self.fb as u64 }

    /// # Safety
    /// The caller must assure that the address is only 64bit
    /// if the ahci supports it and points to usable memory
    pub unsafe fn set_fb_list_addr(&mut self, addr: u64) {
        assert_eq!(addr & 0x3FF, 0, "Address must be 1K aligned");

        self.fb = addr as u32;
        self.fbu = (addr >> 32) as u32;
    }
}

#[repr(C, packed)]
pub struct HBAMemoryRegisters {
    pub cap: HBACapabilities,
    pub ghc: GlobalHBAControl,
    pub int_status: u32,
    pub port_implemented: u32,
    pub version: u32,
    pub ccc_ctl: u32,
    pub ccc_ports: u32,
    pub em_loc: u32,
    pub em_ctl: u32,
    pub cap_ext: u32,
    pub bohc: u32,

    reserved_0: [u8; 116],

    vendor: [u8; 96],

    ports: [MaybeUninit<HBAPortRegisters>; 32],
}

impl HBAMemoryRegisters {
    pub fn get_port(&self, idx: u32) -> Option<&HBAPortRegisters> {
        assert!(idx < 32, "There are only 32 ports");

        let bit = self.port_implemented >> idx;
        if bit & 1 == 1 {
            let ptr = self.ports[idx as usize].as_ptr();
            Some(unsafe { &*ptr })
        } else {
            None
        }
    }

    pub fn get_port_mut(&mut self, idx: u32) -> Option<&mut HBAPortRegisters> {
        assert!(idx < 32, "There are only 32 ports");

        let bit = self.port_implemented >> idx;
        if bit & 1 == 1 {
            let ptr = self.ports[idx as usize].as_mut_ptr();
            Some(unsafe { &mut *ptr })
        } else {
            None
        }
    }

    pub fn port_count(&self) -> u32 { self.port_implemented.count_ones() }

    pub fn port_slice(&self) -> &[HBAPortRegisters] {
        let count = self.port_count() as usize;
        let slice = &self.ports[..count];

        unsafe { MaybeUninit::slice_assume_init_ref(slice) }
    }

    pub fn port_slice_mut(&mut self) -> &mut [HBAPortRegisters] {
        let count = self.port_count() as usize;
        let slice = &mut self.ports[..count];

        unsafe { MaybeUninit::slice_assume_init_mut(slice) }
    }
}
