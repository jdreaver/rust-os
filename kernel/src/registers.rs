use core::fmt::Debug;
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

impl<T: Debug> Debug for RegisterRW<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        debug_fmt_register("RegisterRW", Some(self.read()), self.ptr, f)
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

impl<T: Debug> Debug for RegisterRO<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        debug_fmt_register("RegisterRO", Some(self.read()), self.ptr, f)
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

impl<T: Debug> Debug for RegisterWO<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        debug_fmt_register("RegisterWO", None, self.ptr, f)
    }
}

/// Common `Debug.fmt()` implementation for all register types.
fn debug_fmt_register<T: Debug>(
    struct_name: &str,
    value: Option<T>,
    ptr: *const T,
    f: &mut core::fmt::Formatter<'_>,
) -> core::fmt::Result {
    // We don't print these as structs because they take up way too much space
    // when using {:#?}.
    f.write_str(struct_name)?;
    f.write_str("(")?;
    match value {
        Some(value) => value.fmt(f)?,
        None => f.write_str("UNKNOWN (write only)")?,
    }
    f.write_str(" [")?;
    ptr.fmt(f)?;
    f.write_str("])")
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

/// Abstraction over a region of memory containing an array of `T`s, using
/// volatile reads and writes under the hood.
#[derive(Clone, Copy)]
pub struct VolatileArrayRW<T> {
    ptr: *mut T,
    len: usize,
    _phantom: PhantomData<T>,
}

impl<T> VolatileArrayRW<T> {
    /// # Safety
    ///
    /// The caller must ensure that the address is a valid memory location for a
    /// an array containing `size` `T`s.
    pub unsafe fn new(address: usize, len: usize) -> Self {
        Self {
            ptr: address as *mut T,
            len,
            _phantom: PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    /// Read from the array at `index` using `read_volatile`.
    pub fn read(&self, index: usize) -> T {
        assert!(index < self.len, "VolatileArrayRW read index out of bounds");
        unsafe { core::ptr::read_volatile(self.ptr.add(index)) }
    }

    /// Write to the array at `index` using `write_volatile`.
    pub fn write(&self, index: usize, val: T) {
        assert!(
            index < self.len,
            "VolatileArrayRW write index out of bounds"
        );
        unsafe {
            core::ptr::write_volatile(self.ptr.add(index), val);
        }
    }

    /// Modify the value of the array at `index` by reading it, applying the
    /// given function, and writing the result back.
    pub fn modify(&self, index: usize, f: impl FnOnce(T) -> T) {
        assert!(
            index < self.len,
            "VolatileArrayRW write index out of bounds"
        );
        let val = self.read(index);
        self.write(index, f(val));
    }

    /// Similar to `modify`, but the callback instead takes a mutable reference
    /// to the value read from the array. Then, the mutated value is stored back
    /// in the array.
    ///
    /// This is a nicer API in case all you are doing is calling mutable
    /// functions on the value, because otherwise you would probably `clone()`
    /// the value, mutate it, and then return the value anyway.
    pub fn modify_mut(&self, index: usize, f: impl FnOnce(&mut T)) {
        let mut val = self.read(index);
        f(&mut val);
        self.write(index, val);
    }
}

impl<T: Debug> Debug for VolatileArrayRW<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VolatileArrayRW")
            .field("ptr", &self.ptr)
            .field("size", &self.len)
            .finish()
    }
}
