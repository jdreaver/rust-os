#![no_std]
#![no_main]

use core::arch::asm;
use core::panic::PanicInfo;
use multiboot2::load;

mod vga_buffer;

#[no_mangle]
pub extern "C" fn rust_main(multiboot_info_ptr: u32) -> ! {
    // ATTENTION: we have a somewhat small stack and no guard page

    let boot_info = unsafe { load(multiboot_info_ptr as usize).unwrap() };
    println!("Boot info: {:?}", boot_info);

    println!("Hello World{}", "!");
    println!("Hello again! Some numbers: {} {}", 42, (1.7 * 3.3) as u64);
    println!("A float: {}", 1.337);
    panic!("Some panic message");

    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack));
        }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {
        unsafe {
            asm!("hlt", options(nomem, nostack));
        }
    }
}
