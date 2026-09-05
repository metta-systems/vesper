#![no_std]
#![no_main]

pub mod debug_console;
pub mod domain;
pub mod key;
pub mod key_table;
pub mod object_type;
pub mod rights;
pub mod syscall_status;

use syscall_status as code;

#[cfg(feature = "debug_kernel")]
pub use debug_console::DebugConsoleKey;

pub use {
    key::Key,
    key_table::KeySlot,
    object_type::{ArchType, CoreType, ObjectType},
    rights::Rights,
};

pub type SyscallResult = core::result::Result<(u64, u64), CapError>;

/// Decode the status and two result/detail words without discarding information.
///
/// Known errors require representable details and zero unused words. Object-type
/// details retain the full wire byte; unsupported kinds use category-local indices.
/// Unknown or unrepresentable errors are preserved as `CapError::UnknownResponse`.
pub fn decode_syscall_result((status, detail1, detail2): (u64, u64, u64)) -> SyscallResult {
    let Some(status) = core::num::NonZeroU64::new(status) else {
        return Ok((detail1, detail2));
    };

    let decoded = (|| {
        Some(match (status.get(), detail1, detail2) {
            (code::UNKNOWN, 0, 0) => CapError::Unknown,
            (code::NULL_CAPABILITY, 0, 0) => CapError::NullCapability,
            (code::INVALID_DOMAIN, 0, 0) => CapError::InvalidDomain,
            (code::INVALID_POINTER, 0, 0) => CapError::InvalidPointer,
            (code::INSUFFICIENT_RIGHTS, 0, 0) => CapError::InsufficientRights,
            (code::NOT_MAPPED, 0, 0) => CapError::NotMapped,
            (code::ALREADY_MAPPED, 0, 0) => CapError::AlreadyMapped,
            (code::INVALID_OPERATION, 0, 0) => CapError::InvalidOperation,
            (code::ASID_POOL_EXHAUSTED, 0, 0) => CapError::ASIDPoolExhausted,
            (code::NO_ASID_ASSIGNED, 0, 0) => CapError::NoASIDAssigned,
            (code::INVALID_SLOT, s, 0) => CapError::InvalidSlot(KeySlot(u32::try_from(s).ok()?)),
            (code::EMPTY_SLOT, s, 0) => CapError::EmptySlot(KeySlot(u32::try_from(s).ok()?)),
            (code::SLOT_OCCUPIED, s, 0) => CapError::SlotOccupied(KeySlot(u32::try_from(s).ok()?)),
            (code::NOT_CORE_TYPE, t, 0) => {
                CapError::NotCoreType(ObjectType::from(u8::try_from(t).ok()?))
            }
            (code::UNKNOWN_CORE_TYPE, t, 0) => CapError::UnknownCoreType(u8::try_from(t).ok()?),
            (code::UNSUPPORTED_CORE_TYPE, t, 0) => {
                CapError::UnsupportedCoreType(CoreType::try_from(u8::try_from(t).ok()?).ok()?)
            }
            (code::NOT_ARCH_TYPE, t, 0) => {
                CapError::NotArchType(ObjectType::from(u8::try_from(t).ok()?))
            }
            (code::UNKNOWN_ARCH_TYPE, t, 0) => CapError::UnknownArchType(u8::try_from(t).ok()?),
            (code::UNSUPPORTED_ARCH_TYPE, t, 0) => {
                CapError::UnsupportedArchType(ArchType::try_from(u8::try_from(t).ok()?).ok()?)
            }
            (code::INVALID_OBJECT_TYPE, t, 0) => {
                CapError::InvalidObjectType(ObjectType::from(u8::try_from(t).ok()?))
            }
            (code::TYPE_MISMATCH, expected, found) => CapError::TypeMismatch {
                expected: ObjectType::from(u8::try_from(expected).ok()?),
                found: ObjectType::from(u8::try_from(found).ok()?),
            },
            (code::INSUFFICIENT_MEMORY, 0, 0) => CapError::InsufficientMemory,
            (code::POOL_EXHAUSTED, 0, 0) => CapError::PoolExhausted,
            (code::INVALID_SIZE, s, 0) => CapError::InvalidSize(usize::try_from(s).ok()?),
            (code::INVALID_FRAME_SIZE, s, 0) => {
                CapError::InvalidFrameSize(usize::try_from(s).ok()?)
            }
            _ => return None,
        })
    })();

    Err(decoded.unwrap_or(CapError::UnknownResponse {
        status,
        detail1,
        detail2,
    }))
}

pub enum CapError {
    Unknown,
    NullCapability,
    InvalidDomain,
    InvalidPointer,
    InsufficientRights,
    NotMapped,
    AlreadyMapped,
    InvalidOperation,
    ASIDPoolExhausted,
    NoASIDAssigned,
    InvalidSlot(KeySlot),
    EmptySlot(KeySlot),
    SlotOccupied(KeySlot),
    // Key types
    NotCoreType(ObjectType),
    UnknownCoreType(u8),
    UnsupportedCoreType(CoreType),
    NotArchType(ObjectType),
    UnknownArchType(u8),
    UnsupportedArchType(ArchType),
    InvalidObjectType(ObjectType),
    TypeMismatch {
        expected: ObjectType,
        found: ObjectType,
    },
    // Object pools
    InsufficientMemory,
    PoolExhausted,
    InvalidSize(usize),
    InvalidFrameSize(usize),
    /// Client-side lossless fallback, not a new wire status. The nonzero status
    /// ensures that re-encoding an error can never produce success.
    UnknownResponse {
        status: core::num::NonZeroU64,
        detail1: u64,
        detail2: u64,
    },
}

impl CapError {
    pub fn code(self) -> (u64, u64, u64) {
        match self {
            CapError::Unknown => (code::UNKNOWN, 0, 0),
            CapError::NullCapability => (code::NULL_CAPABILITY, 0, 0),
            CapError::InvalidDomain => (code::INVALID_DOMAIN, 0, 0),
            CapError::InvalidPointer => (code::INVALID_POINTER, 0, 0),
            CapError::InsufficientRights => (code::INSUFFICIENT_RIGHTS, 0, 0),
            CapError::NotMapped => (code::NOT_MAPPED, 0, 0),
            CapError::AlreadyMapped => (code::ALREADY_MAPPED, 0, 0),
            CapError::InvalidOperation => (code::INVALID_OPERATION, 0, 0),
            CapError::ASIDPoolExhausted => (code::ASID_POOL_EXHAUSTED, 0, 0),
            CapError::NoASIDAssigned => (code::NO_ASID_ASSIGNED, 0, 0),
            CapError::InvalidSlot(s) => (code::INVALID_SLOT, u64::from(s.0), 0),
            CapError::EmptySlot(s) => (code::EMPTY_SLOT, u64::from(s.0), 0),
            CapError::SlotOccupied(s) => (code::SLOT_OCCUPIED, u64::from(s.0), 0),
            CapError::NotCoreType(t) => (code::NOT_CORE_TYPE, u64::from(t.as_u8()), 0),
            CapError::UnknownCoreType(t) => (code::UNKNOWN_CORE_TYPE, u64::from(t), 0),
            CapError::UnsupportedCoreType(t) => {
                (code::UNSUPPORTED_CORE_TYPE, u64::from(t.as_u8()), 0)
            }
            CapError::NotArchType(t) => (code::NOT_ARCH_TYPE, u64::from(t.as_u8()), 0),
            CapError::UnknownArchType(t) => (code::UNKNOWN_ARCH_TYPE, u64::from(t), 0),
            CapError::UnsupportedArchType(t) => {
                (code::UNSUPPORTED_ARCH_TYPE, u64::from(t.as_u8()), 0)
            }
            CapError::InvalidObjectType(t) => (code::INVALID_OBJECT_TYPE, u64::from(t.as_u8()), 0),
            CapError::TypeMismatch { expected, found } => (
                code::TYPE_MISMATCH,
                u64::from(expected.as_u8()),
                u64::from(found.as_u8()),
            ),
            CapError::InsufficientMemory => (code::INSUFFICIENT_MEMORY, 0, 0),
            CapError::PoolExhausted => (code::POOL_EXHAUSTED, 0, 0),
            CapError::InvalidSize(s) => (code::INVALID_SIZE, s.try_into().unwrap(), 0),
            CapError::InvalidFrameSize(s) => (code::INVALID_FRAME_SIZE, s.try_into().unwrap(), 0),
            CapError::UnknownResponse {
                status,
                detail1,
                detail2,
            } => (status.get(), detail1, detail2),
        }
    }
}
