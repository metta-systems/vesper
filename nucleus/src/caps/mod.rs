/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Implementation of system capabilities.

// ☐ Rust implementation of capabilities - ?
//   ☐ Need to implement in kernel entries storage and lookup
//   ☐ cte = cap table entry (a cap_t plus mdb_node_t)
//   ☐ mdb = ? (mdb_node_new)
//   ☐ sameObjectAs()

//     cap_get_capType();//generated
//     lookupCapAndSlot();

// cap_domain_cap_new() etc //generated
// create_mapped_it_frame_cap(); //vspace.c

// pptr_of_cap(); -- extracts cap.pptr from cnode_cap
// deriveCap();

// @todo Use bitmatch over cap Type field?
// Could be interesting if usable. See https://github.com/porglezomp/bitmatch
// Maybe look at https://lib.rs/crates/enumflags2 too

use {crate::memory::PhysAddr, core::convert::TryFrom, snafu::Snafu};

mod capnode_cap;
mod captable;
mod derivation_tree;
mod domain_cap;
mod endpoint_cap;
mod irq_control_cap;
mod irq_handler_cap;
mod notification_cap;
mod null_cap;
mod reply_cap;
mod resume_cap;
mod thread_cap;
mod untyped_cap;
mod zombie_cap;

/// Opaque capability object, manipulated by the kernel.
pub trait Capability {
    ///
    /// Is this capability arch-specific?
    ///
    fn is_arch(&self) -> bool;

    ///
    /// Retrieve this capability as scalar value.
    ///
    fn as_u128(&self) -> u128;
}

/// Errors in capability operations.
#[derive(Debug, Snafu)]
pub enum CapError {
    /// Unable to create capability, exact reason TBD.
    CannotCreate,
    /// Capability has a type incompatible with the requested operation.
    InvalidCapabilityType,
}

/// Implement default fns and traits for the capability.
#[macro_export]
macro_rules! capdef {
    ($name:ident) => {
        paste! {
            #[doc = "Wrapper representing `" $name "Capability`."]
            pub struct [<$name Capability>](LocalRegisterCopy<u128, [<$name Cap>]::Register>);
            impl Capability for [<$name Capability>] {
                #[inline]
                fn as_u128(&self) -> u128 {
                    self.0.into()
                }
                #[inline]
                fn is_arch(&self) -> bool {
                    ([<$name Cap>]::Type::Value::value as u8) % 2 != 0
                }
            }
            impl TryFrom<u128> for [<$name Capability>] {
                type Error = CapError;
                fn try_from(v: u128) -> Result<[<$name Capability>], Self::Error> {
                    let reg = LocalRegisterCopy::<_, [<$name Cap>]::Register>::new(v);
                    if reg.read([<$name Cap>]::Type) == u128::from([<$name Cap>]::Type::value) {
                        Ok([<$name Capability>](LocalRegisterCopy::new(v)))
                    } else {
                        Err(Self::Error::InvalidCapabilityType)
                    }
                }
            }
            impl From<[<$name Capability>]> for u128 {
                #[inline]
                fn from(v: [<$name Capability>]) -> u128 {
                    v.as_u128()
                }
            }
        }
    };
}
