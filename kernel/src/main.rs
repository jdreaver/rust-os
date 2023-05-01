#![no_std]
#![no_main]

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;

use rust_os::{allocator, boot_info, gdt, interrupts, memory, serial_println};

#[no_mangle]
extern "C" fn _start() -> ! {
    boot_info::print_limine_boot_info();
    boot_info::print_limine_memory_map();
    boot_info::print_limine_kernel_address();

    let hhdm_offset = boot_info::limine_higher_half_offset();
    serial_println!("limine HHDM offset: {:?}", hhdm_offset);

    // Ensure we got a framebuffer.
    let limine_framebuffer = boot_info::limine_framebuffer();
    serial_println!("limine framebuffer: {:#?}", limine_framebuffer);

    let framebuffer = unsafe {
        vesa_framebuffer::VESAFrambuffer32Bit::from_limine_framebuffer(limine_framebuffer)
    };
    serial_println!("framebuffer: {:#?}", framebuffer);

    for i in 0..100_usize {
        framebuffer.draw_pixel(i, i, vesa_framebuffer::ARGB32BIT_WHITE);
    }

    init();

    let mut mapper = unsafe { memory::init(hhdm_offset) };

    let mut frame_allocator = boot_info::allocator_from_limine_memory_map();
    serial_println!("allocator: {:?}", frame_allocator);

    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("failed to initialize allocator");

    run_tests();

    // Print out some test addresses
    let addresses = [
        // the identity-mapped vga buffer page
        0xb8000,
        0xb8000 + hhdm_offset.as_u64(),
        // some code page
        0x201008,
        // some stack page
        0x0100_0020_1a10,
        // virtual address mapped to physical address 0
        hhdm_offset.as_u64(),
    ];

    use x86_64::structures::paging::Translate;

    for &address in &addresses {
        let virt = x86_64::VirtAddr::new(address);
        let phys = mapper.translate_addr(virt);
        serial_println!("{:?} -> {:?}", virt, phys);
    }

    use x86_64::structures::paging::FrameAllocator;
    serial_println!("next page: {:?}", frame_allocator.allocate_frame());
    serial_println!("next page: {:?}", frame_allocator.allocate_frame());

    for _ in 0..10000 {
        frame_allocator.allocate_frame();
    }

    serial_println!("far page: {:?}", frame_allocator.allocate_frame());

    hlt_loop()
}

fn init() {
    gdt::init();
    interrupts::init_idt();
}

#[panic_handler]
fn rust_panic(info: &core::panic::PanicInfo) -> ! {
    serial_println!("PANIC: {}", info);
    hlt_loop()
}

fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

fn run_tests() {
    serial_println!("Testing serial port! {}", "hello");

    // Invoke a breakpoint exception and ensure we continue on
    serial_println!("interrupt");
    x86_64::instructions::interrupts::int3();

    serial_println!("done with interrupt");

    // Allocate a number on the heap
    let heap_value = Box::new(41);
    serial_println!("heap_value at {:p}", heap_value);
    assert_eq!(*heap_value, 41);

    // create a dynamically sized vector
    let mut vec = Vec::new();
    for i in 0..10 {
        vec.push(i);
    }
    serial_println!("vec at {:p}: {:?}", vec.as_slice(), vec);
    assert_eq!(vec.into_iter().sum::<u32>(), 45);

    // Trigger a page fault, which should trigger a double fault if we don't
    // have a page fault handler.
    // unsafe {
    //     // N.B. Rust panics if we try to use 0xdeadbeef as a pointer (address
    //     // must be a multiple of 0x8), so we use 0xdeadbee0 instead
    //     *(0xdeadbee0 as *mut u64) = 42;
    // };

    serial_println!("Tests passed!");

    // Test custom panic handler
    // panic!("Some panic message");
}
