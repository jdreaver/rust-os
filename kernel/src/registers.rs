// Inspiration taken from
// https://docs.rs/pci-driver/latest/pci_driver/#pci_struct-and-pci_bit_field
//
// TODO:
// - Support for bit fields, like the PCI driver crate does.

use core::marker::PhantomData;

/// Register mapped to some underlying memory address.
#[derive(Debug, Clone, Copy)]
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

/// Read-only register mapped to some underlying memory address.
#[derive(Debug, Clone, Copy)]
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

/// Write-only register mapped to some underlying memory address.
#[derive(Debug, Clone, Copy)]
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
        #[derive(Debug, Clone, Copy)]
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

#[macro_export]
macro_rules! register_method {
    ($offset:expr, $name:ident, $register_type:ident, $type:ty) => {
        fn $name(&self) -> $register_type<$type> {
            unsafe { $register_type::from_address(self.address + $offset) }
        }
    };
}

// #[macro_export]
// macro_rules! register_method_R {
//     ($offset:expr, $name:ident, $type:ty) => {
//         $crate::paste::paste! {
//             fn [<read_ $name>](&self) -> $type {
//                 unsafe {
//                     let ptr = (self.address + $offset) as *const $type;
//                     core::ptr::read_volatile(ptr)
//                 }
//             }
//         }
//     };
// }

// #[macro_export]
// macro_rules! register_method_W {
//     ($offset:expr, $name:ident, $type:ty) => {
//         $crate::paste::paste! {
//             fn [<write_ $name>](&mut self, value: $type) {
//                 unsafe {
//                     let ptr = (self.address + $offset) as *mut $type;
//                     core::ptr::write_volatile(ptr, value)
//                 }
//             }
//         }
//     };
// }

// #[macro_export]
// macro_rules! register_method_RW {
//     ($offset:expr, $name:ident, $type:ty) => {
//         $crate::register_method_R!($offset, $name, $type);
//         $crate::register_method_W!($offset, $name, $type);
//     };
// }
