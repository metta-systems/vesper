//! Shared capability-invocation status words (`x0` on `AArch64`).
//!
//! These values are wire ABI, shared by kernel encoding and client decoding.
//! Unknown nonzero values must remain observable rather than becoming success.

pub const SUCCESS: u64 = 0;
pub const UNKNOWN: u64 = 1;
pub const NULL_CAPABILITY: u64 = 2;
pub const INVALID_DOMAIN: u64 = 3;
pub const INVALID_POINTER: u64 = 4;
pub const INSUFFICIENT_RIGHTS: u64 = 5;
pub const NOT_MAPPED: u64 = 6;
pub const ALREADY_MAPPED: u64 = 7;
pub const INVALID_OPERATION: u64 = 8;
pub const ASID_POOL_EXHAUSTED: u64 = 9;
pub const NO_ASID_ASSIGNED: u64 = 10;
pub const INVALID_SLOT: u64 = 11;
pub const EMPTY_SLOT: u64 = 12;
pub const SLOT_OCCUPIED: u64 = 13;
pub const NOT_CORE_TYPE: u64 = 14;
pub const UNKNOWN_CORE_TYPE: u64 = 15;
pub const UNSUPPORTED_CORE_TYPE: u64 = 16;
pub const NOT_ARCH_TYPE: u64 = 17;
pub const UNKNOWN_ARCH_TYPE: u64 = 18;
pub const UNSUPPORTED_ARCH_TYPE: u64 = 19;
pub const INVALID_OBJECT_TYPE: u64 = 20;
pub const TYPE_MISMATCH: u64 = 21;
pub const INSUFFICIENT_MEMORY: u64 = 22;
pub const POOL_EXHAUSTED: u64 = 23;
pub const INVALID_SIZE: u64 = 24;
pub const INVALID_FRAME_SIZE: u64 = 25;
pub const INVALID_KEY: u64 = 26;
pub const INCONSISTENT_KEY: u64 = 27;
pub const KEY_SLOT_EXHAUSTED: u64 = 28;
/// A mapping walk reached an absent intermediate page table.
/// Detail 1 is the faulting virtual address; detail 2 is zero.
pub const MISSING_INTERMEDIATE: u64 = 29;
/// The mapping violates the alias policy: the frame's physical extent
/// overlaps a live mapping in the target Domain. Detail 1 is the physical
/// base of the conflicting live mapping; detail 2 is zero.
pub const PHYSICAL_ALIAS: u64 = 30;
/// An `EventCount.Advance` would exceed the counter's `u64` range
/// (selected 2026-09-18): the counter is unchanged and every queued
/// `Await` completes with this same error. Details are zero.
pub const COUNTER_OVERFLOW: u64 = 31;
/// Invalid Invocation stack extent, minimum headroom, or submitted SP.
/// Detail 1 is the offending submitted value; detail 2 is an `InvalidStackReason`.
pub const INVALID_STACK: u64 = 32;

/// Field-specific Invocation stack diagnostics, carried as the complete `x2` word.
///
/// Zero is invalid. Numeric IDs do not define validation precedence, and no
/// operand index is packed into the reason. Relational failures report the
/// submitted value, not a computed extent length, headroom, or deficit.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InvalidStackReason {
    /// `end == base`; reports `end`.
    ExtentEmpty = 1,
    /// `end < base`; reports `end`.
    ExtentInverted = 2,
    /// Base lies outside the target user VA range; reports `base`.
    BaseOutsideUserRange = 3,
    /// Exclusive end exceeds the permitted user extent; reports `end`.
    EndOutsideUserRange = 4,
    /// Base is not 16-byte aligned; reports `base`.
    BaseMisaligned = 5,
    /// End is not 16-byte aligned; reports `end`.
    EndMisaligned = 6,
    /// Minimum downward headroom is zero; reports the submitted minimum.
    MinimumHeadroomZero = 7,
    /// Minimum headroom is not 16-byte aligned; reports the submitted minimum.
    MinimumHeadroomMisaligned = 8,
    /// Minimum headroom exceeds `end - base`; reports the submitted minimum.
    MinimumHeadroomTooLarge = 9,
    /// SP is not 16-byte aligned; reports the submitted SP.
    SpMisaligned = 10,
    /// `SP <= base` or `SP > end`; reports the submitted SP.
    SpOutOfRange = 11,
    /// In-range SP has less than the minimum headroom; reports the submitted SP.
    SpInsufficientHeadroom = 12,
}

impl TryFrom<u64> for InvalidStackReason {
    type Error = ();

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::ExtentEmpty),
            2 => Ok(Self::ExtentInverted),
            3 => Ok(Self::BaseOutsideUserRange),
            4 => Ok(Self::EndOutsideUserRange),
            5 => Ok(Self::BaseMisaligned),
            6 => Ok(Self::EndMisaligned),
            7 => Ok(Self::MinimumHeadroomZero),
            8 => Ok(Self::MinimumHeadroomMisaligned),
            9 => Ok(Self::MinimumHeadroomTooLarge),
            10 => Ok(Self::SpMisaligned),
            11 => Ok(Self::SpOutOfRange),
            12 => Ok(Self::SpInsufficientHeadroom),
            _ => Err(()),
        }
    }
}
