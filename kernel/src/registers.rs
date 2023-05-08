// Inspiration taken from
// https://docs.rs/pci-driver/latest/pci_driver/#pci_struct-and-pci_bit_field
//
// TODO:
// - Support for bit fields, like the PCI driver crate does.

use core::marker::PhantomData;

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

    pub fn read(&self) -> T {
        unsafe { core::ptr::read_volatile(self.ptr) }
    }

    pub fn write(&self, val: T) {
        unsafe {
            core::ptr::write_volatile(self.ptr, val);
        }
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
                $crate::register_method!($offset, $name, $register_type, $type);
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
}

#[macro_export]
macro_rules! register_method {
    ($offset:expr, $name:ident, $register_type:ident, $type:ty) => {
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
