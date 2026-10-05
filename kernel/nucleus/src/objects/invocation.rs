//! Kernel-owned numeric stack contract carried inline in an Invocation.
//!
//! A validated extent establishes neither mapped/writable backing nor exclusive
//! stack ownership. Component setup supplies those guarantees; no allocation or
//! translation walk occurs here.

use libobject::{CapError, InvalidStackReason};

/// An immutable, validated target stack extent and downward headroom requirement.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvocationStackExtent {
    base: u64,
    end: u64,
    minimum_headroom: u64,
}

impl InvocationStackExtent {
    /// Validate the published extent within `[0, user_end_exclusive)`.
    /// The exclusive extent end may equal the user ceiling. Predicate order is
    /// part of the diagnostic ABI; compare bounds before subtracting.
    pub fn new(
        base: u64,
        end: u64,
        minimum_headroom: u64,
        user_end_exclusive: u64,
    ) -> Result<Self, CapError> {
        use InvalidStackReason as Reason;

        let invalid = |value, reason| CapError::InvalidStack { value, reason };
        if end == base {
            return Err(invalid(end, Reason::ExtentEmpty));
        }
        if end < base {
            return Err(invalid(end, Reason::ExtentInverted));
        }
        if base >= user_end_exclusive {
            return Err(invalid(base, Reason::BaseOutsideUserRange));
        }
        if end > user_end_exclusive {
            return Err(invalid(end, Reason::EndOutsideUserRange));
        }
        if base & 15 != 0 {
            return Err(invalid(base, Reason::BaseMisaligned));
        }
        if end & 15 != 0 {
            return Err(invalid(end, Reason::EndMisaligned));
        }
        if minimum_headroom == 0 {
            return Err(invalid(minimum_headroom, Reason::MinimumHeadroomZero));
        }
        if minimum_headroom & 15 != 0 {
            return Err(invalid(minimum_headroom, Reason::MinimumHeadroomMisaligned));
        }
        if minimum_headroom > end - base {
            return Err(invalid(minimum_headroom, Reason::MinimumHeadroomTooLarge));
        }
        Ok(Self {
            base,
            end,
            minimum_headroom,
        })
    }

    pub const fn base(self) -> u64 {
        self.base
    }

    pub const fn end(self) -> u64 {
        self.end
    }

    pub const fn minimum_headroom(self) -> u64 {
        self.minimum_headroom
    }

    /// Validate the submitted descending-stack SP before any Call admission.
    /// This helper does not enable Call or inspect mappings, depth or AS readiness.
    pub fn validate_sp(self, sp: u64) -> Result<(), CapError> {
        use InvalidStackReason as Reason;

        let invalid = |reason| CapError::InvalidStack { value: sp, reason };
        if sp & 15 != 0 {
            return Err(invalid(Reason::SpMisaligned));
        }
        if sp <= self.base || sp > self.end {
            return Err(invalid(Reason::SpOutOfRange));
        }
        if sp - self.base < self.minimum_headroom {
            return Err(invalid(Reason::SpInsufficientHeadroom));
        }
        Ok(())
    }
}

const _: () = assert!(core::mem::size_of::<InvocationStackExtent>() == 24);
const _: () = assert!(core::mem::align_of::<InvocationStackExtent>() == 8);
