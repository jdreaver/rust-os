#![no_std]
#![no_main]
#![feature(allocator_api)]

extern crate alloc;

use vesa_framebuffer::TextBuffer;

use rust_os::{boot_info, gdt, heap, hlt_loop, interrupts, memory, panic_handler, serial, tests};

static mut TEXT_BUFFER: TextBuffer = TextBuffer::new();

#[no_mangle]
extern "C" fn _start() -> ! {
    serial::init_serial_writer();

    boot_info::init_boot_info();
    let boot_info_data = boot_info::boot_info();

    gdt::init();
    interrupts::init_idt();

    let mut mapper = unsafe { memory::init(boot_info_data.higher_half_direct_map_offset) };
    let frame_allocator = boot_info::allocator_from_limine_memory_map();
    let mut frame_allocator = memory::LockedNaiveFreeMemoryBlockAllocator::new(frame_allocator);
    heap::init(&mut mapper, &mut frame_allocator).expect("failed to initialize allocator");

    // TODO: Initialize TEXT_BUFFER better so we don't need unsafe.
    let text_buffer = unsafe { &mut TEXT_BUFFER };
    tests::run_tests(
        boot_info_data,
        &mut mapper,
        &mut frame_allocator,
        text_buffer,
    );

    hlt_loop()
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    panic_handler(info)
}
