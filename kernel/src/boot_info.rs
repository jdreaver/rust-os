use core::cmp::max;

use spin::Once;

use limine::{
    LimineBootInfoRequest, LimineEfiSystemTableRequest, LimineFramebufferRequest,
    LimineHhdmRequest, LimineKernelAddressRequest, LimineKernelFileRequest, LimineMemmapRequest,
    LimineMemoryMapEntryType, LimineModuleRequest, LimineRsdpRequest, LimineSmpRequest, NonNullPtr,
};
use x86_64::{PhysAddr, VirtAddr};

use crate::memory::KernPhysAddr;
use crate::{serial_println, strings};

static BOOT_INFO_ONCE: Once<BootInfo> = Once::new();

#[derive(Debug)]
pub(crate) struct BootInfo {
    pub(crate) _info_name: &'static str,
    pub(crate) _info_version: &'static str,
    pub(crate) kernel_cmdline: &'static str,
    pub(crate) higher_half_direct_map_offset: VirtAddr,
    pub(crate) _kernel_address_physical_base: PhysAddr,
    pub(crate) kernel_address_virtual_base: VirtAddr,
    pub(crate) _efi_system_table_address: Option<VirtAddr>,
    pub(crate) rsdp_address: Option<KernPhysAddr>,
    #[allow(dead_code)] // TODO: Remove dead_code modifier. Currently only used in tests
    pub(crate) framebuffer: &'static mut limine::LimineFramebuffer,
    pub(crate) _x2apic_enabled: bool,
    pub(crate) bootstrap_processor_lapic_id: u32,
    pub(crate) _cpu_count: u64,
    pub(crate) kernel_symbol_map_file: Option<KernelSymbolMapFile>,
}

// We need to implement Send for BootInfo so it can be used with `Once`.
// `LimineFramebuffer` uses `core::ptr::NonNull` which is not `Send`.
unsafe impl Send for BootInfo {}

#[derive(Debug)]
pub(crate) struct KernelSymbolMapFile {
    pub(crate) address: VirtAddr,
    pub(crate) length: u64,
}

impl KernelSymbolMapFile {
    /// Given an address of an instruction inside a function, find the function
    /// symbol and address for that instruction address.
    pub(crate) fn find_function_symbol_for_instruction_address(
        &self,
        address: u64,
    ) -> Option<&'static str> {
        self.as_str()
            .lines()
            .map(|line| {
                let address_str = line
                    .split_whitespace()
                    .next()
                    .expect("failed to get symbol map address string");
                let address = u64::from_str_radix(address_str, 16)
                    .expect("failed to parse symbol map address string");
                (address, line)
            })
            .filter(|(symbol_address, _)| *symbol_address <= address)
            .max_by_key(|(symbol_address, _)| *symbol_address)
            .map(|(_, line)| line)
    }

    fn as_str(&self) -> &'static str {
        unsafe { strings::c_str_from_pointer(self.address.as_ptr::<u8>(), self.length as usize) }
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
static KERNEL_FILE_REQUEST: LimineKernelFileRequest = LimineKernelFileRequest::new(0);
static MODULE_REQUEST: LimineModuleRequest = LimineModuleRequest::new(0);

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

        let kernel_file_response = KERNEL_FILE_REQUEST
            .get_response()
            .get()
            .expect("failed to get limine kernel file response");
        let kernel_file_ptr = kernel_file_response
            .kernel_file
            .as_ptr()
            .expect("no kernel file");
        let kernel_file = unsafe { kernel_file_ptr.read() };
        let kernel_cmdline_ptr = kernel_file.cmdline.as_ptr().expect("no kernel cmdline");
        let kernel_cmdline =
            unsafe { strings::c_str_from_pointer(kernel_cmdline_ptr.cast::<u8>(), 1000) };

        let module_response = MODULE_REQUEST
            .get_response()
            .get()
            .expect("failed to get limine module response");

        let mut kernel_symbol_map_file = None;
        for module in module_response.modules() {
            let path_ptr = module
                .path
                .as_ptr()
                .expect("no path pointer")
                .cast_const()
                .cast::<u8>();
            let path = unsafe { strings::c_str_from_pointer(path_ptr, 1000) };
            if path == "/kernel.symbols" {
                let address = module.base.as_ptr().expect("no module base").cast_const() as u64;
                kernel_symbol_map_file = Some(KernelSymbolMapFile {
                    address: VirtAddr::new(address),
                    length: module.length,
                });
            }
        }

        BootInfo {
            _info_name: info_name,
            _info_version: info_version,
            kernel_cmdline,
            higher_half_direct_map_offset,
            _kernel_address_physical_base: PhysAddr::new(kernel_address.physical_base),
            kernel_address_virtual_base: VirtAddr::new(kernel_address.virtual_base),
            _efi_system_table_address: limine_efi_system_table_address(),
            rsdp_address: limine_rsdp_address(),
            framebuffer,
            _x2apic_enabled: smp_info.flags & 1 == 1,
            bootstrap_processor_lapic_id: smp_info.bsp_lapic_id,
            _cpu_count: smp_info.cpu_count,
            kernel_symbol_map_file,
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

fn limine_rsdp_address() -> Option<KernPhysAddr> {
    let Some(rsdp) = RSDP_REQUEST.get_response().get() else { return None; };
    let Some(address_ptr) = rsdp.address.as_ptr() else { return None; };
    Some(KernPhysAddr::new(address_ptr as u64))
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
///
/// Also, limine has gaps in its memory map. For example, 0xa0000 through
/// 0xfffff is reserved for VGA and BIOS memory. In QEMU, limine just doesn't
/// mention it at all.
pub(crate) fn limine_memory_regions() -> impl Iterator<Item = bitmap_alloc::MemoryRegion> {
    limine_memory_map_entries().map(|entry| bitmap_alloc::MemoryRegion {
        start_address: entry.base as usize,
        len_bytes: entry.len,
        // See note above about usable vs reclaimable.
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
