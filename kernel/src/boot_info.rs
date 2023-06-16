use core::cmp::max;

use spin::Once;

use limine::{
    LimineBootInfoRequest, LimineEfiSystemTableRequest, LimineFramebufferRequest,
    LimineHhdmRequest, LimineKernelAddressRequest, LimineMemmapRequest, LimineMemoryMapEntryType,
    LimineRsdpRequest, LimineSmpRequest, NonNullPtr,
};
use x86_64::{PhysAddr, VirtAddr};

use crate::{serial_println, strings};

static BOOT_INFO_ONCE: Once<BootInfo> = Once::new();

#[derive(Debug)]
pub(crate) struct BootInfo {
    pub(crate) _info_name: &'static str,
    pub(crate) _info_version: &'static str,
    pub(crate) higher_half_direct_map_offset: VirtAddr,
    pub(crate) _kernel_address_physical_base: PhysAddr,
    pub(crate) _kernel_address_virtual_base: VirtAddr,
    pub(crate) efi_system_table_address: Option<VirtAddr>,
    rsdp_address: Option<VirtAddr>,
    pub(crate) framebuffer: &'static mut limine::LimineFramebuffer,
    pub(crate) _x2apic_enabled: bool,
    pub(crate) bootstrap_processor_lapic_id: u32,
    pub(crate) _cpu_count: u64,
}

// We need to implement Send for BootInfo so it can be used with `Once`.
// `LimineFramebuffer` uses `core::ptr::NonNull` which is not `Send`.
unsafe impl Send for BootInfo {}

impl BootInfo {
    /// Physical address for the Root System Description Pointer (RSDP). See <https://wiki.osdev.org/RSDP>
    pub(crate) fn rsdp_physical_addr(&self) -> PhysAddr {
        self.rsdp_address
            .map(|addr| {
                x86_64::PhysAddr::new(addr.as_u64() - self.higher_half_direct_map_offset.as_u64())
            })
            .expect("failed to get RSDP physical address")
    }
}

static BOOT_INFO_REQUEST: LimineBootInfoRequest = LimineBootInfoRequest::new(0);
static EFI_SYSTEM_TABLE_REQUEST: LimineEfiSystemTableRequest = LimineEfiSystemTableRequest::new(0);
static FRAMEBUFFER_REQUEST: LimineFramebufferRequest = LimineFramebufferRequest::new(0);
static HIGHER_HALF_DIRECT_MAP_REQUEST: LimineHhdmRequest = LimineHhdmRequest::new(0);
static KERNEL_ADDRESS_REQUEST: LimineKernelAddressRequest = LimineKernelAddressRequest::new(0);
static MEMORY_MAP_REQUEST: LimineMemmapRequest = LimineMemmapRequest::new(0);
static RSDP_REQUEST: LimineRsdpRequest = LimineRsdpRequest::new(0);
static SMP_REQUEST: LimineSmpRequest = LimineSmpRequest::new(0);

pub(crate) fn boot_info() -> &'static BootInfo {
    BOOT_INFO_ONCE.call_once(|| -> BootInfo {
        let (info_name, info_version) = limine_boot_info();

        let higher_half_direct_map_offset = limine_higher_half_offset();

        let kernel_address = KERNEL_ADDRESS_REQUEST
            .get_response()
            .get()
            .expect("failed to get limine kernel address request");

        let framebuffer = limine_framebuffer();

        let smp_info = SMP_REQUEST
            .get_response()
            .get()
            .expect("failed to get limine smp info response");

        BootInfo {
            _info_name: info_name,
            _info_version: info_version,
            higher_half_direct_map_offset,
            _kernel_address_physical_base: PhysAddr::new(kernel_address.physical_base),
            _kernel_address_virtual_base: VirtAddr::new(kernel_address.virtual_base),
            efi_system_table_address: limine_efi_system_table_address(),
            rsdp_address: limine_rsdp_address(),
            framebuffer,
            _x2apic_enabled: smp_info.flags & 1 == 1,
            bootstrap_processor_lapic_id: smp_info.bsp_lapic_id,
            _cpu_count: smp_info.cpu_count,
        }
    })
}

fn limine_boot_info() -> (&'static str, &'static str) {
    let limine_boot_info = BOOT_INFO_REQUEST
        .get_response()
        .get()
        .expect("failed to get limine boot info");

    let info_name_ptr = limine_boot_info
        .name
        .as_ptr()
        .expect("no limine boot info name");
    let info_name = unsafe { strings::c_str_from_pointer(info_name_ptr.cast::<u8>(), 100) };

    let info_version_ptr = limine_boot_info
        .version
        .as_ptr()
        .expect("no limine boot info version");
    let info_version = unsafe { strings::c_str_from_pointer(info_version_ptr.cast::<u8>(), 100) };

    (info_name, info_version)
}

fn limine_higher_half_offset() -> VirtAddr {
    let hhdm = HIGHER_HALF_DIRECT_MAP_REQUEST
        .get_response()
        .get()
        .expect("failed to get limine higher half direct map request");
    VirtAddr::try_new(hhdm.offset).expect("invalid limine hhdm offset virtual address")
}

fn limine_efi_system_table_address() -> Option<VirtAddr> {
    let Some(efi_system_table) = EFI_SYSTEM_TABLE_REQUEST.get_response().get() else { return None; };
    let Some(address_ptr) = efi_system_table.address.as_ptr() else { return None; };
    Some(VirtAddr::from_ptr(address_ptr))
}

fn limine_rsdp_address() -> Option<VirtAddr> {
    let Some(rsdp) = RSDP_REQUEST.get_response().get() else { return None; };
    let Some(address_ptr) = rsdp.address.as_ptr() else { return None; };
    Some(VirtAddr::from_ptr(address_ptr))
}

fn limine_framebuffer() -> &'static mut limine::LimineFramebuffer {
    let response = FRAMEBUFFER_REQUEST
        .get_response()
        .get()
        .expect("failed to get limine framebuffer response");

    assert!(
        response.framebuffer_count >= 1,
        "limine framebuffer count is less than 1"
    );

    let framebuffer = &response.framebuffers()[0];

    unsafe { &mut *framebuffer.as_ptr() }
}

/// Internal struct to iterate over the limine memory map.
///
/// Normally we would just make a `Vec` with all of the memory map entries, but
/// it turns out encapsulating iteration over the raw pointers from limine is
/// helpful, and this also means we can avoid allocating a `Vec` for the memory
/// map entries, so this can be used to construct the kernel's memory allocator.
struct MemoryMapEntryIterator {
    entries: *mut NonNullPtr<limine::LimineMemmapEntry>,
    entry_count: isize,
    current: isize,
}

impl Iterator for MemoryMapEntryIterator {
    type Item = &'static limine::LimineMemmapEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.current >= self.entry_count {
            return None;
        }

        unsafe {
            let entry = &**self.entries.offset(self.current);
            self.current += 1;
            Some(entry)
        }
    }
}

fn limine_memory_map_entries() -> impl Iterator<Item = &'static limine::LimineMemmapEntry> {
    let memory_map = MEMORY_MAP_REQUEST
        .get_response()
        .get()
        .expect("failed to get limine memory map");

    MemoryMapEntryIterator {
        entries: memory_map.entries.as_ptr(),
        #[allow(clippy::cast_possible_wrap)]
        entry_count: memory_map.entry_count as isize,
        current: 0,
    }
}

/// Create usable memory regions iterator from the limine memory map.
///
/// N.B. In principle we could use the reclaimable regions as well (entry.typ ==
/// `limine::LimineMemoryMapEntryType::BootloaderReclaimable`) However, I'm not
/// confident what cleanup needs to be done. In particular, the [limine
/// protocol](https://github.com/limine-bootloader/limine/blob/trunk/PROTOCOL.md)
/// says:
///
/// > The bootloader page tables are in bootloader-reclaimable memory [...], and
/// > their specific layout is undefined as long as they provide the above
/// > memory mappings.
pub(crate) fn limine_memory_regions() -> impl Iterator<Item = bitmap_alloc::MemoryRegion> {
    limine_memory_map_entries().map(|entry| bitmap_alloc::MemoryRegion {
        start_address: entry.base as usize,
        len_bytes: entry.len,
        // See not above about usable vs reclaimable.
        free: entry.typ == limine::LimineMemoryMapEntryType::Usable,
    })
}

pub(crate) fn print_limine_memory_map() {
    let memory_map_iter = limine_memory_map_entries();

    serial_println!("limine memory map:");
    let mut memory_totals = [0u64; 16];
    let mut max_memory = 0;
    for entry in memory_map_iter {
        serial_println!(
            "    base: {:#x}, len: {:#x}, type: {:?}",
            entry.base,
            entry.len,
            entry.typ
        );

        memory_totals[entry.typ as usize] += entry.len;
        max_memory = max(max_memory, entry.base + entry.len);
    }

    serial_println!("limine memory map totals:");
    serial_println!("    max_memory: {} MiB", max_memory / 1024 / 1024);
    serial_println!(
        "    usable: {} MiB",
        memory_totals[LimineMemoryMapEntryType::Usable as usize] / 1024 / 1024
    );
    serial_println!(
        "    reserved: {} MiB",
        memory_totals[LimineMemoryMapEntryType::Reserved as usize] / 1024 / 1024
    );
    serial_println!(
        "    ACPI reclaimable: {} MiB",
        memory_totals[LimineMemoryMapEntryType::AcpiReclaimable as usize] / 1024 / 1024
    );
    serial_println!(
        "    ACPI NVS: {} MiB",
        memory_totals[LimineMemoryMapEntryType::AcpiNvs as usize] / 1024 / 1024
    );
    serial_println!(
        "    bad memory: {} MiB",
        memory_totals[LimineMemoryMapEntryType::BadMemory as usize] / 1024 / 1024
    );
    serial_println!(
        "    boot loader reclaimable: {} MiB",
        memory_totals[LimineMemoryMapEntryType::BootloaderReclaimable as usize] / 1024 / 1024
    );
    serial_println!(
        "    kernel and modules: {} MiB",
        memory_totals[LimineMemoryMapEntryType::KernelAndModules as usize] / 1024 / 1024
    );
    serial_println!(
        "    framebuffer: {} MiB",
        memory_totals[LimineMemoryMapEntryType::Framebuffer as usize] / 1024 / 1024
    );
}

#[derive(Debug)]
pub(crate) struct LimineCPU {
    pub(crate) smp_response: &'static limine::LimineSmpResponse,
    pub(crate) info: &'static mut limine::LimineSmpInfo,
}

impl LimineCPU {
    pub(crate) fn bootstrap_cpu(&mut self, f: extern "C" fn(*const limine::LimineSmpInfo) -> !) {
        if self.smp_response.bsp_lapic_id == self.info.lapic_id {
            // This is the bootstrap processor, so we don't need to do anything.
            return;
        }
        self.info.extra_argument = 0xdead_beef;
        self.info.goto_address = f;
    }
}

/// Internal struct to iterate over CPUs detected by limine.
struct CPUIterator {
    smp_response: &'static limine::LimineSmpResponse,
    current: isize,
}

impl Iterator for CPUIterator {
    type Item = LimineCPU;

    fn next(&mut self) -> Option<Self::Item> {
        #[allow(clippy::cast_possible_wrap)]
        if self.current >= self.smp_response.cpu_count as isize {
            return None;
        }

        unsafe {
            let entry = &mut **self.smp_response.cpus.as_ptr().offset(self.current);
            self.current += 1;
            Some(LimineCPU {
                smp_response: self.smp_response,
                info: entry,
            })
        }
    }
}

pub(crate) fn limine_smp_entries() -> impl Iterator<Item = LimineCPU> {
    let smp_response = SMP_REQUEST
        .get_response()
        .get()
        .expect("failed to get limine SMP response");

    CPUIterator {
        smp_response,
        current: 0,
    }
}
