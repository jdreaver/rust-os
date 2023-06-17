mod magic;
mod misc;

pub(crate) use test_macro::kernel_test;

pub(crate) fn run_test_suite() {
    misc::run_misc_tests();
    magic::run_tests_from_linker();
}
