//! Common PPC export adapter: the userspace side of a component entry point.
//!
//! The `AddressSpace` builder installs the `CurrentReturnOnly` sentinel at
//! `KeySlot::THREAD_RETURN` while provisioning the `AddressSpace` and passes the
//! resulting table-local key to the component's init, which records it with
//! [`init_return_key`]. How the builder transports the key to init is part of
//! the open init-handoff design.
//!
//! Each exported procedure gets a non-returning `extern "C"` wrapper from
//! [`ppc_export!`](crate::ppc_export): it takes two ignored dummy words and the
//! six real inputs (`x2..x7`), calls its linked body normally, and completes
//! the invocation with [`complete_export`]. A Return the kernel rejects goes,
//! with the body's original result words, to the image-supplied non-returning
//! handler `vesper_thread_return_fault`; the adapter never retries the Return
//! or re-runs the body.

use {
    crate::{CapError, RawKey, thread::ThreadReturnKey},
    core::{
        convert::Infallible,
        sync::atomic::{AtomicU64, Ordering},
    },
};

/// The two `u64` result words a body returns in x0/x1 (`#[repr(C)]`, no
/// hidden result pointer under AAPCS64). Experimental convention, not frozen.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PpcResult {
    pub r0: u64,
    pub r1: u64,
}

/// The adapter-owned 16-byte target-stack area holding `(r0, r1)` across key
/// loading, the Return attempt and the fault handoff.
#[repr(C, align(16))]
struct ResultSpill([u64; 2]);

const _: () = {
    assert!(size_of::<PpcResult>() == 16);
    assert!(align_of::<PpcResult>() == 8);
    assert!(core::mem::offset_of!(PpcResult, r0) == 0);
    assert!(core::mem::offset_of!(PpcResult, r1) == 8);
    assert!(size_of::<ResultSpill>() == 16);
    assert!(align_of::<ResultSpill>() == 16);
};

/// This component's table-local Return key, recorded once by component init.
/// Zero (never set) is not a valid key: Return then fails with an ordinary
/// lookup error that reaches the fault handler.
static RETURN_KEY: AtomicU64 = AtomicU64::new(0);

/// Component init: record the Return key the `AddressSpace` builder passed in.
pub fn init_return_key(key: &ThreadReturnKey) {
    RETURN_KEY.store(key.raw().to_wire(), Ordering::Release);
}

/// The Return key recorded by [`init_return_key`].
pub fn return_key() -> ThreadReturnKey {
    ThreadReturnKey::from_key(RawKey::from_wire(RETURN_KEY.load(Ordering::Acquire)))
}

#[cfg(not(test))]
unsafe extern "C" {
    /// Image-supplied, link-resolved libOS handler for a rejected Return:
    /// `(status, detail1, detail2)` from [`CapError::code`] plus the body's
    /// original `(r0, r1)`. It must not return to the completed adapter; it
    /// may diagnose, repair and explicitly retry Return, or terminate.
    fn vesper_thread_return_fault(
        status: u64,
        detail1: u64,
        detail2: u64,
        original_r0: u64,
        original_r1: u64,
    ) -> !;
}

/// Complete the current export: spill `result`, Return through the recorded
/// key and, if that is rejected, hand off to `vesper_thread_return_fault`.
///
/// # Safety
/// Must run as the tail of a PPC export entered by `Invocation.Call` on this
/// Thread; a successful Return abandons this stack and context.
#[cfg(not(test))]
#[inline]
pub unsafe fn complete_export(result: PpcResult) -> ! {
    // SAFETY: forwarded caller contract; the handler is supplied by the image.
    unsafe {
        complete_with(
            result,
            &return_key(),
            |key, r0, r1| key.return_from_invocation(r0, r1),
            |status, detail1, detail2, r0, r1| {
                vesper_thread_return_fault(status, detail1, detail2, r0, r1)
            },
        )
    }
}

/// The adapter sequence with injectable Return and fault steps.
///
/// # Safety
/// `return_step` must be the Return helper or an equivalent that only returns
/// on rejection; `fault_step` must not return to this adapter.
#[inline]
pub unsafe fn complete_with(
    result: PpcResult,
    key: &ThreadReturnKey,
    return_step: impl FnOnce(&ThreadReturnKey, u64, u64) -> Result<Infallible, CapError>,
    fault_step: impl FnOnce(u64, u64, u64, u64, u64) -> !,
) -> ! {
    // Materialize the ordered pair in its 16-byte stack area before anything
    // that can clobber registers; later loads read memory, not stale copies.
    let spill = ResultSpill([result.r0, result.r1]);
    let spilled = core::ptr::from_ref(&spill);
    // SAFETY: `spilled` points at the live, aligned local above.
    let load = || unsafe { core::ptr::read_volatile(spilled).0 };
    let [r0, r1] = load();
    let Err(error) = return_step(key, r0, r1);
    let (status, detail1, detail2) = error.code();
    let [original_r0, original_r1] = load();
    fault_step(status, detail1, detail2, original_r0, original_r1)
}

/// Define a PPC export entry wrapper for a linked body.
///
/// `ppc_export!(pub fn entry => body);` generates
/// `extern "C" fn entry(_: u64, _: u64, a0..a5: u64) -> !`, whose address is
/// what `AddressSpace.CreateInvocation` publishes. The body is an
/// `extern "C" fn(u64, u64, u64, u64, u64, u64) -> PpcResult`, called
/// normally so its linkage is target-local. The kernel enters the wrapper
/// with a zero x30, so the wrapper never returns normally.
#[macro_export]
macro_rules! ppc_export {
    ($(#[$meta:meta])* $visibility:vis fn $entry:ident => $body:path) => {
        $(#[$meta])*
        $visibility extern "C" fn $entry(
            _dummy0: u64,
            _dummy1: u64,
            input0: u64,
            input1: u64,
            input2: u64,
            input3: u64,
            input4: u64,
            input5: u64,
        ) -> ! {
            let result: $crate::export::PpcResult =
                $body(input0, input1, input2, input3, input4, input5);
            // SAFETY: this wrapper is only ever entered by Invocation.Call.
            unsafe { $crate::export::complete_export(result) }
        }
    };
}

#[cfg(test)]
#[path = "../tests/support/export.rs"]
mod tests;
