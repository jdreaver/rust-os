use core::panic::PanicInfo;
use multiboot2::load;

use crate::{gdt, interrupts, println, serial_println};

fn init() {
    gdt::init();
    interrupts::init_idt();
}

#[no_mangle]
pub extern "C" fn kmain(multiboot_info_ptr: usize) -> ! {
    // ATTENTION: we have a somewhat small stack and no guard page
    print_multiboot_info(multiboot_info_ptr);
    init();
    run_tests();
    hlt_loop()
}

fn print_multiboot_info(multiboot_info_ptr: usize) {
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
    println!("kernel start: {:#x}, end: {:#x}", kernel_start, kernel_end);

    let multiboot_start = multiboot_info_ptr;
    let multiboot_end = multiboot_start + boot_info.total_size();
    println!(
        "multiboot start: {:#x}, end: {:#x}",
        multiboot_start, multiboot_end
    );
}

fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    hlt_loop()
}

fn run_tests() {
    serial_println!("Testing serial port! {}", "hello");

    // Invoke a breakpoint exception and ensure we continue on
    println!("interrupt");
    x86_64::instructions::interrupts::int3();

    println!("done with interrupt");

    // Trigger a page fault, which should trigger a double fault if we don't
    // have a page fault handler.
    // unsafe {
    //     // N.B. Rust panics if we try to use 0xdeadbeef as a pointer (address
    //     // must be a multiple of 0x8), so we use 0xdeadbee0 instead
    //     *(0xdeadbee0 as *mut u64) = 42;
    // };

    println!("Tests passed!");

    // Test custom panic handler
    panic!("Some panic message");
}
