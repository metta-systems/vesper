//! Reading the memory of the code that called into the nucleus, at that
//! code's own exception level, in the active `AddressSpace`.
//!
//! Every page is checked with `AT` before it is read, so a bad address is
//! refused instead of faulting the nucleus. An EL0 caller's memory is read
//! with the unprivileged loads `LDTRB`/`LDTR`, which check EL0's permissions:
//! with PAN set, which the cores that have it (the Raspberry Pi 5's
//! Cortex-A76) set on every exception entry, an ordinary EL1 load of memory
//! EL0 may reach faults.
//!
//! [`copy_to_address_space`] and [`copy_from_address_space`] reach the EL0
//! memory of any `AddressSpace`, active or not, by walking its translation
//! tables in software and going through the kernel's direct map. Nothing
//! uses them yet: they are kept for a DebugConsole that reads its caller's
//! buffer by address. Fault frames go to a registered Frame instead (the
//! stack pool's fault area). The physical page a walk finds is not pinned:
//! on several cores, another core could unmap it between the walk and the
//! copy, so a user would have to hold off unmapping for that long.

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

/// The physical address `vaddr` maps to in the tables rooted at
/// `root_paddr`, if EL0 may access it there: readable, and writable too when
/// `write` is set. The mapping must be Normal cacheable memory (device and
/// write-combining mappings are refused), with its access flag set.
///
/// The `AddressSpace` need not be the active one: the walk follows its own
/// tables, through the direct map.
pub fn user_page(root_paddr: u64, vaddr: u64, write: bool) -> Option<u64> {
    use crate::objects::arch::page_table::{ENTRY_ADDR_MASK, VA_BITS, pte, raw_table, slot_index};

    if vaddr >> VA_BITS != 0 {
        return None;
    }
    let mut table = root_paddr;
    for level in 0..=3_u8 {
        // SAFETY: the root is a bound `AddressSpace`'s translation root and
        // each step follows a valid table descriptor `install_table_entry`
        // installed, so every address names a live carved table.
        let entry = unsafe { raw_table(table) }.entries[slot_index(level, vaddr)];
        if entry & pte::VALID == 0 {
            return None;
        }
        let leaf = level == 3 || entry & pte::DESCRIPTOR == 0;
        if !leaf {
            table = entry & ENTRY_ADDR_MASK;
            continue;
        }
        if level == 0 || (level == 3 && entry & pte::DESCRIPTOR == 0) {
            // No block at level 0, and a level-3 entry must be a page.
            return None;
        }
        let ap = entry & (0b11 << 6);
        let el0_access = if write {
            ap == pte::AP_RW_USER
        } else {
            ap == pte::AP_RW_USER || ap == pte::AP_RO_USER
        };
        let normal = entry & (0b111 << 2) == pte::ATTR_NORMAL;
        if !el0_access || !normal || entry & pte::AF == 0 {
            return None;
        }
        let span_bits = 39 - 9 * u32::from(level);
        let offset = vaddr & ((1 << span_bits) - 1);
        return Some((entry & ENTRY_ADDR_MASK & !((1 << span_bits) - 1)) | offset);
    }
    None
}

/// Copy `length` bytes at `vaddr` of the `AddressSpace` rooted at `root`,
/// page by page: `copy` gets each piece's physical address and its range of
/// the buffer. Every page is checked first, so either all of it is copied or
/// none (`false`).
fn walk_copy(
    root: u64,
    vaddr: u64,
    length: usize,
    write: bool,
    mut copy: impl FnMut(u64, core::ops::Range<usize>),
) -> bool {
    let Some(end) = vaddr.checked_add(length as u64) else {
        return false;
    };
    let mut page = vaddr & !(PAGE - 1);
    while page < end {
        if user_page(root, page, write).is_none() {
            return false;
        }
        page += PAGE;
    }
    let mut done = 0;
    while done < length {
        let at = vaddr + done as u64;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a page offset is below 4096"
        )]
        let chunk = (length - done).min((PAGE - (at & (PAGE - 1))) as usize);
        let Some(paddr) = user_page(root, at, write) else {
            return false;
        };
        copy(paddr, done..done + chunk);
        done += chunk;
    }
    true
}

/// Copy `bytes` to `vaddr` in the `AddressSpace` whose translation root is
/// `root_paddr`, through the kernel's direct map; that `AddressSpace` need
/// not be the active one. Every byte must be EL0-writable Normal memory
/// there, or nothing is written and the result is `false`.
pub fn copy_to_address_space(root_paddr: u64, vaddr: u64, bytes: &[u8]) -> bool {
    walk_copy(root_paddr, vaddr, bytes.len(), true, |paddr, range| {
        // SAFETY: `walk_copy` found the page EL0-writable Normal memory of
        // that `AddressSpace`; the direct map reaches all of RAM.
        unsafe {
            core::ptr::copy_nonoverlapping(
                bytes[range.clone()].as_ptr(),
                libaddress::PhysAddr::new(paddr)
                    .user_to_kernel()
                    .as_mut_ptr::<u8>(),
                range.len(),
            );
        }
    })
}

/// Copy from `vaddr` in the `AddressSpace` rooted at `root_paddr` into
/// `bytes`, as [`copy_to_address_space`] writes: every byte must be
/// EL0-readable Normal memory, or the result is `false`.
pub fn copy_from_address_space(root_paddr: u64, vaddr: u64, bytes: &mut [u8]) -> bool {
    let length = bytes.len();
    walk_copy(root_paddr, vaddr, length, false, |paddr, range| {
        // SAFETY: as in `copy_to_address_space`, for EL0-readable memory.
        unsafe {
            core::ptr::copy_nonoverlapping(
                libaddress::PhysAddr::new(paddr)
                    .user_to_kernel()
                    .as_ptr::<u8>(),
                bytes[range.clone()].as_mut_ptr(),
                range.len(),
            );
        }
    })
}
