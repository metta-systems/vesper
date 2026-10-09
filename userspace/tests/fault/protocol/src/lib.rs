#![no_std]

//! What fault-test's builder and its EL0 components agree on.
//!
//! The `faulter` component has a fault handler; each of its Threads plays one
//! [role](ROLE_SEQUENCE) and sets the handler [mode](MODE_SKIP) before its
//! deliberate fault. The `bare` component has no handler. Threads report to
//! the builder through the `done` Notification; a witness Thread queued after
//! a Thread that stops for good wakes the builder so it can inspect the
//! kernel's view.

use {aarch64_cpu::registers::ESR_EL1, tock_registers::LocalRegisterCopy};

/// The faulter's init page (written by the builder before it runs).
pub const INIT_VA: u64 = 0x2000_0000;
/// The 2 MiB span holding the faulter's guarded stacks.
pub const STACK_REGION: u64 = 0x3000_0000;

// ── Roles: a faulter Thread's argument ──────────────────────────────────
/// Skip a fault, retry one, take a Return fault, then report [`SEQUENCE_BIT`].
pub const ROLE_SEQUENCE: u64 = 0;
/// Fault into a handler that terminates the Thread.
pub const ROLE_TERMINATE: u64 = 1;
/// Fault into a handler that faults itself (unhandled).
pub const ROLE_NESTED: u64 = 2;
/// Fault into a handler that blocks until the builder releases it, then
/// report [`BLOCKED_BIT`].
pub const ROLE_BLOCKED: u64 = 3;
/// Fault while the handler is busy with [`ROLE_BLOCKED`] (unhandled).
pub const ROLE_WHILE_BUSY: u64 = 4;
/// Report [`WITNESS_BIT`] and park: proves the scheduler moved on.
pub const ROLE_WITNESS: u64 = 5;

// ── Handler modes ───────────────────────────────────────────────────────
/// Record the fault, resume after the instruction.
pub const MODE_SKIP: u64 = 0;
/// Retry the first fault, skip the second.
pub const MODE_RETRY_ONCE: u64 = 1;
/// Terminate the faulting Thread.
pub const MODE_TERMINATE: u64 = 2;
/// Fault inside the handler.
pub const MODE_NESTED: u64 = 3;
/// Block on the `block` Notification, then skip.
pub const MODE_BLOCK: u64 = 4;

// ── `done` Notification bits ────────────────────────────────────────────
pub const SEQUENCE_BIT: u64 = 1 << 0;
pub const BLOCKED_BIT: u64 = 1 << 1;
pub const WITNESS_BIT: u64 = 1 << 2;

/// What the builder hands the faulter.
#[repr(C)]
pub struct FaulterInit {
    /// Notification the faulter's Threads report to the builder on.
    pub done: u64,
    /// Notification the blocking handler waits on; the builder signals it.
    pub block: u64,
    /// Notification Threads park on for good (never signalled).
    pub park: u64,
    /// The faulter table's guard and capacity exponent, for its export
    /// adapter's `Thread.Return` key.
    pub guard: u64,
    pub size_bits: u64,
}

/// The faulter's init page.
///
/// # Safety
///
/// Only in the faulter, whose builder maps and fills a [`FaulterInit`] at
/// [`INIT_VA`] before it runs and never changes it afterwards.
pub unsafe fn faulter_init() -> &'static FaulterInit {
    // SAFETY: forwarded caller contract.
    unsafe { &*(INIT_VA as *const FaulterInit) }
}

/// Execute a `brk`: always a synchronous fault (`ESR_EL1.EC` 0x3C).
#[inline(never)]
pub fn breakpoint() {
    // SAFETY: `brk` touches no memory, stack or register; it only traps.
    unsafe {
        core::arch::asm!("brk #0x7", options(nomem, nostack));
    }
}

/// Whether `syndrome` (an `ESR_EL1` value) is a `brk` from `AArch64`.
pub fn is_breakpoint(syndrome: u64) -> bool {
    LocalRegisterCopy::<u64, ESR_EL1::Register>::new(syndrome).matches_all(ESR_EL1::EC::Brk64)
}
