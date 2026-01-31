#![no_std]

pub mod debug_console;
pub mod key;
pub mod notification;
pub mod object_type;

pub use {key::Key, key_table::KeySlot, object_type::ObjectType};

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
    NotCoreType,
    UnknownCoreType(u32),
    NotArchType(u32),
    UnknownArchType(u32),
    UnsupportedArchType(u32),
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
