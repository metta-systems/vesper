#![no_std]

//! Stack traces from frame pointers, for panics and exceptions.
//!
//! An `AArch64` function that keeps a frame record stores it at its frame
//! pointer, `x29`: the caller's frame pointer at `[x29]` and the return
//! address at `[x29 + 8]`. Following the chain from a frame pointer gives the
//! return addresses of the calls that led there. [`frames`] follows it with a
//! read function the caller gives, so the same walk reads the running code's
//! own stack, a component's stack from the nucleus, or a modelled stack in
//! the host tests.
//!
//! The chain exists only in code compiled with frame pointers kept. A build
//! asks for traces with `--cfg stacktrace` together with
//! `-C force-frame-pointers=yes` (`just stacktrace=on …`). Without
//! `stacktrace`, [`ENABLED`] is false: [`current_frame_pointer`] returns 0 and
//! a [`Backtrace`] prints nothing, so the callers compile to nothing.
//!
//! The walk stops at a frame pointer of 0 (the first frame of a Thread), one
//! that is not 16-byte aligned, a caller's record that does not lie above the
//! current one within [`MAX_FRAME_BYTES`], a return address of 0, an unread
//! word, or after [`MAX_FRAMES`] frames.

use core::fmt;

/// Whether this build records stack traces (`--cfg stacktrace`).
pub const ENABLED: bool = cfg!(stacktrace);

/// Most return addresses a trace lists.
pub const MAX_FRAMES: usize = 16;

/// Farthest a caller's frame record may lie above its callee's: the chain is
/// taken as broken beyond it.
pub const MAX_FRAME_BYTES: u64 = 64 * 1024;

/// Alignment of every frame record (the stack pointer's).
const RECORD_ALIGN: u64 = 16;

/// Offset of the return address within a frame record.
const RETURN_ADDRESS_OFFSET: u64 = 8;

/// The return addresses along the frame chain starting at a frame pointer,
/// innermost first ([`frames`]).
pub struct Frames<Read> {
    frame_pointer: u64,
    remaining: usize,
    read: Read,
}

impl<Read: FnMut(u64) -> Option<u64>> Iterator for Frames<Read> {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        let record = self.frame_pointer;
        if self.remaining == 0 || record == 0 || !record.is_multiple_of(RECORD_ALIGN) {
            return None;
        }
        let caller = (self.read)(record)?;
        let return_address = (self.read)(record.checked_add(RETURN_ADDRESS_OFFSET)?)?;
        if return_address == 0 {
            return None;
        }
        self.remaining -= 1;
        // The stack grows down: a caller's record lies above its callee's.
        self.frame_pointer = if caller > record && caller - record <= MAX_FRAME_BYTES {
            caller
        } else {
            0
        };
        Some(return_address)
    }
}

/// The return addresses along the frame chain from `frame_pointer`, reading
/// each 64-bit word with `read` (`None` for a word that cannot be read).
pub fn frames<Read: FnMut(u64) -> Option<u64>>(frame_pointer: u64, read: Read) -> Frames<Read> {
    Frames {
        frame_pointer,
        remaining: MAX_FRAMES,
        read,
    }
}

/// The frame pointer of the function this is inlined into; 0 without
/// `stacktrace`.
#[inline(always)]
pub fn current_frame_pointer() -> u64 {
    #[cfg(all(stacktrace, target_arch = "aarch64"))]
    {
        let frame_pointer: u64;
        // SAFETY: reads x29 into a register; nothing else changes.
        unsafe {
            core::arch::asm!(
                "mov {frame_pointer}, x29",
                frame_pointer = out(reg) frame_pointer,
                options(nomem, nostack, preserves_flags)
            );
        }
        frame_pointer
    }
    #[cfg(not(all(stacktrace, target_arch = "aarch64")))]
    {
        0
    }
}

/// The 64-bit word at `address` of the running code's own memory.
///
/// # Safety
/// The frame chain from where the walk starts lies in memory the caller can
/// read: its own stack.
pub unsafe fn read_own(address: u64) -> Option<u64> {
    // SAFETY: the caller's contract; `Frames` reads only aligned records.
    Some(unsafe { (address as *const u64).read_volatile() })
}

/// A stack trace to print: `backtrace: 0x… 0x…`, the place it starts from
/// first when there is one (an exception's PC), then the return addresses
/// along the frame chain. Prints nothing without `stacktrace`, or when
/// there is nothing to list.
pub struct Backtrace<Read> {
    start: Option<u64>,
    frame_pointer: u64,
    read: Read,
}

impl<Read: Fn(u64) -> Option<u64>> Backtrace<Read> {
    /// The trace from `frame_pointer`, reading its words with `read`, after
    /// `start` if given.
    pub const fn new(start: Option<u64>, frame_pointer: u64, read: Read) -> Self {
        Self {
            start,
            frame_pointer,
            read,
        }
    }
}

impl<Read: Fn(u64) -> Option<u64>> fmt::Display for Backtrace<Read> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !ENABLED {
            return Ok(());
        }
        write_trace(
            out,
            self.start
                .into_iter()
                .chain(frames(self.frame_pointer, &self.read)),
        )
    }
}

/// Write `backtrace: 0x… 0x…` for `addresses`; nothing when there are none.
///
/// # Errors
///
/// What writing to `out` fails with.
pub fn write_trace(out: &mut impl fmt::Write, addresses: impl Iterator<Item = u64>) -> fmt::Result {
    let mut addresses = addresses.peekable();
    if addresses.peek().is_none() {
        return Ok(());
    }
    out.write_str("backtrace:")?;
    for address in addresses {
        write!(out, " {address:#x}")?;
    }
    Ok(())
}
