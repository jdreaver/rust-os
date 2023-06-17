//! Holds definitions and functions used in kernel tests. It is only outside the
//! kernel so we can create proc macros for tests.

#![no_std]

/// Holds a single test.
pub struct SimpleTest {
    pub name: &'static str,
    pub test_fn: fn(),
}
