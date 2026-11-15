//! Reading the memory of the code that called into the nucleus, at that
//! code's own exception level, in the active `AddressSpace`.
//!
//! Every page is checked with `AT` before it is read, so a bad address is
//! refused instead of faulting the nucleus. An EL0 caller's memory is read
//! with the unprivileged loads `LDTRB`/`LDTR`, which check EL0's permissions:
//! with PAN set, which the cores that have it (the Raspberry Pi 5's
//! Cortex-A76) set on every exception entry, an ordinary EL1 load of memory
//! EL0 may reach faults.

/// The exception level of the code whose memory is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reader {
    /// A Thread at EL0
    El0,
    /// The boot code at EL1
    El1,
}

/// Bytes of one translation granule: the step [`readable`] checks in.
const PAGE: u64 = 4096;

/// Whether `reader` may read each of the `length` bytes at `address` in the
/// active `AddressSpace`: every page of the range translates for its read.
pub fn readable(address: u64, length: usize, reader: Reader) -> bool {
    let Some(last) = length
        .checked_sub(1)
        .and_then(|last| address.checked_add(last as u64))
    else {
        // Empty (readable) or wrapping (not).
        return length == 0;
    };
    let mut page = address & !(PAGE - 1);
    loop {
        if !translates(page, reader) {
            return false;
        }
        match page.checked_add(PAGE) {
            Some(next) if next <= last => page = next,
            _ => return true,
        }
    }
}

/// The byte at `address`, read with `reader`'s permissions.
///
/// # Safety
/// `reader` may read `address` in the active `AddressSpace` ([`readable`]),
/// which stays active through the call.
pub unsafe fn read_byte(address: u64, reader: Reader) -> u8 {
    match reader {
        Reader::El0 => {
            let byte: u32;
            // SAFETY: the caller's contract; LDTRB only reads.
            unsafe {
                core::arch::asm!(
                    "ldtrb {byte:w}, [{address}]",
                    byte = out(reg) byte,
                    address = in(reg) address,
                    options(nostack, preserves_flags, readonly)
                );
            }
            // LDTRB zero-extends the byte it loads.
            byte.to_le_bytes()[0]
        }
        // SAFETY: the caller's contract.
        Reader::El1 => unsafe { (address as *const u8).read_volatile() },
    }
}

/// The aligned 64-bit word at `address` of an EL0 Thread's memory in the
/// active `AddressSpace`; `None` if EL0 may not read it or it is not
/// aligned.
pub fn read_el0_word(address: u64) -> Option<u64> {
    if !address.is_multiple_of(8) || !translates(address, Reader::El0) {
        return None;
    }
    let word: u64;
    // SAFETY: EL0 may read the aligned word, as `AT` just found; LDTR only
    // reads.
    unsafe {
        core::arch::asm!(
            "ldtr {word}, [{address}]",
            word = out(reg) word,
            address = in(reg) address,
            options(nostack, preserves_flags, readonly)
        );
    }
    Some(word)
}

/// Whether a read of `address` at `reader`'s level translates in the active
/// `AddressSpace` (`AT S1E0R` or `AT S1E1R`).
fn translates(address: u64, reader: Reader) -> bool {
    use aarch64_cpu::registers::{PAR_EL1, Read};
    // SAFETY: `AT` only writes PAR_EL1, which nothing else holds across this
    // call: the nucleus runs with interrupts masked.
    unsafe {
        match reader {
            Reader::El0 => core::arch::asm!(
                "at s1e0r, {address}",
                "isb",
                address = in(reg) address,
                options(nostack, preserves_flags)
            ),
            Reader::El1 => core::arch::asm!(
                "at s1e1r, {address}",
                "isb",
                address = in(reg) address,
                options(nostack, preserves_flags)
            ),
        }
    }
    !PAR_EL1.is_set(PAR_EL1::F)
}
