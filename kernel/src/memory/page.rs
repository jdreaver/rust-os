use core::fmt;
use core::ops::{Add, Sub};

use x86_64::{PhysAddr, VirtAddr};

use super::address::KernPhysAddr;

/// A `Page` is a page of memory of a given address type `A` (e.g. `VirtAddr`,
/// `PhysAddr`, etc).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Page<A> {
    start_addr: A,
    size: PageSize,
}

impl<A: Address + fmt::Debug> Page<A> {
    pub(crate) fn from_start_addr(start_addr: A, size: PageSize) -> Self {
        assert!(
            start_addr.is_aligned(size.size_bytes() as u64),
            "start_addr {start_addr:x?} is not aligned to page size {size:?}"
        );
        Self { start_addr, size }
    }

    pub(crate) fn containing_address(addr: A, size: PageSize) -> Self {
        let start_addr = addr.align_down(size.size_bytes() as u64);
        Self { start_addr, size }
    }

    pub(crate) fn start_addr(&self) -> A {
        self.start_addr
    }

    pub(crate) fn size(&self) -> PageSize {
        self.size
    }
}

impl<A: AsMutPtr + Copy> Page<A> {
    pub(crate) fn zero(&mut self) {
        let start_ptr = self.start_addr.as_mut_ptr::<u8>();
        let size_bytes = self.size.size_bytes();
        unsafe {
            // N.B. `write_bytes` is a highly optimized way to write zeroes.
            // Making a slice and doing `slice.fill(0)` is supposed to optimize
            // to this, but it doesn't seem to when compiling in debug mode and
            // it makes page table allocation very slow.
            start_ptr.write_bytes(0, size_bytes);
        }
    }
}

impl Page<VirtAddr> {
    pub(crate) fn flush(&self) {
        x86_64::instructions::tlb::flush(self.start_addr);
    }
}

#[derive(Debug)]
pub(crate) struct PageRange<A> {
    start: Page<A>,
    num_pages: usize,
}

impl<A: Address> PageRange<A> {
    pub(crate) fn new(start: Page<A>, num_pages: usize) -> Self {
        Self { start, num_pages }
    }

    pub(crate) fn from_bytes_inclusive(start: Page<A>, num_bytes: usize) -> Self {
        let num_pages = num_bytes.div_ceil(start.size.size_bytes());
        Self { start, num_pages }
    }

    pub(crate) fn start_addr(&self) -> A {
        self.start.start_addr
    }

    pub(crate) fn page_size(&self) -> PageSize {
        self.start.size
    }

    pub(crate) fn num_pages(&self) -> usize {
        self.num_pages
    }

    pub(crate) fn iter(&self) -> PageRangeIter<A> {
        PageRangeIter {
            range: self,
            current_page: 0,
        }
    }
}

impl<A: AsMutPtr + Copy> PageRange<A> {
    pub(crate) fn zero(&mut self) {
        let start_ptr = self.start.start_addr.as_mut_ptr::<u8>();
        let size_bytes = self.num_pages * self.start.size.size_bytes();
        unsafe {
            // N.B. `write_bytes` is a highly optimized way to write zeroes.
            // Making a slice and doing `slice.fill(0)` is supposed to optimize
            // to this, but it doesn't seem to when compiling in debug mode and
            // it makes page table allocation very slow.
            start_ptr.write_bytes(0, size_bytes);
        }
    }
}

#[derive(Debug)]
pub(crate) struct PageRangeIter<'a, A> {
    range: &'a PageRange<A>,
    current_page: usize,
}

impl<'a, A: Address> Iterator for PageRangeIter<'a, A> {
    type Item = Page<A>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current_page >= self.range.num_pages {
            return None;
        }

        let start_addr =
            self.range.start.start_addr + self.current_page * self.range.start.size.size_bytes();
        let page = Page {
            start_addr,
            size: self.range.start.size,
        };

        self.current_page += 1;
        Some(page)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageSize {
    Size4KiB,
    Size2MiB,
    Size1GiB,
}

impl PageSize {
    pub(crate) fn size_bytes(self) -> usize {
        match self {
            Self::Size4KiB => 4096,
            Self::Size2MiB => 2 * 1024 * 1024,
            Self::Size1GiB => 1024 * 1024 * 1024,
        }
    }
}

/// The `Address` trait abstracts over different address types.
pub(crate) trait Address:
    Copy + Sized + PartialOrd + PartialEq + Eq + Add<usize, Output = Self> + Sub<usize, Output = Self>
{
    fn as_u64(self) -> u64;

    fn align_down(self, align: u64) -> Self;

    fn is_aligned(self, align: u64) -> bool {
        self.align_down(align) == self
    }
}

impl Address for VirtAddr {
    fn as_u64(self) -> u64 {
        self.as_u64()
    }

    fn align_down(self, align: u64) -> Self {
        self.align_down(align)
    }
}

impl Address for PhysAddr {
    fn as_u64(self) -> u64 {
        self.as_u64()
    }

    fn align_down(self, align: u64) -> Self {
        self.align_down(align)
    }
}

impl Address for KernPhysAddr {
    fn as_u64(self) -> u64 {
        self.as_u64()
    }

    fn align_down(self, align: u64) -> Self {
        self.align_down(align)
    }
}

pub(crate) trait AsMutPtr {
    #[allow(clippy::wrong_self_convention)]
    fn as_mut_ptr<T>(self) -> *mut T;
}

impl AsMutPtr for VirtAddr {
    fn as_mut_ptr<T>(self) -> *mut T {
        Self::as_mut_ptr(self)
    }
}

impl AsMutPtr for KernPhysAddr {
    fn as_mut_ptr<T>(self) -> *mut T {
        Self::as_mut_ptr(self)
    }
}
