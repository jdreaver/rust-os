use crate::memory::KernPhysAddr;

/// Location within the PCI Express Enhanced Configuration Mechanism memory
/// region. See "7.2.2 PCI Express Enhanced Configuration Access Mechanism
/// (ECAM)" of the PCI Express Base Specification, as well as
/// <https://wiki.osdev.org/PCI_Express>.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PCIDeviceLocation {
    /// Physical address where the PCI Express extended configuration mechanism
    /// memory region starts for this device.
    pub(crate) ecam_base_address: KernPhysAddr,

    /// Which PCIe bus this device is on.
    pub(crate) bus_number: u8,

    /// Device number of the device within the bus.
    pub(crate) device_number: u8,

    /// Function number of the device if the device is a multifunction device.
    pub(crate) function_number: u8,
}

impl PCIDeviceLocation {
    pub(crate) fn device_base_address(&self) -> KernPhysAddr {
        let bus = u64::from(self.bus_number);
        let device = u64::from(self.device_number);
        let function = u64::from(self.function_number);
        self.ecam_base_address + ((bus << 20) | (device << 15) | (function << 12))
    }
}
