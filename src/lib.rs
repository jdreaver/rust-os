#![no_std]
#![no_main]

use core::panic::PanicInfo;

mod vga_buffer;

#[no_mangle]
pub extern "C" fn rust_main() -> ! {
    // ATTENTION: we have a somewhat small stack and no guard page

    println!("Hello World{}", "!");
    println!("Hello again! Some numbers: {} {}", 42, (1.7 * 3.3) as u64);
    println!("A float: {}", 1.337);
    panic!("Some panic message");

    loop {}
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}
