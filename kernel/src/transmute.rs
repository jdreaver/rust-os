//! Utilities to cast between types and bytes.

use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};

use zerocopy::{AsBytes, FromBytes, LayoutVerified};

/// Casts a slice of bytes to a reference of the given type.
pub(crate) fn try_cast_bytes_ref<T: FromBytes>(bytes: &[u8]) -> Option<&T> {
    Some(LayoutVerified::<_, T>::new_from_prefix(bytes)?.0.into_ref())
}

/// Casts a slice of bytes to a reference of the given type, starting at the
/// given offset.
pub(crate) fn try_cast_bytes_ref_offset<T: FromBytes>(bytes: &[u8], offset: usize) -> Option<&T> {
    let bytes = bytes.get(offset..)?;
    try_cast_bytes_ref(bytes)
}

/// Casts a slice of bytes to a mutable reference of the given type.
pub(crate) fn try_cast_bytes_ref_mut<T: FromBytes + AsBytes>(bytes: &mut [u8]) -> Option<&mut T> {
    Some(LayoutVerified::<_, T>::new_from_prefix(bytes)?.0.into_mut())
}

/// Casts a slice of bytes to a mutable reference of the given type, starting at
/// the given offset.
pub(crate) fn try_cast_bytes_ref_mut_offset<T: FromBytes + AsBytes>(
    bytes: &mut [u8],
    offset: usize,
) -> Option<&mut T> {
    let bytes = bytes.get_mut(offset..)?;
    try_cast_bytes_ref_mut(bytes)
}

pub(crate) fn try_write_bytes_offset<T: FromBytes + AsBytes>(
    bytes: &mut [u8],
    offset: usize,
    value: T,
) -> Option<()> {
    let val_ref = try_cast_bytes_ref_mut_offset(bytes, offset)?;
    *val_ref = value;
    Some(())
}

/// Wrapper around a buffer `B` that interprets the underlying bytes as a given
/// type.
#[derive(Debug)]
pub(crate) struct TransmuteView<B, T> {
    buffer: B,
    _phantom: PhantomData<T>,
}

impl<B, T> TransmuteView<B, T> {
    pub(crate) fn buffer(&self) -> &B {
        &self.buffer
    }
}

impl<B: AsRef<[u8]>, T: FromBytes> TransmuteView<B, T> {
    pub(crate) fn new(buffer: B) -> Option<Self> {
        // Assert that the conversion works. Ideally we could just store the
        // LayoutVerified here, but we can't do that _and_ store the buffer
        // itself because it makes the borrow checker unhappy.
        let _: &T = try_cast_bytes_ref(buffer.as_ref())?;

        Some(Self {
            buffer,
            _phantom: PhantomData,
        })
    }
}

impl<B: AsRef<[u8]>, T: FromBytes> Deref for TransmuteView<B, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Invariant: the cast is supposed to be infallible because we checked
        // it in the constructor.
        try_cast_bytes_ref(self.buffer.as_ref())
            .expect("INTERNAL ERROR: cast is supposed to be infallible")
    }
}

impl<B: AsRef<[u8]> + AsMut<[u8]>, T: FromBytes + AsBytes> DerefMut for TransmuteView<B, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Invariant: the cast is supposed to be infallible because we checked
        // it in the constructor.
        try_cast_bytes_ref_mut(self.buffer.as_mut())
            .expect("INTERNAL ERROR: cast is supposed to be infallible")
    }
}

impl<B, T> AsRef<T> for TransmuteView<B, T>
where
    T: FromBytes,
    B: AsRef<[u8]>,
    <Self as Deref>::Target: AsRef<T>,
{
    #[allow(clippy::explicit_deref_methods)]
    fn as_ref(&self) -> &T {
        self.deref()
    }
}

impl<B: AsRef<[u8]> + AsMut<[u8]>, T: FromBytes + AsBytes> AsMut<T> for TransmuteView<B, T>
where
    <Self as Deref>::Target: AsMut<T>,
{
    #[allow(clippy::explicit_deref_methods)]
    fn as_mut(&mut self) -> &mut T {
        self.deref_mut()
    }
}

/// Wrapper around a buffer `B` that interprets the underlying bytes as a
/// collection of the given type. Supports indexing into this collection by a
/// given offset.
#[derive(Debug)]
pub(crate) struct TransmuteCollection<B, T> {
    buffer: B,
    _phantom: PhantomData<T>,
}

impl<B, T> TransmuteCollection<B, T> {
    pub(crate) fn new(buffer: B) -> Self {
        Self {
            buffer,
            _phantom: PhantomData,
        }
    }

    pub(crate) fn buffer(&self) -> &B {
        &self.buffer
    }
}

impl<B: AsRef<[u8]>, T: FromBytes> TransmuteCollection<B, T> {
    pub(crate) fn get(&self, offset: usize) -> Option<&T> {
        try_cast_bytes_ref_offset(self.buffer.as_ref(), offset)
    }
}

impl<B: AsRef<[u8]> + AsMut<[u8]>, T: FromBytes + AsBytes> TransmuteCollection<B, T> {
    pub(crate) fn get_mut(&mut self, offset: usize) -> Option<&mut T> {
        try_cast_bytes_ref_mut_offset(self.buffer.as_mut(), offset)
    }

    pub(crate) fn write(&mut self, offset: usize, value: T) -> Option<()> {
        try_write_bytes_offset(self.buffer.as_mut(), offset, value)
    }
}
