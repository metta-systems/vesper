use crate::CapError;

/// Operations on an `Invocation` capability.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InvocationOp {
    /// Call the capability's interface function.
    Call = 0,
}

impl TryFrom<u64> for InvocationOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
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
