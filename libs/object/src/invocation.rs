use crate::CapError;

/// Maximum number of nested PPC continuations retained by one Thread.
///
/// This is a shared contract constant; storage and Call admission are separate
/// implementation steps.
pub const INVOCATION_STACK_DEPTH: usize = 16;

/// Operations on a Call-only `Invocation` capability.
/// Return is `Thread.Return` on the current-relative `CurrentReturnOnly` selector.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InvocationOp {
    /// Call the capability's interface function.
    Call = 0,
}

impl TryFrom<u64> for InvocationOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, CapError> {
        match value {
            0 => Ok(Self::Call),
            _ => Err(CapError::InvalidOperation),
        }
    }
}

impl TryFrom<u32> for InvocationOp {
    type Error = CapError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::try_from(u64::from(value))
    }
}
