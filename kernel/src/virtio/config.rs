use core::fmt;

use bitfield_struct::bitfield;
use bitflags::Flags;
use x86_64::PhysAddr;

use crate::barrier::barrier;
use crate::memory::KernPhysAddr;
use crate::pci::{
    PCIDeviceCapability, PCIDeviceCapabilityHeader, PCIDeviceConfig, PCIDeviceConfigType0,
    PCIDeviceConfigTypes,
};
use crate::register_struct;
use crate::registers::{RegisterRO, RegisterROSideEffect, RegisterRW};
use crate::serial_println;

use super::features::Features;
use super::queue::VirtQueueIndex;

/// Holds the configuration for a VirtIO device.
#[derive(Debug, Clone)]
pub(crate) struct VirtIODeviceConfig {
    /// Common PCI configuration registers.
    pci_config: PCIDeviceConfig,

    /// Registers specifically for type 0 devices (which all VirtIO devices
    /// are).
    pci_type0_config: PCIDeviceConfigType0,

    common_virtio_config: VirtIOPCICommonConfigRegisters,

    /// See "4.1.4.5.2 Driver Requirements: ISR status capability". Note that we
    /// should never access this with MSI-X enabled!:
    ///
    /// > If MSI-X capability is enabled, the driver SHOULD NOT access ISR status upon detecting a Queue Interrupt.
    _isr: VirtIOPCIISRRegisters,

    notify_config: VirtIONotifyConfig,

    device_config_phys_addr: KernPhysAddr,
}

impl VirtIODeviceConfig {
    pub(crate) fn from_pci_config(pci_config: PCIDeviceConfig) -> Option<Self> {
        // Check that this is a VirtIO device.
        let vendor_id = pci_config.device_id().registers().vendor_id().read();
        if vendor_id != 0x1af4 {
            return None;
        };

        let config_type = pci_config
            .config_type()
            .expect("failed to read device config type");
        let PCIDeviceConfigTypes::GeneralDevice(pci_type0_config) = config_type else {
            panic!("invalid VirtIO device config type, expected Type 0");
        };

        // Scan capabilities to record the ones we need
        let mut common_virtio_config = None;
        let mut isr = None;
        let mut notify_config = None;
        let mut device_config_phys_addr = None;
        for capability in pci_config.iter_capabilities() {
            let capability = unsafe {
                VirtIOPCICapabilityHeader::from_pci_capability(&pci_type0_config, &capability)
            };
            let Some(capability) = capability else { continue; };

            // The PCI config type is a way to access the configuration over PCI
            // (not PCI Express, which is the memory mapped method we are using).
            // Just skip it, because this requires accessing the capability config
            // over I/O, which we don't support. See "4.1.4.9 PCI configuration
            // access capability" in the spec.
            if capability.config_type() == VirtIOPCIConfigType::PCI {
                continue;
            }

            let config = capability.config();

            // N.B. The VirtIO spec says that capabilities of the same type
            // should be ordered by preference. It also says "The driver
            // SHOULD use the first instance of each virtio structure type
            // they can support." That means we take the first instance of
            // each type we find, hence the use of `get_or_insert`.
            match config {
                VirtIOConfig::Common(cfg) => {
                    common_virtio_config.get_or_insert(cfg);
                }
                VirtIOConfig::Notify(cfg) => {
                    notify_config.get_or_insert(cfg);
                }
                VirtIOConfig::ISR(isr_regs) => {
                    isr.get_or_insert(isr_regs);
                }
                VirtIOConfig::Device(addr) => {
                    device_config_phys_addr.get_or_insert(addr);
                }
                VirtIOConfig::PCI => {
                    serial_println!("VirtIO PCI config found: {:#x?}", capability);
                }
                VirtIOConfig::SharedMemory => {
                    serial_println!("VirtIO SharedMemory config found: {:#x?}", capability);
                }
                VirtIOConfig::Vendor => {
                    serial_println!("VirtIO Vendor config found: {:#x?}", capability);
                }
            }
        }

        let common_virtio_config =
            common_virtio_config.expect("failed to find VirtIO common config");
        let isr = isr.expect("failed to find VirtIO ISR");
        let notify_config = notify_config.expect("failed to find VirtIO notify config");
        let device_config_phys_addr =
            device_config_phys_addr.expect("failed to find VirtIO device config");

        Some(Self {
            pci_config,
            pci_type0_config,
            common_virtio_config,
            _isr: isr,
            notify_config,
            device_config_phys_addr,
        })
    }

    pub(super) fn pci_config(&self) -> &PCIDeviceConfig {
        &self.pci_config
    }

    pub(super) fn pci_type0_config(&self) -> &PCIDeviceConfigType0 {
        &self.pci_type0_config
    }

    pub(super) fn common_virtio_config(&self) -> VirtIOPCICommonConfigRegisters {
        self.common_virtio_config
    }

    pub(super) fn notify_config(&self) -> VirtIONotifyConfig {
        self.notify_config
    }

    pub(super) fn device_config_phys_addr(&self) -> KernPhysAddr {
        self.device_config_phys_addr
    }

    /// Iterates through `device_feature_select` and `device_feature` to find
    /// the device features.
    pub(super) fn get_device_features<F>(&self) -> Features<F>
    where
        F: Flags<Bits = u128>,
    {
        let mut features: u128 = 0;
        for i in 0..4_u32 {
            self.common_virtio_config.device_feature_select().write(i);
            barrier();
            let mut feature_bits = u128::from(self.common_virtio_config.device_feature().read());
            feature_bits <<= i * 32;
            features |= feature_bits;
            barrier();
        }
        Features::new(features)
    }

    /// Iterates through `driver_feature_select` and `driver_feature` to set
    /// the driver features.
    pub(super) fn set_driver_features<F>(&self, features: &Features<F>)
    where
        F: Flags<Bits = u128>,
    {
        let bits = features.as_u128();
        for i in 0..4_u32 {
            self.common_virtio_config.driver_feature_select().write(i);
            barrier();
            let feature_bits = (bits >> (i * 32)) as u32;
            self.common_virtio_config
                .driver_feature()
                .write(feature_bits);
            barrier();
        }
    }
}

#[derive(Clone)]
pub(super) struct VirtIOPCICapabilityHeader {
    /// The body of the PCID device for this VirtIO device.
    device_config_body: PCIDeviceConfigType0,

    registers: VirtIOPCICapabilityHeaderRegisters,
}

impl fmt::Debug for VirtIOPCICapabilityHeader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VirtIOPCICapabilityHeader")
            .field("config_type", &self.config_type())
            .field("registers", &self.registers)
            .finish_non_exhaustive()
    }
}

impl VirtIOPCICapabilityHeader {
    /// # Safety
    ///
    /// Caller must ensure that the capability header is from a VirtIO device.
    pub(super) unsafe fn from_pci_capability(
        device_config_body: &PCIDeviceConfigType0,
        header: &PCIDeviceCapabilityHeader,
    ) -> Option<Self> {
        // VirtIO-specific capabilities must have an ID for vendor-specific.
        let PCIDeviceCapability::VendorSpecific(_) = header.capability() else { return None; };

        Some(Self {
            // TODO: Remove need for clone() here
            device_config_body: device_config_body.clone(),
            registers: VirtIOPCICapabilityHeaderRegisters::from_address(header.address()),
        })
    }

    fn config_type(&self) -> VirtIOPCIConfigType {
        let cfg_type = self.registers.cfg_type().read();
        VirtIOPCIConfigType::from_cfg_type(cfg_type).expect("invalid VirtIO config type")
    }

    /// Returns the VirtIO device configuration associated with this capability
    /// header.
    fn config(&self) -> VirtIOConfig {
        match self.config_type() {
            VirtIOPCIConfigType::Common => VirtIOConfig::Common(unsafe {
                let config_addr = self.compute_and_map_config_address();
                VirtIOPCICommonConfigRegisters::from_address(config_addr)
            }),
            VirtIOPCIConfigType::Notify => VirtIOConfig::Notify({
                let config_addr = self.compute_and_map_config_address();

                // Per 4.1.4.4 Notification structure layout, the notify
                // configuration is in the capabilities struct and the notify
                // offset multiplier is right after the capabilities struct.
                let notify_off_ptr =
                    (self.registers.address + VIRTIO_CAPABILITY_HEADER_SIZE).as_ptr::<u32>();
                let notify_off_multiplier = unsafe { *notify_off_ptr };

                VirtIONotifyConfig {
                    config_addr,
                    notify_off_multiplier,
                }
            }),
            VirtIOPCIConfigType::ISR => VirtIOConfig::ISR(unsafe {
                let config_addr = self.compute_and_map_config_address();
                VirtIOPCIISRRegisters::from_address(config_addr)
            }),
            VirtIOPCIConfigType::Device => {
                VirtIOConfig::Device(self.compute_and_map_config_address())
            }
            VirtIOPCIConfigType::PCI => VirtIOConfig::PCI,
            VirtIOPCIConfigType::SharedMemory => VirtIOConfig::SharedMemory,
            VirtIOPCIConfigType::Vendor => VirtIOConfig::Vendor,
        }
    }

    /// Compute and map physical address for VirtIO capabilities that need to
    /// reach through a BAR to access their configuration.
    pub(super) fn compute_and_map_config_address(&self) -> KernPhysAddr {
        let bar_idx = self.registers.bar().read();
        let physical_offset = self.registers.offset().read();
        let region_size = u64::from(self.registers.cap_len().read());
        self.device_config_body
            .bar_region_physical_address(bar_idx, physical_offset, region_size)
    }
}

/// Ensure this matches the size of the VirtIO capability header! (See
/// `VirtIOPCICapabilityHeaderRegisters`.)
const VIRTIO_CAPABILITY_HEADER_SIZE: usize = 16;

register_struct!(
    /// See 4.1.4 Virtio Structure PCI Capabilities in spec
    pub(super) VirtIOPCICapabilityHeaderRegisters {
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
        0x0C => length: RegisterRO<u32>,
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

#[derive(Debug, Clone)]
enum VirtIOConfig {
    Common(VirtIOPCICommonConfigRegisters),
    Notify(VirtIONotifyConfig),
    ISR(VirtIOPCIISRRegisters),
    Device(KernPhysAddr),
    PCI,
    SharedMemory,
    Vendor,
}

register_struct!(
    /// 4.1.4.3 Common configuration structure layout
    pub(super) VirtIOPCICommonConfigRegisters {
        0x00 => device_feature_select: RegisterRW<u32>,
        0x04 => device_feature: RegisterRO<u32>,
        0x08 => driver_feature_select: RegisterRW<u32>,
        0x0C => driver_feature: RegisterRW<u32>,
        0x10 => config_msix_vector: RegisterRW<u16>,
        0x12 => num_queues: RegisterRO<u16>,
        0x14 => device_status: RegisterRW<VirtIOConfigStatus>,
        0x15 => config_generation: RegisterRO<u8>,

        0x16 => queue_select: RegisterRW<VirtQueueIndex>,
        0x18 => queue_size: RegisterRW<u16>,
        0x1A => queue_msix_vector: RegisterRW<u16>,
        0x1C => queue_enable: RegisterRW<u16>,
        0x1E => queue_notify_off: RegisterRO<u16>,
        0x20 => queue_desc: RegisterRW<PhysAddr>,
        0x28 => queue_driver: RegisterRW<PhysAddr>,
        0x30 => queue_device: RegisterRW<PhysAddr>,
        0x38 => queue_notify_data: RegisterRO<u16>,
        0x3A => queue_reset: RegisterRW<u16>,
    }
);

#[bitfield(u8)]
/// 2.1 Device Status Field
pub(super) struct VirtIOConfigStatus {
    /// ACKNOWLEDGE (1) Indicates that the guest OS has found the device and
    /// recognized it as a valid virtio device.
    pub(super) acknowledge: bool,

    /// DRIVER (2) Indicates that the guest OS knows how to drive the device.
    pub(super) driver: bool,

    /// DRIVER_OK (4) Indicates that the guest OS knows how to drive the device.
    pub(super) driver_ok: bool,

    /// FEATURES_OK (8) Indicates that the features negotiated by the driver are
    /// acceptable to the device. This bit is optional since not all devices
    /// support feature negotiation, and some devices may accept any subset of
    /// the features offered by the driver.
    pub(super) features_ok: bool,

    __reserved: bool,
    __reserved: bool,

    /// DEVICE_NEEDS_RESET (64) Indicates that the device has experienced an
    /// error from which it can’t recover. The device has stopped working. The
    /// driver should not send any further requests to the device, and should
    /// reset the device at the earliest convenience.
    pub(super) device_needs_reset: bool,

    /// FAILED (128) Indicates that something went wrong in the guest, and it
    /// has given up on the device. This could be an internal error, or the
    /// driver didn’t like the device for some reason, or even a fatal error
    /// during device operation. The device should not be used any further
    /// without a reset.
    pub(super) failed: bool,
}

register_struct!(
    /// 4.1.4.5 ISR status capability
    ///
    /// Reading the ISR has side effects, so we mark it as such. Also, we should
    /// never read this value if MSI-X is enabled.
    ///
    /// > If MSI-X capability is enabled, the driver SHOULD NOT access ISR status upon detecting a Queue Interrupt.
    pub(super) VirtIOPCIISRRegisters {
        0x00 => isr: RegisterROSideEffect<VirtIOISRStatus>,
    }
);

#[bitfield(u32)]
/// 4.1.4.5 ISR status capability
pub(super) struct VirtIOISRStatus {
    queue_interrupt: bool,
    device_config_interrupt: bool,

    #[bits(30)]
    __reserved: u32,
}

/// 4.1.4.4 Notification structure layout
#[derive(Debug, Clone, Copy)]
pub(super) struct VirtIONotifyConfig {
    /// Physical address for the configuration area for the Notify capability
    /// (BAR + offset has already been applied).
    config_addr: KernPhysAddr,

    notify_off_multiplier: u32,
}

impl VirtIONotifyConfig {
    /// 4.1.4.4 Notification structure layout
    fn queue_notify_address(&self, queue_notify_offset: u16) -> KernPhysAddr {
        let offset = u64::from(queue_notify_offset) * u64::from(self.notify_off_multiplier);
        self.config_addr + offset
    }

    /// 4.1.5.2 Available Buffer Notifications: When VIRTIO_F_NOTIFICATION_DATA
    /// has not been negotiated, the driver sends an available buffer
    /// notification to the device by writing the 16-bit virtqueue index of this
    /// virtqueue to the Queue Notify address.
    ///
    /// # Safety
    ///
    /// Caller must ensure that `queue_notify_offset` and `queue_index` are
    /// valid.
    pub(super) unsafe fn notify_device(
        &self,
        queue_notify_offset: u16,
        queue_index: VirtQueueIndex,
    ) {
        // TODO: Check if VIRTIO_F_NOTIFICATION_DATA has been negotiated or not.
        // This function currently assumes it has been negotiated.

        // 4.1.5.2 Available Buffer Notifications: When
        // VIRTIO_F_NOTIFICATION_DATA has not been negotiated, the driver sends
        // an available buffer notification to the device by writing the 16-bit
        // virtqueue index of this virtqueue to the Queue Notify address.
        let notify_addr = self.queue_notify_address(queue_notify_offset);
        let notify_ptr = notify_addr.as_mut_ptr::<VirtQueueIndex>();
        unsafe {
            notify_ptr.write_volatile(queue_index);
        }
    }
}
