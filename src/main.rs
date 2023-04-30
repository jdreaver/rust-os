#![no_std]
#![no_main]

use rust_os::{
    gdt, interrupts,
    limine::{self, limine_framebuffer},
    memory, serial_println,
};

#[no_mangle]
extern "C" fn _start() -> ! {
    limine::print_limine_boot_info();
    limine::print_limine_memory_map();
    limine::print_limine_kernel_address();

    let hhdm_offset = limine::limine_higher_half_offset();
    serial_println!("limine HHDM offset: {:?}", hhdm_offset);

    // Ensure we got a framebuffer.
    let framebuffer = limine_framebuffer();

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

    init();
    run_tests();

    // Print out some test addresses
    let mut mapper = unsafe { memory::init(hhdm_offset) };

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

    let mut frame_allocator = unsafe { limine::allocator_from_limine_memory_map() };
    serial_println!("allocator: {:?}", frame_allocator);

    use x86_64::structures::paging::FrameAllocator;
    serial_println!("next page: {:?}", frame_allocator.allocate_frame());
    serial_println!("next page: {:?}", frame_allocator.allocate_frame());

    for _ in 0..10000 {
        frame_allocator.allocate_frame();
    }

    serial_println!("far page: {:?}", frame_allocator.allocate_frame());

    // TODO: Delete all of these framebuffer testing. Mapping 0 to the
    // framebuffer is probably not a good idea :)
    //
    // Map to the limine framebuffer
    use x86_64::structures::paging::page::Size4KiB;
    use x86_64::structures::paging::{Mapper, Page, PageTableFlags, PhysFrame};

    let new_virt_addr = x86_64::VirtAddr::new(0);
    let page: Page<Size4KiB> = Page::containing_address(new_virt_addr);
    let framebuffer_virt_addr = framebuffer.address.as_ptr().unwrap() as u64;
    let framebuffer_physical_addr = framebuffer_virt_addr - hhdm_offset.as_u64();
    serial_println!(
        "limine framebuffer physical address: {:#x}",
        framebuffer_physical_addr
    );
    let frame = PhysFrame::containing_address(x86_64::PhysAddr::new(framebuffer_physical_addr));
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;

    let map_to_result = unsafe {
        // FIXME: this is not safe, we do it only for testing
        mapper.map_to(page, frame, flags, &mut frame_allocator)
    };
    map_to_result.expect("map_to failed").flush();

    let phys = mapper.translate_addr(new_virt_addr);
    serial_println!("MAPPED: {:?} -> {:?}", new_virt_addr, phys);

    // Draw horizontal blue line to test starting at the new map to just
    let writable_ptr: *mut u8 = new_virt_addr.as_mut_ptr();
    let blue = 0x000000FF;
    for i in 0..100_usize {
        let pixel_offset = i * (framebuffer.bpp / 8) as usize;
        unsafe {
            *(writable_ptr.add(pixel_offset) as *mut u32) = blue;
        }
    }

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
