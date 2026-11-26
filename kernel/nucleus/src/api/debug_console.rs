use {
    crate::{api::KeyEntry, objects::DebugConsole, user_memory::Reader},
    libobject::{CapError, ObjectType, SyscallResult, debug_console::DebugConsoleOp},
};

// =====================
// == Syscall handler ==
// =====================

// Debug-only prototype: compiled only with the opt-in `debug_kernel` feature.
// This is not a generally available service or a safe interface for untrusted
// callers. The maintainer has explicitly retained the current mechanism for now;
// see doc/agentic/logging-design.md, "DebugConsole debug-only exception".
//
// Write reads the caller's virtual memory: each page of the range must
// translate for a read at the caller's exception level in the active
// AddressSpace (`AT S1E0R` for an EL0 Thread, `AT S1E1R` for the EL1 boot
// code), the bytes
// are copied out before printing, and at most `WRITE_LIMIT` are printed. Output
// goes to semihosting under `qemu` (up to the first NUL) and to the board
// console otherwise.
//
// Deferred repairs before general availability (not approved ABI changes):
// - Establish an explicit caller/principal and console-write right; do not
//   infer Thread zero from no current Thread. Define permitted origins.
// - Never recursively invoke capabilities on a copy fault while nucleus/object
//   guards are held. Entry already checks the exception class and origin and
//   ignores the SVC immediate (there is one syscall). The translation check
//   keeps the copy from faulting while the range stays mapped; it is not
//   copy-fault recovery.
// - Specify embedded NUL and partial-output/progress semantics: a longer
//   message is cut at `WRITE_LIMIT` without telling the caller.
// - Gate routine semihosting diagnostics.
// - Test success/errors, missing rights, empty/invalid slots, wide raw arguments,
//   length boundaries, unauthorized/faulting pointers, exception routing and
//   recovery, register preservation, and both feature-off/on configurations.
//
// Alternatives requiring a separate decision: restrict pointer writes to registered
// bootstrap buffers, or add a distinct register-inline byte operation (for example
// up to 40 bytes/call with client chunking) to avoid user-copy dependencies. Do not
// silently reinterpret Write=0. Generally available caller-virtual writes need
// D1/D6 memory and lifetime guarantees; new schemas/rights/results need the
// relevant D4/D9 decisions.
#[inline]
pub fn invoke(cap: &KeyEntry, op: u64, arg0: u64, arg1: u64, reader: Reader) -> SyscallResult {
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
            DebugConsole::handle_write(arg0, arg1, reader)?;
            libqemu::semihosting::println!("✅ DebugConsole::Write()");
            Ok((0, 0))
        }
    }
}
