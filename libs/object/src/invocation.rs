use crate::CapError;

/// Operations on an `Invocation` capability.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InvocationOp {
    /// Call the capability's interface function.
    Call = 0,
    /// Return from the current invocation, popping the calling Thread's own
    /// continuation record. Valid only on the fixed kernel-installed return
    /// key (`KeySlot::INVOCATION_RETURN`); not yet dispatched by the kernel.
    Return = 1,
}

impl TryFrom<u64> for InvocationOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, CapError> {
        match value {
            0 => Ok(Self::Call),
            1 => Ok(Self::Return),
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
