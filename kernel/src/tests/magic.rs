//! This module is called magic because it uses linker script magic to register
//! and run tests. Based on the Linux kernel [KUnit
//! architecture](https://www.kernel.org/doc/html/latest/dev-tools/kunit/architecture.html).

extern "C" {
    static _start_init_test_array: u8;
    static _end_init_test_array: u8;
}

pub(super) fn run_tests_from_linker() {
    log::info!("Running tests from linker...");

    let test_array_start = unsafe { core::ptr::addr_of!(_start_init_test_array) };
    let test_array_end = unsafe { core::ptr::addr_of!(_end_init_test_array) };
    let test_array_size_bytes = test_array_end as usize - test_array_start as usize;
    assert!(
        test_array_size_bytes % core::mem::size_of::<Test>() == 0,
        "test array size must be a multiple of Test struct size"
    );
    let num_tests = test_array_size_bytes / core::mem::size_of::<Test>();
    log::info!("{} tests found", num_tests);

    let tests = unsafe {
        assert!(
            test_array_start as usize % core::mem::align_of::<Test>() == 0,
            "test array start must be aligned to Test struct alignment"
        );
        #[allow(clippy::cast_ptr_alignment)]
        core::slice::from_raw_parts(test_array_start.cast::<Test>(), num_tests)
    };
    for test in tests {
        log::info!("Running test {}...", test.name);
        let test_fn = test.test_fn;
        test_fn();
    }

    log::info!("Tests from linker complete!");
}

/// Holds a single test.
pub struct Test {
    name: &'static str,
    test_fn: fn(),
}

#[used]
#[link_section = ".init_test_array"]
pub static MY_TEST: Test = Test {
    name: "my_test",
    test_fn: my_test_fn,
};

fn my_test_fn() {
    log::info!("Hello from my_test_fn!");
}
