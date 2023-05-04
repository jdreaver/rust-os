use limine::{
    LimineBootInfoRequest, LimineEfiSystemTableRequest, LimineFramebufferRequest,
    LimineHhdmRequest, LimineKernelAddressRequest, LimineMemmapRequest, LimineRsdpRequest,
    NonNullPtr,
};
use x86_64::{PhysAddr, VirtAddr};

use crate::{memory, serial_println};

static mut BOOT_INFO: Option<BootInfo> = None;

#[derive(Debug)]
pub struct BootInfo {
    pub info_name: &'static str,
    pub info_version: &'static str,
    pub higher_half_direct_map_offset: VirtAddr,
    pub kernel_address_physical_base: PhysAddr,
    pub kernel_address_virtual_base: VirtAddr,
    pub efi_system_table_address: Option<VirtAddr>,
    pub rsdp_address: Option<VirtAddr>,
    pub framebuffer: &'static mut limine::LimineFramebuffer,
}

static BOOT_INFO_REQUEST: LimineBootInfoRequest = LimineBootInfoRequest::new(0);
static EFI_SYSTEM_TABLE_REQUEST: LimineEfiSystemTableRequest = LimineEfiSystemTableRequest::new(0);
static FRAMEBUFFER_REQUEST: LimineFramebufferRequest = LimineFramebufferRequest::new(0);
static HIGHER_HALF_DIRECT_MAP_REQUEST: LimineHhdmRequest = LimineHhdmRequest::new(0);
static KERNEL_ADDRESS_REQUEST: LimineKernelAddressRequest = LimineKernelAddressRequest::new(0);
static MEMORY_MAP_REQUEST: LimineMemmapRequest = LimineMemmapRequest::new(0);
static RSDP_REQUEST: LimineRsdpRequest = LimineRsdpRequest::new(0);

pub fn init_boot_info() {
    let (info_name, info_version) = limine_boot_info();

    let higher_half_direct_map_offset = limine_higher_half_offset();

    let kernel_address = KERNEL_ADDRESS_REQUEST
        .get_response()
        .get()
        .expect("failed to get limine kernel address request");

    let framebuffer = limine_framebuffer();

    let boot_info = BootInfo {
        info_name,
        info_version,
        higher_half_direct_map_offset,
        kernel_address_physical_base: PhysAddr::new(kernel_address.physical_base),
        kernel_address_virtual_base: VirtAddr::new(kernel_address.virtual_base),
        efi_system_table_address: limine_efi_system_table_address(),
        rsdp_address: limine_rsdp_address(),
        framebuffer,
    };

    unsafe {
        BOOT_INFO = Some(boot_info);
    }
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
    let info_name = unsafe { str_from_pointer(info_name_ptr.cast::<u8>(), 100) };

    let info_version_ptr = limine_boot_info
        .version
        .as_ptr()
        .expect("no limine boot info version");
    let info_version = unsafe { str_from_pointer(info_version_ptr.cast::<u8>(), 100) };

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

pub fn boot_info() -> &'static BootInfo {
    unsafe { BOOT_INFO.as_ref().expect("boot info not initialized") }
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

/// # Safety
///
/// If the string is not null-terminated, this will happily iterate through
/// memory and print garbage until it finds a null byte or we hit a protection
/// fault because we tried to ready a page we don't have access to.
unsafe fn str_from_pointer(ptr: *const u8, max_size: usize) -> &'static str {
    let mut len: usize = 0;
    while len < max_size {
        let c = *ptr.add(len);
        if c == 0 {
            break;
        }
        len += 1;
    }

    let slice = core::slice::from_raw_parts(ptr, len);
    core::str::from_utf8(slice).unwrap_or("<invalid utf8>")
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

pub fn print_limine_memory_map() {
    let memory_map_iter = limine_memory_map_entries();

    serial_println!("limine memory map:");
    let mut usable = 0;
    let mut reclaimable = 0;
    for entry in memory_map_iter {
        serial_println!(
            "    base: {:#x}, len: {:#x}, type: {:?}",
            entry.base,
            entry.len,
            entry.typ
        );

        if entry.typ == limine::LimineMemoryMapEntryType::Usable {
            usable += entry.len;
        } else if entry.typ == limine::LimineMemoryMapEntryType::BootloaderReclaimable {
            reclaimable += entry.len;
        }
    }

    serial_println!(
        "limine memory map usable: {} MiB, reclaimable: {} MiB, reusable + reclaimable: {} MiB",
        usable / 1024 / 1024,
        reclaimable / 1024 / 1024,
        (usable + reclaimable) / 1024 / 1024
    );
}

/// Create a `NaiveFreeMemoryBlockAllocator` from the reclaimable regions in the
/// limine memory map.
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
pub fn allocator_from_limine_memory_map() -> memory::NaiveFreeMemoryBlockAllocator {
    // SAFETY: The limine memory map is valid for the lifetime of the kernel.
    unsafe {
        memory::NaiveFreeMemoryBlockAllocator::from_iter(
            limine_memory_map_entries()
                // See not above about usable vs reclaimable.
                .filter(|entry| entry.typ == limine::LimineMemoryMapEntryType::Usable)
                .map(|entry| memory::UsableMemoryRegion {
                    start_address: PhysAddr::new(entry.base),
                    len: entry.len,
                }),
        )
    }
}
