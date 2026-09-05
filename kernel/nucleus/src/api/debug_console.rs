use {
    crate::{api::KeyEntry, objects::DebugConsole},
    libaddress::PhysAddr,
    libobject::{CapError, ObjectType, SyscallResult, debug_console::DebugConsoleOp},
};

// =====================
// == Syscall handler ==
// =====================

// Debug-only prototype: compiled only with the opt-in `debug_kernel` feature.
// This is not a generally available service or a safe interface for untrusted
// callers. The maintainer has explicitly retained the current mechanism for now;
// see doc/nucleus_capabilities.md, "DebugConsole debug-only exception".
//
// Deferred repairs before general availability (not approved ABI changes):
// - Establish an explicit caller/principal and console-write right/bootstrap
//   grant; do not infer domain zero from no current domain. The active caller is
//   trusted EL1h boot code, not yet an EL0 domain. Define permitted origins.
// - Classify EC/SVC immediate before dispatch; reject oversized raw slots/ops
//   without panics. Route non-SVC faults separately, and never recursively invoke
//   capabilities on a copy fault while nucleus/object guards are held.
// - Treat input as caller virtual memory, not PhysAddr plus a direct-map offset.
//   Authorize the whole readable range against the caller, check length/overflow,
//   stabilize backing/mappings and snapshot input, and provide scoped copy-fault
//   recovery. A translated/kernel-readable address alone does not grant access.
// - Specify bytes versus UTF-8/C strings, embedded NUL and empty-input behavior,
//   a maximum length or chunking policy, and partial-output/progress semantics.
//   The current 4096-byte buffer needs terminator space and has no length check.
// - Decode/propagate kernel results in the client and gate routine semihosting
//   diagnostics; define backend availability (actual output currently needs qemu).
// - Test success/errors, missing rights, empty/invalid slots, wide raw arguments,
//   length boundaries, unauthorized/faulting pointers, exception routing and
//   recovery, register preservation, and both feature-off/on configurations.
//
// Alternatives requiring a separate decision: restrict pointer writes to registered
// bootstrap buffers, or add a distinct register-inline byte operation (for example
// up to 40 bytes/call with client chunking) to avoid user-copy dependencies. Do not
// silently reinterpret Write=0. Full caller-virtual writes need D1/D6 memory and
// lifetime guarantees; new schemas/rights/results need the relevant D4/D9 decisions.
#[inline]
pub fn invoke(cap: &KeyEntry, op: u32, arg0: u64, arg1: u64) -> SyscallResult {
    // The console has no per-object state. Validate the capability header without
    // turning its stored pointer into a reference; this is not a rights check.
    if cap.object_type() != ObjectType::DEBUG_CONSOLE {
        return Err(CapError::TypeMismatch {
            expected: ObjectType::DEBUG_CONSOLE,
            found: cap.object_type(),
        });
    }
    let op = DebugConsoleOp::try_from(op)?;

    libqemu::semihosting::println!("DebugConsole:invoke");

    match op {
        DebugConsoleOp::Write => {
            // TODO: validate client phys ptr validity
            DebugConsole::handle_write(PhysAddr::new(arg0), arg1)?;
            Ok((0, 0))
        }
    }
}
