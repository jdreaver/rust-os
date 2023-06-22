use core::ops::{Add, Sub};

use x86_64::{PhysAddr, VirtAddr};

use super::mapping::{KERNEL_PHYSICAL_MAPPING_END, KERNEL_PHYSICAL_MAPPING_START};

/// Physical address that has been mapped to the kernel physical address space.
/// A `KernPhysAddr` is trivially convertible to and from a `PhysAddr` by using
/// the `KERNEL_PHYSICAL_MAPPING_START` offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub(crate) struct KernPhysAddr(u64);

impl KernPhysAddr {
    pub(crate) fn new(addr: u64) -> Self {
        assert!(
            (KERNEL_PHYSICAL_MAPPING_START..=KERNEL_PHYSICAL_MAPPING_END).contains(&addr),
            "physical address {addr:#x} is not in kernel physical mapping range"
        );
        Self(addr)
    }

    pub(crate) fn as_u64(self) -> u64 {
        self.0
    }

    pub(crate) fn to_phys_addr(self) -> PhysAddr {
        let addr = self.0;
        assert!(
            (KERNEL_PHYSICAL_MAPPING_START..=KERNEL_PHYSICAL_MAPPING_END).contains(&addr),
            "physical address {addr:#x} is not in kernel physical mapping range"
        );
        PhysAddr::new(addr - KERNEL_PHYSICAL_MAPPING_START)
    }

    pub(crate) fn from_phys_addr(addr: PhysAddr) -> Self {
        Self::new(addr.as_u64() + KERNEL_PHYSICAL_MAPPING_START)
    }

    pub(crate) fn align_down(self, align: u64) -> Self {
        Self(x86_64::align_down(self.0, align))
    }
}

impl From<KernPhysAddr> for VirtAddr {
    fn from(addr: KernPhysAddr) -> Self {
        Self::new(addr.0)
    }
}

impl From<KernPhysAddr> for PhysAddr {
    fn from(addr: KernPhysAddr) -> Self {
        addr.to_phys_addr()
    }
}

impl From<PhysAddr> for KernPhysAddr {
    fn from(addr: PhysAddr) -> Self {
        Self::from_phys_addr(addr)
    }
}

impl Add<u64> for KernPhysAddr {
    type Output = Self;

    fn add(self, rhs: u64) -> Self::Output {
        Self::new(self.0 + rhs)
    }
}

impl Sub<u64> for KernPhysAddr {
    type Output = Self;

    fn sub(self, rhs: u64) -> Self::Output {
        Self::new(self.0 - rhs)
    }
}

impl Add<usize> for KernPhysAddr {
    type Output = Self;

    fn add(self, rhs: usize) -> Self::Output {
        Self::new(self.0 + rhs as u64)
    }
}

impl Sub<usize> for KernPhysAddr {
    type Output = Self;

    fn sub(self, rhs: usize) -> Self::Output {
        Self::new(self.0 - rhs as u64)
    }
}
