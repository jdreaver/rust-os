use core::fmt::{self, Write};

use x86_64::structures::paging::mapper::MapToError;
use x86_64::structures::paging::{FrameAllocator, Mapper, PageTableFlags, PhysFrame, Size4KiB};
use x86_64::PhysAddr;

use crate::pci::{
    self, BARAddress, PCIDeviceCapabilityHeader, PCIDeviceConfigBodyType0, PCIeDeviceConfig,
};
use crate::register_struct;
use crate::registers::{RegisterRO, RegisterRW};
use crate::strings::IndentWriter;

/// Temporary function for debugging how we get VirtIO information.
pub fn print_virtio_device<W: Write>(
    w: &mut W,
    device: &PCIeDeviceConfig,
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
    // TODO: Move everything from here down into a "VirtIODevice" type

    let header = device.header();
    assert_eq!(
        header.vendor_id(),
        0x1af4,
        "invalid vendor ID, not a VirtIO device"
    );

    let pci::PCIDeviceConfigBody::GeneralDevice(body) = device
            .body()
            .expect("failed to read device body")
            else { return; };

    let w = &mut IndentWriter::new(w, 2);

    writeln!(w, "Found VirtIO device: {header:?}").expect("failed to write");
    w.indent();

    for (i, capability) in body.iter_capabilities().enumerate() {
        writeln!(w, "VirtIO Capability {i}:").expect("failed to write");
        w.indent();

        let virtio_cap =
            unsafe { VirtIOPCICapabilityHeader::from_pci_capability(body, &capability) };
        virtio_cap
            .print(w)
            .expect("failed to print VirtIO capability header");

        // The PCI config type is a way to access the configuration over PCI
        // (not PCI Express, which is the memory mapped method we are using).
        // Just skip it, because this requires accessing the capability config
        // over I/O, which we don't support. See "4.1.4.9 PCI configuration
        // access capability" in the spec.
        if virtio_cap.config_type() == VirtIOPCIConfigType::PCI {
            w.unindent();
            continue;
        }

        let config = virtio_cap.config(mapper, frame_allocator);
        match config {
            VirtIOConfig::Common(cfg) => {
                writeln!(w, "VirtIO Common Config: {cfg:#x?}").expect("failed to write");
            }
            VirtIOConfig::Notify => {
                writeln!(w, "VirtIO Notify Config: TODO").expect("failed to write");
            }
            VirtIOConfig::ISR => {
                writeln!(w, "VirtIO ISR Config: TODO").expect("failed to write");
            }
            VirtIOConfig::Device => {
                writeln!(w, "VirtIO Device Config: TODO").expect("failed to write");
            }
            VirtIOConfig::PCI => {
                writeln!(w, "VirtIO PCI Config: TODO").expect("failed to write");
            }
            VirtIOConfig::SharedMemory => {
                writeln!(w, "VirtIO Shared Memory Config: TODO").expect("failed to write");
            }
            VirtIOConfig::Vendor => {
                writeln!(w, "VirtIO Vendor Config: TODO").expect("failed to write");
            }
        }

        w.unindent();
    }

    w.unindent();
}

#[derive(Debug, Clone, Copy)]
pub struct VirtIOPCICapabilityHeader {
    /// The body of the PCID device for this VirtIO device.
    device_config_body: PCIDeviceConfigBodyType0,

    registers: VirtIOPCICapabilityHeaderRegisters,
}

impl VirtIOPCICapabilityHeader {
    /// # Safety
    ///
    /// Caller must ensure that the capability header is from a VirtIO device.
    pub unsafe fn from_pci_capability(
        device_config_body: PCIDeviceConfigBodyType0,
        header: &PCIDeviceCapabilityHeader,
    ) -> Self {
        Self {
            device_config_body,
            registers: VirtIOPCICapabilityHeaderRegisters::from_address(header.address()),
        }
    }

    fn bar_address(self) -> BARAddress {
        let bar = self.registers.bar().read();
        self.device_config_body.bar(bar as usize)
    }

    fn config_type(self) -> VirtIOPCIConfigType {
        let cfg_type = self.registers.cfg_type().read();
        VirtIOPCIConfigType::from_cfg_type(cfg_type).expect("invalid VirtIO config type")
    }

    /// Returns the VirtIO device configuration associated with this capability
    /// header.
    fn config(
        self,
        mapper: &mut impl Mapper<Size4KiB>,
        frame_allocator: &mut impl FrameAllocator<Size4KiB>,
    ) -> VirtIOConfig {
        let config_type = self.config_type();

        let bar_phys_addr = match self.bar_address() {
            // TODO: Use the prefetchable field when doing mapping.
            pci::BARAddress::Mem32Bit {
                address,
                prefetchable: _,
            } => PhysAddr::new(u64::from(address)),
            pci::BARAddress::Mem64Bit {
                address,
                prefetchable: _,
            } => PhysAddr::new(address),
            pci::BARAddress::IO(address) => panic!(
                "VirtIO capability {:?} uses I/O BAR (address: {:#x}), not supported",
                config_type, address,
            ),
        };

        // Need to identity map the BAR target page(s) so we can access them
        // without faults. Note that these addresses can be outside of physical
        // memory, in which case they are intercepted by the PCI bus and handled
        // by the device, so we aren't mapping physical RAM pages here, we are
        // just ensuring these addresses are identity mapped in the page table
        // so they don't fault.
        let config_addr = bar_phys_addr + u64::from(self.registers.offset().read());
        let config_start_frame = PhysFrame::<Size4KiB>::containing_address(config_addr);
        let config_end_frame =
            PhysFrame::containing_address(config_addr + u64::from(self.registers.cap_len().read()));
        let frame_range = PhysFrame::range_inclusive(config_start_frame, config_end_frame);
        for frame in frame_range {
            let map_result = unsafe {
                mapper.identity_map(
                    frame,
                    PageTableFlags::PRESENT | PageTableFlags::WRITABLE,
                    frame_allocator,
                )
            };
            match map_result {
                // These errors are okay. They just mean the frame is already
                // identity mapped (well, hopefully).
                Ok(_) | Err(MapToError::ParentEntryHugePage | MapToError::PageAlreadyMapped(_)) => {
                }
                Err(e) => panic!("failed to map VirtIO device config page: {:?}", e),
            }
        }

        unsafe { VirtIOConfig::from_address(config_type, config_addr) }
    }

    pub fn print<W: Write>(&self, w: &mut IndentWriter<W>) -> fmt::Result {
        writeln!(w, "VirtIO PCI capability header:")?;

        w.indent();

        let cap_vndr = self.registers.cap_vndr().read();
        writeln!(w, "cap_vndr: {cap_vndr:#x}")?;
        let cap_next = self.registers.cap_next().read();
        writeln!(w, "cap_next: {cap_next:#x}")?;
        let cap_len = self.registers.cap_len().read();
        writeln!(w, "cap_len: {cap_len:#x}")?;

        let cfg_type = self.registers.cfg_type().read();
        let config_type = self.config_type();
        writeln!(w, "cfg_type: {cfg_type:#x} ({config_type:?})")?;

        let bar = self.registers.bar().read();
        writeln!(w, "bar_index: {bar:#x}")?;

        let bar_address = self.bar_address();
        writeln!(w, "bar address: {bar_address:#x?}")?;

        let id = self.registers.id().read();
        writeln!(w, "id: {id:#x}")?;

        let offset = self.registers.offset().read();
        writeln!(w, "offset: {offset:#x}")?;

        let length = self.registers.length().read();
        writeln!(w, "length: {length:#x}")?;
        w.unindent();

        Ok(())
    }
}

register_struct!(
    /// See 4.1.4 Virtio Structure PCI Capabilities in spec
    VirtIOPCICapabilityHeaderRegisters {
        // TODO: Support field docstrings in register_struct! macro.
        // This should equal 0x9, which is the PCI capability ID meaning "vendor
        // specific".
        0x00 => cap_vndr: RegisterRO<u8>,
        0x01 => cap_next: RegisterRO<u8>,
        0x02 => cap_len: RegisterRO<u8>,
        0x03 => cfg_type: RegisterRO<u8>,
        0x04 => bar: RegisterRO<u8>,
        0x05 => id: RegisterRO<u8>,

        // 2 bytes of padding

        0x08 => offset: RegisterRO<u32>,
        // Length of the entire capability structure, in bytes.
        0x12 => length: RegisterRO<u32>,
    }
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VirtIOPCIConfigType {
    Common = 1,
    Notify = 2,
    ISR = 3,
    Device = 4,
    PCI = 5,
    SharedMemory = 8,
    Vendor = 9,
}

impl VirtIOPCIConfigType {
    fn from_cfg_type(cfg_type: u8) -> Option<Self> {
        match cfg_type {
            1 => Some(Self::Common),
            2 => Some(Self::Notify),
            3 => Some(Self::ISR),
            4 => Some(Self::Device),
            5 => Some(Self::PCI),
            8 => Some(Self::SharedMemory),
            9 => Some(Self::Vendor),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum VirtIOConfig {
    Common(VirtIOPCICommonConfigRegisters),
    Notify,
    ISR,
    Device,
    PCI,
    SharedMemory,
    Vendor,
}

impl VirtIOConfig {
    /// # Safety
    ///
    /// Caller must ensure that the given BAR (base address register) is valid
    /// and is for the VirtIO device.
    pub unsafe fn from_address(config_type: VirtIOPCIConfigType, config_addr: PhysAddr) -> Self {
        match config_type {
            VirtIOPCIConfigType::Common => Self::Common(
                VirtIOPCICommonConfigRegisters::from_address(config_addr.as_u64() as usize),
            ),
            VirtIOPCIConfigType::Notify => Self::Notify,
            VirtIOPCIConfigType::ISR => Self::ISR,
            VirtIOPCIConfigType::Device => Self::Device,
            VirtIOPCIConfigType::PCI => Self::PCI,
            VirtIOPCIConfigType::SharedMemory => Self::SharedMemory,
            VirtIOPCIConfigType::Vendor => Self::Vendor,
        }
    }
}

register_struct!(
    /// 4.1.4.3 Common configuration structure layout
    VirtIOPCICommonConfigRegisters {
        0x00 => device_feature_select: RegisterRW<u32>,
        0x04 => device_feature: RegisterRO<u32>,
        0x08 => driver_feature_select: RegisterRW<u32>,
        0x12 => driver_feature: RegisterRW<u32>,
        0x10 => msix_config: RegisterRW<u16>,
        0x12 => num_queues: RegisterRO<u16>,
        0x14 => device_status: RegisterRW<u8>,
        0x15 => config_generation: RegisterRO<u8>,
        0x16 => queue_select: RegisterRW<u16>,
        0x18 => queue_size: RegisterRW<u16>,
        0x1A => queue_msix_vector: RegisterRW<u16>,
        0x1C => queue_enable: RegisterRW<u16>,
        0x1E => queue_notify_off: RegisterRO<u16>,
        0x20 => queue_desc: RegisterRW<u64>,
        0x28 => queue_driver: RegisterRW<u64>,
        0x30 => queue_device: RegisterRW<u64>,
        0x38 => queue_notify_data: RegisterRO<u16>,
        0x3A => queue_reset: RegisterRW<u16>,
    }
);
