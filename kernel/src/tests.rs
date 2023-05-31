use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt::Write;

use vesa_framebuffer::{TextBuffer, VESAFramebuffer32Bit};
use x86_64::structures::paging::{Size2MiB, Size4KiB};

use crate::{boot_info, hpet, interrupts, memory, scheduler, serial_println};

static mut TEXT_BUFFER: TextBuffer = TextBuffer::new();

pub(crate) fn run_misc_tests() {
    let boot_info_data = boot_info::boot_info();

    // Ensure we got a framebuffer.
    let mut framebuffer = unsafe {
        VESAFramebuffer32Bit::from_limine_framebuffer(boot_info_data.framebuffer)
            .expect("failed to create VESAFramebuffer32Bit")
    };
    serial_println!("framebuffer: {framebuffer:#?}");

    // TODO: Initialize TEXT_BUFFER better so we don't need unsafe.
    let text_buffer = unsafe { &mut TEXT_BUFFER };

    writeln!(text_buffer, "Hello!").expect("failed to write to text buffer");
    writeln!(text_buffer, "World!").expect("failed to write to text buffer");

    text_buffer.flush(&mut framebuffer);

    // Print out some test addresses
    let addresses = [
        // the identity-mapped vga buffer page
        0xb8000,
        0xb8000 + boot_info_data.higher_half_direct_map_offset.as_u64(),
        // some code page
        0x0020_1008,
        // some stack page
        0x0100_0020_1a10,
        // virtual address mapped to physical address 0
        boot_info_data.higher_half_direct_map_offset.as_u64(),
    ];

    for &address in &addresses {
        let virt = x86_64::VirtAddr::new(address);
        let phys = memory::translate_addr(virt);
        serial_println!("{:?} -> {:?}", virt, phys);
    }

    serial_println!(
        "next 4KiB page: {:?}",
        memory::allocate_physical_frame::<Size4KiB>()
    );
    serial_println!(
        "next 2MiB page: {:?}",
        memory::allocate_physical_frame::<Size2MiB>()
    );
    serial_println!(
        "next 4KiB page: {:?}",
        memory::allocate_physical_frame::<Size4KiB>()
    );
    serial_println!(
        "next 2MiB page: {:?}",
        memory::allocate_physical_frame::<Size2MiB>()
    );

    for _ in 0..10000 {
        memory::allocate_physical_frame::<Size4KiB>();
    }

    serial_println!(
        "far page: {:?}",
        memory::allocate_physical_frame::<Size4KiB>()
    );

    // Invoke a breakpoint exception and ensure we continue on
    serial_println!("interrupt");
    x86_64::instructions::interrupts::int3();

    serial_println!("done with interrupt");

    // Allocate a number on the heap
    let heap_value = Box::new(41);
    serial_println!("heap_value at {heap_value:p}");
    assert_eq!(*heap_value, 41);

    // create a dynamically sized vector
    let mut vec = Vec::new();
    for i in 0..10 {
        vec.push(i);
    }
    serial_println!("vec at {:p}: {vec:?}", vec.as_slice());
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

pub(crate) fn test_scheduler() {
    scheduler::push_task("task 1", task_1_test_task, 0xdead_beef as *const ());
    scheduler::push_task("task 2", task_2_test_task, 0xabab_cdcd as *const ());

    scheduler::start_multitasking();
}

extern "C" fn task_1_test_task(arg: *const ()) {
    loop {
        serial_println!("task 1 is running! arg: {arg:x?}");
        let p = naive_nth_prime(2500);
        serial_println!("Task 1 DONE: 2500th prime: {}", p);
        scheduler::run_scheduler();
    }
}

extern "C" fn task_2_test_task(arg: *const ()) {
    serial_println!("task 2 is running! arg: {arg:x?}");
    let p = naive_nth_prime(3000);
    serial_println!("Task 2 DONE: 3000th prime: {}", p);
    scheduler::run_scheduler();
    serial_println!("TASK 2 DONE!!!");
}

fn naive_nth_prime(n: usize) -> usize {
    fn is_prime(x: usize) -> bool {
        for i in 2..x {
            if x % i == 0 {
                return false;
            }
        }
        true
    }

    let mut i = 2;
    let mut found_primes = 0;
    loop {
        i += 1;
        if is_prime(i) {
            found_primes += 1;
            if found_primes == n {
                return i;
            }
        }

        // Temporarily insert a point where we yield
        if found_primes % 500 == 0 {
            scheduler::run_scheduler();
        }
    }
}

pub(crate) fn test_hpet() {
    hpet::enable_periodic_timer_handler(
        123,
        test_hpet_interrupt_handler,
        TEST_HPET_TIMER_IOAPIC_IRQ_NUMBER,
        0,
        &hpet::Milliseconds::new(1000),
    );
}

/// Arbitrary IO/APIC interrupt number for the test HPET timer
const TEST_HPET_TIMER_IOAPIC_IRQ_NUMBER: u8 = 9;

fn test_hpet_interrupt_handler(_vector: u8, _handler_id: interrupts::InterruptHandlerID) {
    let ms_since_boot = hpet::elapsed_milliseconds();
    serial_println!("Test HPET interrupt fired. Time since boot: {ms_since_boot}");
}
