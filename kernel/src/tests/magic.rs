//! This module is called magic because it uses linker script magic to register
//! and run tests. Based on the Linux kernel [KUnit
//! architecture](https://www.kernel.org/doc/html/latest/dev-tools/kunit/architecture.html).

use proptest::{prop_assert, proptest};

use test_infra::SimpleTest;
use test_macro::kernel_test;

extern "C" {
    static _start_init_test_array: u8;
    static _end_init_test_array: u8;
}

pub(super) fn run_tests_from_linker() {
    log::info!("Running tests from linker...");

    let tests = find_tests();
    log::info!("{} tests found", tests.len());

    for test in tests {
        log::info!("Running test {}::{}...", test.module, test.name);
        let test_fn = test.test_fn;
        test_fn();
    }

    log::info!("Tests from linker complete!");
}

pub fn find_tests() -> &'static [SimpleTest] {
    let test_array_start = unsafe { core::ptr::addr_of!(_start_init_test_array) };
    let test_array_end = unsafe { core::ptr::addr_of!(_end_init_test_array) };
    let test_array_size_bytes = test_array_end as usize - test_array_start as usize;
    assert!(
        test_array_size_bytes % core::mem::size_of::<SimpleTest>() == 0,
        "test array size must be a multiple of Test struct size"
    );
    let num_tests = test_array_size_bytes / core::mem::size_of::<SimpleTest>();

    let tests = unsafe {
        assert!(
            test_array_start as usize % core::mem::align_of::<SimpleTest>() == 0,
            "test array start must be aligned to Test struct alignment"
        );
        #[allow(clippy::cast_ptr_alignment)]
        core::slice::from_raw_parts(test_array_start.cast::<SimpleTest>(), num_tests)
    };
    tests
}

#[kernel_test]
fn my_test_fn() {
    let x = 1;
    assert!(x == 1);
}

#[kernel_test]
fn my_other_test() {
    let x = "hello";
    assert!(x == "hello");
}

proptest!(
    #[kernel_test]
    fn example_proptest_test(x in 0..100u8) {
        prop_assert!(x < 100);
    }
);
