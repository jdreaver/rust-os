// Take inspiration from
// https://docs.rs/pci-driver/latest/pci_driver/#pci_struct-and-pci_bit_field

#[macro_export]
macro_rules! register_struct {
    (
        $(#[$attr:meta])*
        $struct_name:ident {
            $(
                $offset:literal => $name:ident : $type:ty
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
                $crate::register_method_RW!($offset, $name, $type);
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
macro_rules! register_method_R {
    ($offset:expr, $name:ident, $type:ty) => {
        $crate::paste::paste! {
            fn [<read_ $name>](&self) -> $type {
                unsafe {
                    let ptr = (self.address + $offset) as *const $type;
                    core::ptr::read_volatile(ptr)
                }
            }
        }
    };
}

#[macro_export]
macro_rules! register_method_W {
    ($offset:expr, $name:ident, $type:ty) => {
        $crate::paste::paste! {
            fn [<write_ $name>](&mut self, value: $type) {
                unsafe {
                    let ptr = (self.address + $offset) as *mut $type;
                    core::ptr::write_volatile(ptr, value)
                }
            }
        }
    };
}

#[macro_export]
macro_rules! register_method_RW {
    ($offset:expr, $name:ident, $type:ty) => {
        $crate::register_method_R!($offset, $name, $type);
        $crate::register_method_W!($offset, $name, $type);
    };
}

register_struct!(
    MyStruct {
        0x0 => hello: u8,
        0x2 => blah: u16,
    }
);

pub fn test_code() {
    let mut my_struct = MyStruct::from_address(0x1000);
    my_struct.write_hello(0x42);
    my_struct.write_blah(0x1234);

    let my_struct = MyStruct::from_address(0x1000);
    assert_eq!(my_struct.read_hello(), 0x42);
    assert_eq!(my_struct.read_blah(), 0x1234);
}
