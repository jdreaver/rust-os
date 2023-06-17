mod magic;

pub(crate) use test_macro::kernel_test;

pub(crate) fn run_test_suite() {
    magic::run_tests_from_linker();
}
