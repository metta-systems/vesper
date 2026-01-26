//# anyhow = "*"

// #![allow(dead_code, unused_variables)]

// mod arch { pub mod aarch64 { pub mod address {
//     use {core::marker::PhantomData, crate::address::{Address, Physical}};

//     pub struct PhysAddr(Address<Physical>);

//     impl PhysAddr {
//         pub fn new(addr: u64) -> Self {
//             Self(Address::<Physical>::new(addr))
//         }
//     }

//     impl Address<Physical> {
//         pub fn new(addr: u64) -> Self {
//             Self(addr, PhantomData)
//         }
//     }
// } } }

// mod address {
//     use core::marker::PhantomData;

//     pub struct Physical;
//     pub struct Virtual;
//     pub trait AddressType {}
//     impl AddressType for Physical {}
//     impl AddressType for Virtual {}
//     pub struct Address<T: AddressType>(pub u64, pub PhantomData<T>,);
// }

// fn main() {
//     use arch::aarch64::address::PhysAddr;
//     let a = PhysAddr::new(0xf0f0f0f0);
// }

//=================================================================================================
//=================================================================================================
//=================================================================================================

#![allow(dead_code, unused_variables)]

mod arch {
    pub mod aarch64 {
        pub mod address {
            use {
                crate::address::{Address, Physical},
                core::marker::PhantomData,
            };

            pub struct Physical;
            pub struct Virtual;

            impl AddressType for Physical {}
            impl AddressType for Virtual {}

            pub struct PhysAddr(Address<Physical>);

            impl PhysAddr {
                pub fn new(addr: u64) -> Self {
                    Self(Address::<Physical>::new(addr))
                }
            }

            impl Address<Physical> {
                pub fn new(addr: u64) -> Self {
                    Self(addr, PhantomData)
                }
            }
        }
    }
}

mod address {
    // The platform-independent interface is a union of all the platform-specific
    // things, but on the platforms that don't support them they are either errors
    // or no-ops. (?? this doesn't sound too strict/safe though ??)
    use core::marker::PhantomData;

    pub trait AddressType {
        fn new(addr: u64) -> Result<Address, Error>;
    }
    pub struct Address<T: AddressType>(u64, PhantomData<T>);
    impl<T: AddressType> Address<T> {
        pub fn new(addr: u64) -> Result<Self> {
            let inner = T::new(addr)?;
        }
    }
}

fn main() {
    use arch::aarch64::address::PhysAddr; // we want to take address::PhysAddr tho - no platform-specific stuff
    let a = PhysAddr::new(0xf0f0f0f0);
}

//=================================================================================================
//=================================================================================================
//=================================================================================================
