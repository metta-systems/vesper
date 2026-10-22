/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */

//! A flat identity map for chainboot, on while it runs and off again before it
//! jumps to the kernel.
//!
//! With the MMU off every access is to Device memory: an access that is not
//! aligned to its size faults, and a store-exclusive never succeeds (an atomic
//! read-modify-write loops for ever). QEMU forgives both and the board does not.
//!
//! RAM is mapped as Normal memory, non-cacheable: unaligned access works, and
//! because nothing is cached the kernel image and the mailbox buffers need no
//! cache maintenance. The data cache stays off, which also means exclusives still
//! do not work: chainboot must not use atomic read-modify-write. The instruction
//! cache is on, for speed, and invalidated on the way in and out. Peripherals are
//! Device memory, never executed from.
//!
//! The map is 2 MiB blocks under one level-1 table, for the first gigabyte (the
//! Raspberry Pi 3) or the first four (the Raspberry Pi 4). It works at EL2, where
//! the firmware starts the kernel, and at EL1.

use {
    aarch64_cpu::{asm::barrier, registers::*},
    arbitrary_int::{u2, u3, u27, u36},
    bitbybit::bitfield,
    core::{arch::asm, ptr},
    liblocking::{IRQSafeNullLock, interface::Mutex},
    tock_registers::interfaces::{ReadWriteable, Readable, Writeable},
};

/// Address space of one level-2 table: 512 blocks of 2 MiB.
const GIB: u64 = 1 << 30;
const BLOCK: u64 = 2 << 20;

/// Gigabytes mapped, one level-2 table each.
#[cfg(board_rpi3)]
const GIBS: usize = 1;
#[cfg(board_rpi4)]
const GIBS: usize = 4;

/// Where RAM ends and the peripherals begin.
#[cfg(board_rpi3)]
const RAM_END: u64 = 0x3f00_0000;
#[cfg(board_rpi4)]
const RAM_END: u64 = 0xfc00_0000;

/// `MAIR` attribute slots.
const DEVICE: u8 = 0;
const NORMAL_UNCACHED: u8 = 1;

/// A level-2 block descriptor: 2 MiB of memory.
#[bitfield(u64, default = 0, debug)]
struct Block {
    #[bit(0, rw)]
    valid: bool,
    /// Zero for a block.
    #[bit(1, rw)]
    table: bool,
    #[bits(2..=4, rw)]
    attribute: u3,
    /// Bit 6 is reserved as one in the EL2 regime, and lets EL0 in at EL1, where
    /// nothing runs.
    #[bits(6..=7, rw)]
    access: u2,
    #[bits(8..=9, rw)]
    shareability: u2,
    /// Without it the first access faults.
    #[bit(10, rw)]
    accessed: bool,
    /// Bits 47:21 of the address.
    #[bits(21..=47, rw)]
    address: u27,
    /// Never execute at EL1 (PXN). The EL2 regime has no such bit.
    #[bit(53, rw)]
    privileged_execute_never: bool,
    /// Never execute (XN in the EL2 regime, UXN in the EL1 one).
    #[bit(54, rw)]
    execute_never: bool,
}

/// A level-1 table descriptor, pointing at a level-2 table.
#[bitfield(u64, default = 0, debug)]
struct Table {
    #[bit(0, rw)]
    valid: bool,
    /// One for a table.
    #[bit(1, rw)]
    table: bool,
    /// Bits 47:12 of the table's address.
    #[bits(12..=47, rw)]
    address: u36,
}

/// Shareability of a block: outer shareable (Normal non-cacheable memory is
/// treated so whatever is said, Device memory too).
const OUTER_SHAREABLE: u2 = u2::new(0b10);

/// A block mapping the 2 MiB at `address`: RAM, or a peripheral. Peripherals
/// are never executed from, at EL2 or at EL1.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the address is below 2^48, so its 27 bits above the block offset fit"
)]
const fn block(address: u64, ram: bool) -> Block {
    Block::ZERO
        .with_valid(true)
        .with_table(false)
        .with_attribute(u3::new(if ram { NORMAL_UNCACHED } else { DEVICE }))
        .with_access(u2::new(0b01))
        .with_shareability(OUTER_SHAREABLE)
        .with_accessed(true)
        .with_address(u27::new((address >> 21) as u32))
        .with_execute_never(!ram)
}

/// A page of 512 descriptors, aligned as the hardware wants.
#[repr(C, align(4096))]
struct Page {
    entries: [u64; 512],
}

/// The tables: one level 1 and a level 2 for each gigabyte.
#[repr(C)]
struct Tables {
    level1: Page,
    level2: [Page; GIBS],
}

impl Tables {
    const fn new() -> Self {
        Self {
            level1: Page { entries: [0; 512] },
            level2: [const { Page { entries: [0; 512] } }; GIBS],
        }
    }

    /// Fill the tables. They are written with the MMU off, straight to memory.
    /// Where the peripherals are also never executed from by privileged code
    /// (`privileged_never`: EL1 has a bit for that).
    fn build(&mut self, privileged_never: bool) {
        let base = ptr::from_ref(&self.level2).addr() as u64;
        for (gib, level2) in self.level2.iter_mut().enumerate() {
            let first = gib as u64 * GIB;
            for (index, entry) in level2.entries.iter_mut().enumerate() {
                let address = first + index as u64 * BLOCK;
                let ram = address < RAM_END;
                *entry = block(address, ram)
                    .with_privileged_execute_never(privileged_never && !ram)
                    .raw_value();
            }
            let table = base + (gib * size_of::<Page>()) as u64;
            self.level1.entries[gib] = Table::ZERO
                .with_valid(true)
                .with_table(true)
                .with_address(u36::new(table >> 12))
                .raw_value();
        }
    }
}

static TABLES: IRQSafeNullLock<Tables> = IRQSafeNullLock::new(Tables::new());

// A 2 MiB block of RAM, executable and outer shareable, in attribute slot 1;
// one of Device memory, never executed from (bit 54), in slot 0.
const _: () = assert!(block(0x20_0000, true).raw_value() == 0x20_0000 | 0x645);
const _: () = assert!(block(0x20_0000, false).raw_value() == 0x20_0000 | 0x0040_0000_0000_0641);

/// Whether the MMU is on, at the exception level chainboot runs at.
#[must_use]
pub fn enabled() -> bool {
    match current_level() {
        ExceptionLevel::El2 => SCTLR_EL2.matches_all(SCTLR_EL2::M::Enable),
        ExceptionLevel::El1 => SCTLR_EL1.matches_all(SCTLR_EL1::M::Enable),
        ExceptionLevel::Other => false,
    }
}

/// The exception levels chainboot can map memory at.
enum ExceptionLevel {
    El1,
    El2,
    Other,
}

fn current_level() -> ExceptionLevel {
    match CurrentEL.read_as_enum(CurrentEL::EL) {
        Some(CurrentEL::EL::Value::EL2) => ExceptionLevel::El2,
        Some(CurrentEL::EL::Value::EL1) => ExceptionLevel::El1,
        _ => ExceptionLevel::Other,
    }
}

/// Invalidate the instruction cache and wait for it.
fn invalidate_instruction_cache() {
    // SAFETY: a cache maintenance instruction with no operand and no memory effect.
    unsafe {
        asm!("ic iallu", options(nostack, preserves_flags));
    }
    barrier::dsb(barrier::SY);
    barrier::isb(barrier::SY);
}

/// Turn the MMU on with the identity map.
pub fn enable() {
    let level = current_level();
    let level1 = TABLES.lock(|tables| {
        tables.build(matches!(level, ExceptionLevel::El1));
        ptr::from_ref(&tables.level1).addr() as u64
    });
    // The tables are in memory before the MMU looks at them.
    barrier::dsb(barrier::SY);
    match level {
        ExceptionLevel::El2 => enable_at_el2(level1),
        ExceptionLevel::El1 => enable_at_el1(level1),
        ExceptionLevel::Other => {}
    }
}

fn enable_at_el2(level1: u64) {
    MAIR_EL2.write(
        MAIR_EL2::Attr0_Device::nonGathering_nonReordering_noEarlyWriteAck
            + MAIR_EL2::Attr1_Normal_Outer::NonCacheable
            + MAIR_EL2::Attr1_Normal_Inner::NonCacheable,
    );
    TCR_EL2.modify(
        TCR_EL2::PS::Bits_40
            + TCR_EL2::TG0::KiB_4
            + TCR_EL2::SH0::None
            + TCR_EL2::ORGN0::NonCacheable
            + TCR_EL2::IRGN0::NonCacheable
            + TCR_EL2::T0SZ.val(25), // 39 bits of address, translation starts at level 1
    );
    TTBR0_EL2.set_baddr(level1);
    barrier::isb(barrier::SY);
    // SAFETY: invalidating the translation caches at EL2 has no operand.
    unsafe {
        asm!("tlbi alle2", options(nostack, preserves_flags));
    }
    barrier::dsb(barrier::SY);
    invalidate_instruction_cache();
    SCTLR_EL2.modify(SCTLR_EL2::M::Enable + SCTLR_EL2::I::Cacheable);
    barrier::isb(barrier::SY);
}

fn enable_at_el1(level1: u64) {
    MAIR_EL1.write(
        MAIR_EL1::Attr0_Device::nonGathering_nonReordering_noEarlyWriteAck
            + MAIR_EL1::Attr1_Normal_Outer::NonCacheable
            + MAIR_EL1::Attr1_Normal_Inner::NonCacheable,
    );
    TCR_EL1.modify(
        TCR_EL1::IPS::Bits_40
            + TCR_EL1::TG0::KiB_4
            + TCR_EL1::SH0::None
            + TCR_EL1::ORGN0::NonCacheable
            + TCR_EL1::IRGN0::NonCacheable
            + TCR_EL1::T0SZ.val(25)
            + TCR_EL1::EPD1::DisableTTBR1Walks,
    );
    TTBR0_EL1.set_baddr(level1);
    barrier::isb(barrier::SY);
    // SAFETY: invalidating the translation caches at EL1 has no operand.
    unsafe {
        asm!("tlbi vmalle1", options(nostack, preserves_flags));
    }
    barrier::dsb(barrier::SY);
    invalidate_instruction_cache();
    SCTLR_EL1.modify(SCTLR_EL1::M::Enable + SCTLR_EL1::I::Cacheable);
    barrier::isb(barrier::SY);
}

/// Turn the MMU and the instruction cache off again, and invalidate the
/// instruction cache: the kernel just written over memory chainboot ran from may
/// find old instructions there.
pub fn disable() {
    match current_level() {
        ExceptionLevel::El2 => {
            SCTLR_EL2.modify(SCTLR_EL2::M::Disable + SCTLR_EL2::I::NonCacheable);
        }
        ExceptionLevel::El1 => {
            SCTLR_EL1.modify(SCTLR_EL1::M::Disable + SCTLR_EL1::I::NonCacheable);
        }
        ExceptionLevel::Other => {}
    }
    barrier::isb(barrier::SY);
    invalidate_instruction_cache();
}
