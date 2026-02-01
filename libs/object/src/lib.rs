#![no_std]

pub mod debug_console;
pub mod key;
pub mod key_table;
pub mod object_type;
pub mod rights;

pub use {
    key::Key,
    key_table::KeySlot,
    object_type::{ArchType, CoreType, ObjectType},
    rights::Rights,
};

pub type SyscallResult = core::result::Result<(u64, u64), CapError>;

pub enum CapError {
    Unknown,
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
    NotCoreType(ObjectType),
    UnknownCoreType(u8),
    NotArchType(ObjectType),
    UnknownArchType(u8),
    UnsupportedArchType(u8),
    InsufficientMemory,
    PoolExhausted,
    InvalidObjectType(ObjectType),
    InvalidSize(usize),
    NullCapability,
    InvalidFrameSize(usize),
    TypeMismatch {
        expected: ObjectType,
        found: ObjectType,
    },
}
