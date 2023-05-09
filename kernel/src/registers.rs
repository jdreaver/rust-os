// Inspiration taken from
// https://docs.rs/pci-driver/latest/pci_driver/#pci_struct-and-pci_bit_field

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

    /// Similar to `modify`, but the callback instead takes a mutable reference
    /// to the value read from the register. Then, the mutated value is stored back in the register.
    ///
    /// This is a nicer API in case all you are doing is calling mutable
    /// functions on the value, because otherwise you would probably `clone()`
    /// the value, mutate it, and then return the value anyway.
    pub fn modify_mut(&self, f: impl FnOnce(&mut T)) {
        let mut val = self.read();
        f(&mut val);
        self.write(val);
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
                $offset:literal => $name:ident : $register_type:ident $(< $type:ty >)?
            ),* $(,)?
        }
    ) => {
        $(#[$attr])*
        #[derive(Clone, Copy)]
        pub struct $struct_name {
            address: usize,
        }

        impl $struct_name {
            unsafe fn from_address(address: usize) -> Self {
                Self { address }
            }

            $(
                $crate::register_struct!(@register_method $offset, $name, $register_type, $(< $type >)? );
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

    (@register_method $offset:expr, $name:ident, $register_type:ident, $(< $type:ty >)? ) => {
        pub fn $name(&self) -> $register_type $(< $type >)? {
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
