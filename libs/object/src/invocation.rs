use crate::{CapError, Key, RawKey, decode_syscall_result};

#[cfg(not(test))]
use libsyscall::{ppc_call, ppc_call_with_key};
#[cfg(test)]
use tests::{ppc_call, ppc_call_with_key};

#[cfg(test)]
#[path = "../tests/support/invocation.rs"]
mod tests;

/// Maximum number of nested PPC continuations retained by one Thread.
///
/// This is a shared contract constant: a Call at this depth is rejected with
/// `NestingDepth { count: INVOCATION_STACK_DEPTH }`.
pub const INVOCATION_STACK_DEPTH: usize = 16;

/// Operations on an `Invocation` capability.
/// Return is `Thread.Return` on the current-relative `CurrentReturnOnly` selector.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InvocationOp {
    /// Call the capability's interface function with data only.
    Call = 0,
    /// Call the interface function and lend one key to the callee for the
    /// duration of the call.
    CallWithKey = 1,
}

impl TryFrom<u64> for InvocationOp {
    type Error = CapError;

    fn try_from(value: u64) -> Result<Self, CapError> {
        match value {
            0 => Ok(Self::Call),
            1 => Ok(Self::CallWithKey),
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

/// The key-placement window an exporter declares at
/// `AddressSpace.CreateInvocation`: `count` consecutive slots of the target
/// `AddressSpace`'s `KeyTable` starting at `first`, where `CallWithKey` places the
/// lent key. A count of zero means the Invocation accepts no lent key.
///
/// Wire form (`x8` of `CreateInvocation`): `first` in bits 31:0, `count` in
/// bits 63:32.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PlacementWindow {
    first: u32,
    count: u32,
}

impl PlacementWindow {
    /// The window of an Invocation that accepts no lent key.
    pub const NONE: Self = Self { first: 0, count: 0 };

    /// A window of `count` slots starting at slot `first`. A zero `count` is
    /// the empty window, whatever `first` is.
    pub const fn new(first: u32, count: u32) -> Self {
        if count == 0 {
            Self::NONE
        } else {
            Self { first, count }
        }
    }

    /// Decode the `x8` word of `CreateInvocation`.
    pub const fn from_wire(word: u64) -> Self {
        // Truncation selects the two 32-bit halves by construction.
        #[allow(clippy::cast_possible_truncation)]
        Self::new(word as u32, (word >> 32) as u32)
    }

    /// The `x8` word of `CreateInvocation`.
    pub const fn to_wire(self) -> u64 {
        (self.first as u64) | ((self.count as u64) << 32)
    }

    /// First slot of the window.
    pub const fn first(self) -> u32 {
        self.first
    }

    /// Number of slots in the window.
    pub const fn count(self) -> u32 {
        self.count
    }

    /// Whether `CallWithKey` may place a key (the window is not empty).
    pub const fn accepts_keys(self) -> bool {
        self.count != 0
    }

    /// The window's end (exclusive), or `None` if it overflows the slot space.
    pub const fn end(self) -> Option<u32> {
        self.first.checked_add(self.count)
    }
}

/// PPC capability: an entry point in a target `AddressSpace`.
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

    /// `Invocation.CallWithKey` (op 1): as [`call`](Self::call), and lend
    /// `lent_key` (a key in this component's own `KeyTable` carrying `LEND`) to
    /// the target for the duration of the call. The target receives the key
    /// installed in its own table in `x0`, the kernel removes it when the call
    /// returns, and the lender keeps its original key throughout.
    ///
    /// Rejections happen before migration and leave this Thread and both
    /// tables unchanged: `KeyNotAccepted` (the Invocation declared no
    /// key-placement window), a lookup error or `InsufficientRights` (no
    /// `LEND`) for the lent key, `InvalidObjectType` for a kind that cannot be
    /// lent, then the stack and depth errors of `call`, then `NoPlacementSlot`.
    ///
    /// # Safety
    /// As [`call`](Self::call); additionally the target may use the lent key,
    /// within its rights, for as long as the call lasts.
    pub unsafe fn call_with_key(
        &self,
        lent_key: RawKey,
        args: [u64; 6],
        target_sp: u64,
    ) -> Result<(u64, u64), CapError> {
        // SAFETY: forwarded caller contract; ppc_call_with_key declares every
        // register the kernel may rewrite.
        let response =
            unsafe { ppc_call_with_key(self.key.to_wire(), lent_key.to_wire(), args, target_sp) };
        decode_syscall_result(response)
    }
}
