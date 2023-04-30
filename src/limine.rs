use limine::{
    LimineBootInfoRequest, LimineFramebufferRequest, LimineHhdmRequest, LimineKernelAddressRequest,
    LimineMemmapRequest,
};
use x86_64::VirtAddr;

use crate::{serial, serial_print, serial_println};

pub static FRAMEBUFFER_REQUEST: LimineFramebufferRequest = LimineFramebufferRequest::new(0);

static BOOT_INFO_REQUEST: LimineBootInfoRequest = LimineBootInfoRequest::new(0);

pub fn print_limine_boot_info() {
    let boot_info = BOOT_INFO_REQUEST
        .get_response()
        .get()
        .expect("failed to get limine boot info");

    let boot_info_name_ptr = boot_info.name.as_ptr().expect("no limine boot info name");
    serial_print!("limine boot info name: ");
    unsafe {
        serial::print_null_terminated_string(boot_info_name_ptr as *const u8);
    }
    serial_println!("");

    let boot_info_version_ptr = boot_info
        .version
        .as_ptr()
        .expect("no limine boot info version");
    serial_print!("limine boot info version: ");
    unsafe {
        serial::print_null_terminated_string(boot_info_version_ptr as *const u8);
    }
    serial_println!("");
}

static MEMORY_MAP_REQUEST: LimineMemmapRequest = LimineMemmapRequest::new(0);

pub fn print_limine_memory_map() {
    let memory_map = MEMORY_MAP_REQUEST
        .get_response()
        .get()
        .expect("failed to get limine memory map");

    serial_println!("limine memory map:");
    let mut usable = 0;
    let mut reclaimable = 0;
    let entries = memory_map.entries.as_ptr();
    for i in 0..memory_map.entry_count {
        unsafe {
            #[allow(clippy::cast_possible_wrap)]
            let entry = &**entries.offset(i as isize);
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
    }

    serial_println!(
        "limine memory map usable: {} MiB, reclaimable: {} MiB, reusable + reclaimable: {} MiB",
        usable / 1024 / 1024,
        reclaimable / 1024 / 1024,
        (usable + reclaimable) / 1024 / 1024
    );
}

static HIGHER_HALF_DIRECT_MAP_REQUEST: LimineHhdmRequest = LimineHhdmRequest::new(0);

pub fn limine_higher_half_offset() -> VirtAddr {
    let hhdm = HIGHER_HALF_DIRECT_MAP_REQUEST
        .get_response()
        .get()
        .expect("failed to get limine higher half direct map request");
    VirtAddr::try_new(hhdm.offset).expect("invalid limine hhdm offset virtual address")
}

static KERNEL_ADDRESS_REQUEST: LimineKernelAddressRequest = LimineKernelAddressRequest::new(0);

pub fn print_limine_kernel_address() {
    let kernel_address = KERNEL_ADDRESS_REQUEST
        .get_response()
        .get()
        .expect("failed to get limine kernel address request");

    serial_println!(
        "limine kernel address physical base: {:#x}, virtual base: {:#x}",
        kernel_address.physical_base,
        kernel_address.virtual_base
    );
}
