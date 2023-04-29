use core::panic::PanicInfo;
use multiboot2::load;

use crate::{gdt, interrupts, println, serial_println};

#[no_mangle]
pub extern "C" fn kmain(multiboot_info_ptr: usize) -> ! {
    // ATTENTION: we have a somewhat small stack and no guard page

    let boot_info = unsafe { load(multiboot_info_ptr).unwrap() };

    let memory_map_tag = boot_info.memory_map_tag().expect("Memory map tag required");

    println!("memory areas:");
    for area in memory_map_tag.memory_areas() {
        println!(
            "    start: 0x{:x}, end: 0x{:x}",
            area.start_address(),
            area.end_address()
        );
    }

    let elf_sections_tag = boot_info
        .elf_sections_tag()
        .expect("Elf-sections tag required");

    println!("kernel sections:");
    for section in elf_sections_tag.sections() {
        println!(
            "    addr: 0x{:x}, size: 0x{:x}, flags: 0x{:x}",
            section.start_address(),
            section.size(),
            section.flags()
        );
    }

    let kernel_start = elf_sections_tag
        .sections()
        .map(|s| s.start_address())
        .min()
        .unwrap();
    let kernel_end = elf_sections_tag
        .sections()
        .map(|s| s.end_address())
        .max()
        .unwrap();
    println!(
        "kernel start: {:#x}, end: {:#x}",
        kernel_start, kernel_end
    );

    let multiboot_start = multiboot_info_ptr;
    let multiboot_end = multiboot_start + boot_info.total_size();
    println!(
        "multiboot start: {:#x}, end: {:#x}",
        multiboot_start, multiboot_end
    );

    serial_println!("Testing serial port! {}", "hello");

    init();

    // invoke a breakpoint exception
    x86_64::instructions::interrupts::int3(); // new

    // trigger a page fault
    unsafe {
        *(0xdeadbee0 as *mut u64) = 42;
    };

    panic!("Some panic message");

    loop {
        x86_64::instructions::hlt();
    }
}

fn init() {
    gdt::init();
    interrupts::init_idt();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {
        x86_64::instructions::hlt();
    }
}
