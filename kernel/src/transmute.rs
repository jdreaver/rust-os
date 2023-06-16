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

pub(crate) trait CastBytes {
    fn try_cast_ref<T: FromBytes>(&self) -> Option<&T>;
    fn try_cast_ref_offset<T: FromBytes>(&self, offset: usize) -> Option<&T>;
}

impl<T: AsRef<[u8]> + ?Sized> CastBytes for T {
    fn try_cast_ref<U: FromBytes>(&self) -> Option<&U> {
        try_cast_bytes_ref(self.as_ref())
    }

    fn try_cast_ref_offset<U: FromBytes>(&self, offset: usize) -> Option<&U> {
        try_cast_bytes_ref_offset(self.as_ref(), offset)
    }
}

pub(crate) trait CastBytesMut {
    fn try_cast_ref_mut<T: FromBytes + AsBytes>(&mut self) -> Option<&mut T>;
    fn try_cast_ref_mut_offset<T: FromBytes + AsBytes>(&mut self, offset: usize) -> Option<&mut T>;
}

impl<T: AsMut<[u8]> + ?Sized> CastBytesMut for T {
    fn try_cast_ref_mut<U: FromBytes + AsBytes>(&mut self) -> Option<&mut U> {
        try_cast_bytes_ref_mut(self.as_mut())
    }

    fn try_cast_ref_mut_offset<U: FromBytes + AsBytes>(&mut self, offset: usize) -> Option<&mut U> {
        try_cast_bytes_ref_mut_offset(self.as_mut(), offset)
    }
}

/// Wrapper around a buffer `B` that interprets the underlying bytes as a given
/// type.
#[derive(Debug)]
pub(crate) struct TransmuteView<B, T> {
    buffer: B,
    offset: usize,
    _phantom: PhantomData<T>,
}

impl<B, T> TransmuteView<B, T> {
    pub(crate) fn buffer(&self) -> &B {
        &self.buffer
    }
}

impl<B: CastBytes, T: FromBytes> TransmuteView<B, T> {
    pub(crate) fn new(buffer: B) -> Option<Self> {
        Self::new_offset(buffer, 0)
    }

    pub(crate) fn new_offset(buffer: B, offset: usize) -> Option<Self> {
        // Assert that the conversion works. Ideally we could just store the
        // LayoutVerified here, but we can't do that _and_ store the buffer
        // itself because it makes the borrow checker unhappy.
        let _: &T = buffer.try_cast_ref_offset(offset)?;

        Some(Self {
            buffer,
            offset,
            _phantom: PhantomData,
        })
    }
}

impl<B: CastBytes, T: FromBytes> Deref for TransmuteView<B, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // Invariant: the cast is supposed to be infallible because we checked
        // it in the constructor.
        self.buffer
            .try_cast_ref_offset(self.offset)
            .expect("INTERNAL ERROR: cast is supposed to be infallible")
    }
}

impl<B: CastBytes + CastBytesMut, T: FromBytes + AsBytes> DerefMut for TransmuteView<B, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // Invariant: the cast is supposed to be infallible because we checked
        // it in the constructor.
        self.buffer
            .try_cast_ref_mut_offset(self.offset)
            .expect("INTERNAL ERROR: cast is supposed to be infallible")
    }
}

impl<B, T> AsRef<T> for TransmuteView<B, T>
where
    T: FromBytes,
    B: CastBytes,
    <Self as Deref>::Target: AsRef<T>,
{
    #[allow(clippy::explicit_deref_methods)]
    fn as_ref(&self) -> &T {
        self.deref()
    }
}

impl<B: CastBytes + CastBytesMut, T: FromBytes + AsBytes> AsMut<T> for TransmuteView<B, T>
where
    <Self as Deref>::Target: AsMut<T>,
{
    #[allow(clippy::explicit_deref_methods)]
    fn as_mut(&mut self) -> &mut T {
        self.deref_mut()
    }
}
