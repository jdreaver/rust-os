use core::fmt;

use zerocopy::{AsBytes, FromBytes, FromZeroes};

/// Wrapper around a byte array that represents a nul-terminated string.
#[derive(Copy, Clone, FromZeroes, FromBytes, AsBytes)]
#[repr(transparent)]
pub(super) struct CStringBytes<B>(B);

impl<const N: usize> CStringBytes<[u8; N]> {
    pub(super) fn as_str(&self) -> &str {
        c_str_from_bytes(&self.0)
    }
}

impl<const N: usize> fmt::Debug for CStringBytes<[u8; N]> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CStringBytes").field(&self.as_str()).finish()
    }
}

/// Creates a null-terminated string from a byte slice.
pub(super) fn c_str_from_bytes(bytes: &[u8]) -> &str {
    let nul_location = bytes.iter().position(|&c| c == 0).unwrap_or(bytes.len());
    core::str::from_utf8(&bytes[..nul_location]).unwrap_or("<invalid UTF-8>")
}
