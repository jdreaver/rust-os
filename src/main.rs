#![no_std]
#![no_main]

use rust_os::{gdt, interrupts, limine, memory, serial_println};

#[no_mangle]
extern "C" fn _start() -> ! {
    limine::print_limine_boot_info();
    limine::print_limine_memory_map();
    limine::print_limine_kernel_address();

    let hhdm_offset = limine::limine_higher_half_offset();
    serial_println!("limine HHDM offset: {:?}", hhdm_offset);

    // Ensure we got a framebuffer.
    if let Some(framebuffer_response) = limine::FRAMEBUFFER_REQUEST.get_response().get() {
        if framebuffer_response.framebuffer_count < 1 {
            hlt_loop();
        }

        // Get the first framebuffer's information.
        let framebuffer = &framebuffer_response.framebuffers()[0];

        for i in 0..100_usize {
            // Calculate the pixel offset using the framebuffer information we obtained above.
            // We skip `i` scanlines (pitch is provided in bytes) and add `i * 4` to skip `i` pixels forward.
            let pixel_offset = i * framebuffer.pitch as usize + i * 4;

            // Write 0xFFFFFFFF to the provided pixel offset to fill it white.
            // We can safely unwrap the result of `as_ptr()` because the framebuffer address is
            // guaranteed to be provided by the bootloader.
            unsafe {
                *(framebuffer.address.as_ptr().unwrap().add(pixel_offset) as *mut u32) = 0xFFFFFFFF;
            }
        }
    }

    init();
    run_tests();

    // Print out some test addresses
    let mapper = unsafe { memory::init(hhdm_offset) };

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

    let mut allocator = unsafe { limine::allocator_from_limine_memory_map() };
    serial_println!("allocator: {:?}", allocator);

    use x86_64::structures::paging::FrameAllocator;
    serial_println!("next page: {:?}", allocator.allocate_frame());
    serial_println!("next page: {:?}", allocator.allocate_frame());

    for _ in 0..10000 {
        allocator.allocate_frame();
    }

    serial_println!("far page: {:?}", allocator.allocate_frame());

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
