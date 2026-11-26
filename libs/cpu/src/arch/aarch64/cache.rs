/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! Cache maintenance.
//!
//! Two kinds, as the Arm ARM separates them (D7.5, "Cache maintenance
//! instructions"):
//!
//! - by virtual address over a range of memory, to the Point of Coherency
//!   (`DC CVAC`, `DC IVAC`, `DC CIVAC`) or of Unification (`DC CVAU` and an
//!   instruction cache invalidate): what makes memory agree with another
//!   observer, a non-coherent bus master or the instruction fetch. They are
//!   broadcast to every core in the shareability domain of the address.
//!   These are what the Frame cache operations run;
//! - by set/way over every data or unified cache level up to the Point of
//!   Coherency (`DC ISW`, `DC CISW`): for boot and power transitions only,
//!   when this core alone uses the caches. They act on this core's caches
//!   and are not broadcast, and another core or a speculative fetch can
//!   refill a line behind the walk, so they are no way to make memory
//!   coherent with another core or with a device doing DMA.
//!
//! A cache maintenance API for userspace, in the manner of seL4's per-Frame
//! and per-AddressSpace cache operations, builds on the by-address range
//! functions only. The set/way functions stay in the boot path: kickstart
//! cleans and invalidates with them before it turns its data cache on.
//!
//! Each operation completes with the barrier it needs before it returns.

use {
    aarch64_cpu::registers::{CCSIDR_EL1, CLIDR_EL1, CSSELR_EL1, Read, Write},
    tock_registers::{LocalRegisterCopy, fields::Field, register_bitfields},
};

register_bitfields! {
    u64,

    /// Cache Type Register.
    pub CTR_EL0 [
        /// Log2 of the 4-byte words in the smallest instruction cache line
        IminLine OFFSET(0) NUMBITS(4) [],
        /// Log2 of the 4-byte words in the smallest data or unified cache line
        DminLine OFFSET(16) NUMBITS(4) [],
    ]
}

/// The running core's Cache Type Register.
fn cache_type() -> LocalRegisterCopy<u64, CTR_EL0::Register> {
    let raw: u64;
    // SAFETY: reading `CTR_EL0` has no side effects.
    unsafe {
        core::arch::asm!(
            "mrs {raw}, ctr_el0",
            raw = out(reg) raw,
            options(nomem, nostack, preserves_flags),
        );
    }
    LocalRegisterCopy::new(raw)
}

/// Bytes in the smallest data cache line of the running core: the step of
/// the by-address operations.
pub fn data_cache_line_bytes() -> u64 {
    4 << cache_type().read(CTR_EL0::DminLine)
}

/// Bytes in the smallest instruction cache line of the running core.
pub fn instruction_cache_line_bytes() -> u64 {
    4 << cache_type().read(CTR_EL0::IminLine)
}

/// Issue one data-cache maintenance instruction by virtual address.
macro_rules! data_cache_line_op {
    ($instruction:literal, $address:expr) => {
        // SAFETY: the caller of the enclosing function vouches that the line
        // is mapped; the operation changes no memory contents beyond what it
        // defines, and the enclosing function completes it with a barrier.
        unsafe {
            core::arch::asm!(
                concat!("dc ", $instruction, ", {address}"),
                address = in(reg) $address,
                options(nostack, preserves_flags),
            );
        }
    };
}

/// The start of each data cache line touching `[start, end)`, and whether
/// the line lies wholly inside it.
fn lines(start: u64, end: u64) -> impl Iterator<Item = (u64, bool)> {
    let line = data_cache_line_bytes();
    let first = start & !(line - 1);
    (0..)
        .map(move |index| first + index * line)
        .take_while(move |&address| address < end)
        .map(move |address| (address, address >= start && address + line <= end))
}

/// Complete data-cache maintenance before the code after it, including
/// towards a non-coherent bus master.
fn complete_data_maintenance() {
    // SAFETY: a barrier only orders.
    unsafe {
        core::arch::asm!("dsb sy", options(nostack, preserves_flags));
    }
}

/// Write the dirty data-cache lines of `[start, end)` back to the Point of
/// Coherency (`DC CVAC`).
///
/// # Safety
/// The range is mapped.
pub unsafe fn clean_data_range(start: u64, end: u64) {
    for (address, _whole) in lines(start, end) {
        data_cache_line_op!("cvac", address);
    }
    complete_data_maintenance();
}

/// Discard the data-cache lines of `[start, end)` (`DC IVAC`); a line only
/// partly inside the range is cleaned and invalidated instead (`DC CIVAC`),
/// so that the bytes outside the range keep their values.
///
/// # Safety
/// The range is mapped, and what the cache held for it may be lost: the
/// memory holds what the caller wants to read next.
pub unsafe fn invalidate_data_range(start: u64, end: u64) {
    for (address, whole) in lines(start, end) {
        if whole {
            data_cache_line_op!("ivac", address);
        } else {
            data_cache_line_op!("civac", address);
        }
    }
    complete_data_maintenance();
}

/// Write back, then discard, the data-cache lines of `[start, end)`
/// (`DC CIVAC`).
///
/// # Safety
/// The range is mapped.
pub unsafe fn clean_invalidate_data_range(start: u64, end: u64) {
    for (address, _whole) in lines(start, end) {
        data_cache_line_op!("civac", address);
    }
    complete_data_maintenance();
}

/// Make instructions written to `[start, end)` visible to instruction fetch:
/// clean the data-cache lines to the Point of Unification (`DC CVAU`), then
/// invalidate every instruction cache of the inner shareable domain (`IC
/// IALLUIS`, which also covers aliasing instruction caches).
///
/// # Safety
/// The range is mapped.
pub unsafe fn unify_instruction_range(start: u64, end: u64) {
    for (address, _whole) in lines(start, end) {
        data_cache_line_op!("cvau", address);
    }
    // SAFETY: completes the cleans, drops cached instructions, and
    // synchronizes the instruction stream.
    unsafe {
        core::arch::asm!("dsb ish", "ic ialluis", "dsb ish", "isb", options(nostack));
    }
}

/// The geometry of one cache, as `CCSIDR_EL1` gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheGeometry {
    /// Log2 of the bytes in a line
    pub line_shift: u32,
    pub ways: u32,
    pub sets: u32,
}

impl CacheGeometry {
    /// The geometry `CCSIDR_EL1`'s fields encode: `LineSize` is log2 of the
    /// line's bytes less 4, the associativity and the set count are each
    /// one less than the number. (The fields are 3, 21 and 24 bits wide.)
    pub fn from_fields(line_size: u64, associativity: u64, sets: u64) -> Self {
        let field = |value: u64| u32::try_from(value).unwrap_or(u32::MAX);
        Self {
            line_shift: field(line_size).saturating_add(4),
            ways: field(associativity).saturating_add(1),
            sets: field(sets).saturating_add(1),
        }
    }

    /// The operand of `DC ISW` and `DC CISW` for `set` and `way` of this
    /// cache at `level` (0 for L1). The Arm ARM's `SetWay` layout: the way in
    /// the top bits from 32 − ⌈log2 ways⌉, the set from bit log2 of the line
    /// length, the level less one in bits 3:1.
    pub const fn set_way(&self, level: u32, set: u32, way: u32) -> u64 {
        let way_bits = self.ways.next_power_of_two().trailing_zeros();
        let way_field = if way_bits == 0 {
            0
        } else {
            (way as u64) << (32 - way_bits)
        };
        way_field | ((set as u64) << self.line_shift) | ((level as u64) << 1)
    }
}

/// The `CLIDR_EL1` cache type field of each level, L1 first.
const CACHE_TYPES: [Field<u64, CLIDR_EL1::Register>; 7] = [
    CLIDR_EL1::Ctype1,
    CLIDR_EL1::Ctype2,
    CLIDR_EL1::Ctype3,
    CLIDR_EL1::Ctype4,
    CLIDR_EL1::Ctype5,
    CLIDR_EL1::Ctype6,
    CLIDR_EL1::Ctype7,
];

/// Whether a `CLIDR_EL1` cache type holds data: a data-only, separate or
/// unified cache.
const fn holds_data(cache_type: u64) -> bool {
    cache_type == CLIDR_EL1::Ctype1::DataCacheOnly.value
        || cache_type == CLIDR_EL1::Ctype1::SeparateInstructionAndDataCaches.value
        || cache_type == CLIDR_EL1::Ctype1::UnifiedCache.value
}

/// The data cache levels up to the Point of Coherency.
fn levels_to_coherency() -> usize {
    usize::try_from(CLIDR_EL1.read(CLIDR_EL1::LoC)).unwrap_or(0)
}

/// Run `operation` on the operand of every set and way of the first `levels`
/// data or unified cache levels, L1 first.
fn each_set_way(levels: usize, mut operation: impl FnMut(u64)) {
    for (level, cache_type) in (0..).zip(CACHE_TYPES).take(levels) {
        if !holds_data(CLIDR_EL1.read(cache_type)) {
            continue;
        }
        CSSELR_EL1.write(CSSELR_EL1::Level.val(u64::from(level)) + CSSELR_EL1::InD::Data);
        // SAFETY: makes the selection visible to the `CCSIDR_EL1` read.
        unsafe {
            core::arch::asm!("isb", options(nostack, preserves_flags));
        }
        let geometry = CacheGeometry::from_fields(
            CCSIDR_EL1.read(CCSIDR_EL1::LineSize),
            CCSIDR_EL1.get_associativity(),
            CCSIDR_EL1.get_num_sets(),
        );
        for way in 0..geometry.ways {
            for set in 0..geometry.sets {
                operation(geometry.set_way(level, set, way));
            }
        }
    }
    // SAFETY: completes the maintenance and synchronizes what follows.
    unsafe {
        core::arch::asm!("dsb sy", "isb", options(nostack, preserves_flags));
    }
}

/// Write back and discard every line of every data and unified cache level
/// up to the Point of Coherency (`DC CISW`), on this core.
///
/// For boot, before this code turns its data cache on: nothing it reads
/// then can come from a stale line, and the dirty lines of whoever ran with
/// caches on before (firmware resident at EL3) reach memory instead of being
/// lost.
///
/// Set/way maintenance is local to this core: levels shared with other cores
/// (the Pi 3 and 4's L2) are walked too, so run it while no other core has
/// its caches on.
pub fn clean_invalidate_data_caches() {
    each_set_way(levels_to_coherency(), clean_invalidate_set_way);
}

/// Write back and discard every line of this core's L1 data cache (`DC
/// CISW`), the level private to the core on every supported Arm core: for a
/// secondary core before it turns its caches on, while other cores already
/// run with theirs, without walking the levels they share.
pub fn clean_invalidate_l1_data_cache() {
    each_set_way(1, clean_invalidate_set_way);
}

/// `DC CISW` of one set/way operand.
fn clean_invalidate_set_way(operand: u64) {
    // SAFETY: cleaning and invalidating by set/way keeps every value in
    // memory or cache; it changes no memory contents.
    unsafe {
        core::arch::asm!("dc cisw, {operand}", operand = in(reg) operand, options(nostack, preserves_flags));
    }
}

/// Discard every line of every data and unified cache level up to the Point
/// of Coherency (`DC ISW`), on this core.
///
/// # Safety
/// Nothing the caches hold dirty is needed any more: their dirty lines are
/// lost, whoever wrote them.
pub unsafe fn invalidate_data_caches() {
    each_set_way(levels_to_coherency(), |operand| {
        // SAFETY: the caller's contract.
        unsafe {
            core::arch::asm!("dc isw, {operand}", operand = in(reg) operand, options(nostack, preserves_flags));
        }
    });
}
