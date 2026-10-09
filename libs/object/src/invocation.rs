use crate::{CapError, Key, RawKey, decode_syscall_result};

#[cfg(not(test))]
use libsyscall::ppc_call;
#[cfg(test)]
use tests::ppc_call;

#[cfg(test)]
#[path = "../tests/support/invocation.rs"]
mod tests;

/// Maximum number of nested PPC continuations retained by one Thread.
///
/// This is a shared contract constant: a Call at this depth is rejected with
/// `NestingDepth { count: INVOCATION_STACK_DEPTH }`.
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

/// Call-only PPC capability: an entry point in a target `AddressSpace`.
///
/// Non-owning handle; the phantom type is ergonomic, not authority.
pub struct InvocationKey {
    key: Key<InvocationType>,
}

enum InvocationType {}

impl InvocationKey {
    /// Construct a handle without installing or validating authority.
    pub const fn from_key(key: RawKey) -> Self {
        Self { key: Key::new(key) }
    }

    pub const fn raw(&self) -> RawKey {
        self.key.raw()
    }

    /// `Invocation.Call` (op 0): migrate this Thread into the target
    /// `AddressSpace`, run its entry with `args` in x2..x7 on `target_sp`,
    /// and return the target's `(r0, r1)` once it completes `Thread.Return`.
    ///
    /// `target_sp` must be 16-byte aligned with `base < sp <= end` and at
    /// least the Invocation's minimum headroom above `base`; violations are
    /// `InvalidStack`. A full invocation stack is `NestingDepth`. Rejections
    /// happen before migration and leave this Thread unchanged.
    ///
    /// # Safety
    /// The target runs arbitrary interface code on this Thread and stack
    /// region `target_sp` names; the caller must supply a stack that the
    /// target component reserved for this Call and nobody else uses.
    pub unsafe fn call(&self, args: [u64; 6], target_sp: u64) -> Result<(u64, u64), CapError> {
        // SAFETY: forwarded caller contract; ppc_call declares every
        // register the kernel may rewrite.
        let response = unsafe { ppc_call(self.key.to_wire(), args, target_sp) };
        decode_syscall_result(response)
    }
}
