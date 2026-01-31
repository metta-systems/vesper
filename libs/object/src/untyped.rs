use crate::{api::domain::CAPTBL_SELF, key::Key};

// ==================================================
// == Public user interface, usable from userspace ==
// ==================================================

pub struct UntypedKey {
    key: Key<Untyped>,
}

#[repr(u8)]
pub enum UntypedOp {
    Retype = 0,
}

// Errors that can occur during retype operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum RetypeError {
    /// Invalid object type specified
    InvalidObjectType = 1,
    /// Requested size is invalid for object type
    InvalidSize = 2,
    /// Not enough memory remaining in untyped
    InsufficientMemory = 3,
    /// Alignment requirements not met
    AlignmentError = 4,
    /// Destination slot already occupied
    SlotOccupied = 5,
    /// Invalid destination slot
    InvalidSlot = 6,
    /// Untyped has already been retyped (has children)
    AlreadyRetyped = 7,
    /// Object type requires specific size_bits
    SizeMismatch = 8,
    /// Maximum number of objects reached
    ObjectLimitReached = 9,
    /// Internal kernel error
    InternalError = 10,
}

impl RetypeError {
    pub fn code(self) -> u32 {
        self as u32
    }
}

// ┌─────────────────────────────────────────────────────────────────┐
// │  ALLOWED OPERATIONS ON UNTYPED                                  │
// ├─────────────────────────────────────────────────────────────────┤
// │  ✓ seL4_Untyped_Retype  → Create children (objects/sub-untypeds)│
// │  ✓ seL4_CNode_Revoke    → Delete all children, reset watermark  │
// │  ✓ seL4_CNode_Delete    → Delete this cap (if no children)      │
// │  ✓ seL4_CNode_Move      → Move cap to different slot            │
// ├─────────────────────────────────────────────────────────────────┤
// │  DISALLOWED                                                     │
// ├─────────────────────────────────────────────────────────────────┤
// │  ✗ seL4_CNode_Copy      → Cannot duplicate                      │
// │  ✗ seL4_CNode_Mint      → Cannot derive with reduced rights     │
// │  ✗ seL4_CNode_Mutate    → Cannot modify                         │
// └─────────────────────────────────────────────────────────────────┘

impl UntypedKey {
    /// Retype untyped memory into a typed nucleus object.
    ///
    /// This is how ALL nucleus objects are created (seL4 pattern).
    /// The untyped capability is consumed/reduced by the operation.
    pub fn retype(
        &self,
        object_type: ObjectType,
        size_bits: u8, // log2 of size (for variable-size objects)
        dest_slot: CapSlot,
    ) -> Result<(), RetypeError> {
        let ret = unsafe {
            crate::syscall::protected_call3(
                self.key.slot as u64,
                UntypedOp::Retype,
                object_type as u64,
                dest_slot as u64,
                size_bits as u64,
            )
        };
        Error::from_code(ret)
    }
}
