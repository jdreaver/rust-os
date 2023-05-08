// Inspiration taken from
// https://docs.rs/pci-driver/latest/pci_driver/#pci_struct-and-pci_bit_field

use core::cmp::Eq;
use core::marker::PhantomData;
use core::ops::{BitAnd, BitOr, Not, Shl, Shr};

/// Register mapped to some underlying memory address.
#[derive(Clone, Copy)]
pub struct RegisterRW<T> {
    ptr: *mut T,
    _phantom: PhantomData<T>,
}

impl<T> RegisterRW<T> {
    /// # Safety
    ///
    /// The caller must ensure that the address is a valid memory location for a
    /// register of size `T`.
    pub unsafe fn from_address(address: usize) -> Self {
        Self {
            ptr: address as *mut T,
            _phantom: PhantomData,
        }
    }

    /// Read from the register using `read_volatile`.
    pub fn read(&self) -> T {
        unsafe { core::ptr::read_volatile(self.ptr) }
    }

    /// Write to the register using `write_volatile`.
    pub fn write(&self, val: T) {
        unsafe {
            core::ptr::write_volatile(self.ptr, val);
        }
    }

    /// Modify the value of the register by reading it, applying the given
    /// function, and writing the result back.
    pub fn modify(&self, f: impl FnOnce(T) -> T) {
        let val = self.read();
        self.write(f(val));
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for RegisterRW<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RegisterRW")
            .field("ptr", &self.ptr)
            .field("value", &self.read())
            .finish()
    }
}

/// Read-only register mapped to some underlying memory address.
#[derive(Clone, Copy)]
pub struct RegisterRO<T> {
    ptr: *const T,
    _phantom: PhantomData<T>,
}

impl<T> RegisterRO<T> {
    /// # Safety
    ///
    /// The caller must ensure that the address is a valid memory location for a
    /// register of size `T`.
    pub unsafe fn from_address(address: usize) -> Self {
        Self {
            ptr: address as *const T,
            _phantom: PhantomData,
        }
    }

    /// Read from the register using `read_volatile`.
    pub fn read(&self) -> T {
        unsafe { core::ptr::read_volatile(self.ptr) }
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for RegisterRO<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RegisterRO")
            .field("ptr", &self.ptr)
            .field("value", &self.read())
            .finish()
    }
}

/// Write-only register mapped to some underlying memory address.
#[derive(Clone, Copy)]
pub struct RegisterWO<T> {
    ptr: *mut T,
    _phantom: PhantomData<T>,
}

impl<T> RegisterWO<T> {
    /// # Safety
    ///
    /// The caller must ensure that the address is a valid memory location for a
    /// register of size `T`.
    pub unsafe fn from_address(address: usize) -> Self {
        Self {
            ptr: address as *mut T,
            _phantom: PhantomData,
        }
    }

    /// Write to the register using `write_volatile`.
    pub fn write(&self, val: T) {
        unsafe {
            core::ptr::write_volatile(self.ptr, val);
        }
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for RegisterWO<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RegisterWO")
            .field("ptr", &self.ptr)
            .field("value", &"UNKNOWN (write only)")
            .finish()
    }
}

/// TODO: Document this better once it is stabilized.
#[macro_export]
macro_rules! register_struct {
    (
        $(#[$attr:meta])*
        $struct_name:ident {
            $(
                $offset:literal => $name:ident : $register_type:ident < $type:ty >
            ),* $(,)?
        }
    ) => {
        $(#[$attr])*
        #[derive(Clone, Copy)]
        struct $struct_name {
            address: usize,
        }

        impl $struct_name {
            fn from_address(address: usize) -> Self {
                Self { address }
            }

            $(
                $crate::register_struct!(@register_method $offset, $name, $register_type, $type);
            )*
        }

        impl core::fmt::Debug for $struct_name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.debug_struct(stringify!($struct_name))
                    .field("address", &self.address)
                    $(
                        .field(stringify!($name), &self.$name())
                    )*
                    .finish()
            }
        }
    };

    (@register_method $offset:expr, $name:ident, $register_type:ident, $type:ty) => {
        fn $name(&self) -> $register_type<$type> {
            unsafe { $register_type::from_address(self.address + $offset) }
        }
    };
}

// /// Experimental macro to automatically compute field offsets. I'm scared to
// /// rely on core::mem::size_of always matching what we would get from a
// /// `#[repr(C)]` struct.
// #[macro_export]
// macro_rules! register_struct_v2 {
//     (
//         $(#[$attr:meta])*
//         $struct_name:ident {
//             $(
//                 $name:ident : $type:ty
//             ),* $(,)?
//         }
//     ) => {
//         $(#[$attr])*
//         #[derive(Debug, Clone, Copy)]
//         struct $struct_name {
//             address: usize,
//         }

//         impl $struct_name {
//             fn from_address(address: usize) -> Self {
//                 Self { address }
//             }

//             $crate::register_struct_v2!(@internal 0, $( $name : $type ),*);
//         }
//     };

//     (@internal $offset:expr, $name:ident : $type:ty) => {
//         $crate::register_method_RW!($offset, $name, $type);
//     };

//     (@internal $offset:expr, $name:ident : $type:ty, $($rest_name:ident : $rest_type:ty),* $(,)?) => {
//         $crate::register_method_RW!($offset, $name, $type);
//         $crate::register_struct_v2!(@internal $offset + core::mem::size_of::<$type>(), $($rest_name : $rest_type),*);
//     };
// }

// TODO: This bit field stuff is super cool, but it can get very complicated.
// Maybe I don't need it for now? See
// https://docs.rs/pci-driver/latest/pci_driver/#pci_struct-and-pci_bit_field
// for inspiration.

/// TODO: Document this better once it is stabilized.
#[macro_export]
macro_rules! bit_field_struct {
    (
        $(#[$attr:meta])*
        $struct_name:ident: $register_type:ident {
            $(
                $bit_start:literal $(.. $bit_end:literal)? => $name:ident : $bits_type:ident $(< $field_type:ty >)?
            ),* $(,)?
        }
    ) => {
        $(#[$attr])*
        #[derive(Clone, Copy)]
        struct $struct_name {
            address: usize,
        }

        impl $struct_name {
            fn from_address(address: usize) -> Self {
                Self { address }
            }

            $(
                $crate::bit_field_struct!(@field_method $register_type $bit_start $(..$bit_end )? => $name, $bits_type $(< $field_type >)?);
            )*
        }

        impl core::fmt::Debug for $struct_name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.debug_struct(stringify!($struct_name))
                    .field("address", &self.address)
                    $(
                        .field(stringify!($name), &self.$name())
                    )*
                    .finish()
            }
        }
    };

    // No bits end. Default to 0.
    (@field_method $register_type: ident $bit_start:literal => $name:ident, $bits_type:ident $(< $field_type:ty> )? ) => {
        $crate::bit_field_struct!(@field_method $register_type $bit_start..0 => $name, $bits_type $(< $field_type> )?);
    };

    (@field_method $register_type: ident $bit_start:literal..$bit_end:literal => $name:ident, $bits_type:ident $(< $field_type:ty> )?) => {
        fn $name(&self) -> $bits_type< $( $field_type,)? $register_type> {
            todo!();
        }
    };
}

/// Access to some bits, represented by type `T`, in an underlying
/// `RegisterRW<U>`.
pub struct BitsRW<T, U> {
    register: RegisterRW<U>,
    mask: U,
    shift: u8,
    _phantom: PhantomData<T>,
}

impl<T, U> BitsRW<T, U>
where
    U: TryInto<T>
        + Shr<u8, Output = U>
        + Shl<u8, Output = U>
        + BitAnd<Output = U>
        + BitOr<Output = U>
        + Not<Output = U>
        + Copy,
    U::Error: core::fmt::Debug,
    T: Into<U>,
{
    /// # Safety
    ///
    /// The caller must ensure that the address is a valid memory location for a
    /// register of size `U`.
    pub unsafe fn from_address(address: usize, mask: U, shift: u8) -> Self {
        Self {
            register: unsafe { RegisterRW::from_address(address) },
            mask,
            shift,
            _phantom: PhantomData,
        }
    }

    pub fn read(&self) -> T {
        let val: U = self.register.read();
        let masked: U = val & self.mask;
        let shifted: U = masked >> self.shift;
        shifted
            .try_into()
            .expect("failed to convert underlying to type")
    }

    pub fn write(&self, value: T) {
        let val: U = value.into();
        let shifted: U = val << self.shift;
        let masked: U = shifted & self.mask;
        self.register.modify(|old| (old & !self.mask) | masked);
    }

    pub fn modify(&self, f: impl FnOnce(T) -> T) {
        let old = self.read();
        let new = f(old);
        self.write(new);
    }
}

impl<T, U> core::fmt::Debug for BitsRW<T, U>
where
    U: TryInto<T>
        + Shr<u8, Output = U>
        + Shl<u8, Output = U>
        + BitAnd<Output = U>
        + BitOr<Output = U>
        + Not<Output = U>
        + Copy,
    U::Error: core::fmt::Debug,
    T: Into<U> + core::fmt::Debug,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BitsRW")
            .field("value", &self.read())
            .finish()
    }
}

/// Similar to `BitsRW`, but only for a single bit.
pub struct BitRW<U> {
    register: RegisterRW<U>,
    mask: U,
    shift: u8,
}

impl<U> BitRW<U>
where
    U: Shr<u8, Output = U>
        + Shl<u8, Output = U>
        + BitAnd<Output = U>
        + BitOr<Output = U>
        + Eq
        + Not<Output = U>
        + From<bool>
        + From<u8>
        + Copy,
{
    /// # Safety
    ///
    /// The caller must ensure that the address is a valid memory location for a
    /// register of size `U`.
    pub unsafe fn from_address(address: usize, mask: U, shift: u8) -> Self {
        Self {
            register: unsafe { RegisterRW::from_address(address) },
            mask,
            shift,
        }
    }

    pub fn read(&self) -> bool {
        let val: U = self.register.read();
        let masked: U = val & self.mask;
        let shifted: U = masked >> self.shift;
        shifted != U::from(0u8)
    }

    pub fn write(&self, value: bool) {
        let val: U = value.into();
        let shifted: U = val << self.shift;
        let masked: U = shifted & self.mask;
        self.register.modify(|old| (old & !self.mask) | masked);
    }

    pub fn modify(&self, f: impl FnOnce(bool) -> bool) {
        let old = self.read();
        let new = f(old);
        self.write(new);
    }
}

impl<U> core::fmt::Debug for BitRW<U>
where
    U: Shr<u8, Output = U>
        + Shl<u8, Output = U>
        + BitAnd<Output = U>
        + BitOr<Output = U>
        + Eq
        + Not<Output = U>
        + From<bool>
        + From<u8>
        + Copy,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BitRW")
            .field("value", &self.read())
            .finish()
    }
}
