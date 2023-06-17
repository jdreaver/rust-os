mod magic;
mod misc;

use crate::fs;

pub(crate) fn run_test_suite() {
    fs::ext2::run_tests();
    misc::run_misc_tests();
    magic::run_tests_from_linker();
}
