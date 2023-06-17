//! Holds definitions and functions used in kernel tests. It is only outside the
//! kernel so we can create proc macros for tests.

#![no_std]

use core::fmt;

/// Holds a single test.
pub struct SimpleTest {
    pub name: &'static str,
    pub module: &'static str,
    pub file: &'static str,
    pub line: u32,
    pub column: u32,
    pub test_fn: fn(),
}

impl fmt::Debug for SimpleTest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SimpleTest")
            .field("name", &self.name)
            .field("module", &self.module)
            .field("file", &self.file)
            .field("line", &self.line)
            .field("column", &self.column)
            .finish()
    }
}
