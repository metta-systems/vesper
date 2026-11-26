//! The secondary cores' entry and release (SMP design, "How the secondary cores are
//! started" and "Mixed-attribute handshake").
//!
//! Kickstart [`release`]s the cores at EL2 just before its own drop to EL1. A released
//! core starts at `_secondary_entry` at EL2 with its MMU and caches off, on Kickstart's
//! image at its physical address. It finds its parameters by its own affinity in
//! [`SECONDARY_BOOT`], which Kickstart wrote with plain stores and cleaned to the Point of
//! Coherency before the release: the spin table passes no argument, and PSCI's context id
//! is not needed. It runs on the physical address of its own trap stack, sets its EL2
//! registers ([`libboot`]'s `configure_el2`, as the boot core does) and goes through
//! [`el_switch`](crate::el_switch) with the boot core's tables and vectors and its own
//! `SP_EL1` and `TPIDR_EL1`, entering `EL1h` on its trap stack (C12(a)).
//!
//! Before its MMU is on the core clears no BSS, takes no lock and does no atomic
//! read-modify-write: those do not complete on Non-cacheable memory on the Pi's cores.
//! With the MMU on it sets its bit in [`ONLINE`] with a cacheable atomic, which the boot
//! core reads at EL1 ([`wait_online`]), and parks.

use {
    crate::{cores::Cores, cpu_start::LoadedCpuStart, el_switch},
    aarch64_cpu::registers::{CNTFRQ_EL0, CNTPCT_EL0, CurrentEL, MPIDR_EL1, Read},
    core::{
        arch::global_asm,
        cell::UnsafeCell,
        mem::offset_of,
        sync::atomic::{AtomicU64, Ordering},
    },
    libcpu::{
        endless_sleep,
        smp::{CoreAffinity, LOW_AFFINITY_MASK},
    },
    libcpu_start::{MAX_CPUS, StartStatus},
};

/// One secondary core's boot parameters. `el2_stack_top` is physical, for its EL2 code
/// with the MMU off; the others are used once its MMU is on.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SecondaryBoot {
    /// The core's affinity ([`libcpu::smp::CoreAffinity::bits`]); [`UNUSED`] for a slot
    /// no core claims.
    pub affinity: u64,
    /// Physical top of its trap stack, its stack at EL2.
    pub el2_stack_top: u64,
    /// `TTBR0_EL1`, the boot core's.
    pub ttbr0: u64,
    /// `TTBR1_EL1`, the boot core's.
    pub ttbr1: u64,
    /// `VBAR_EL1`, the boot core's.
    pub vbar: u64,
    /// Where it starts at EL1.
    pub entry: u64,
    /// Its `SP_EL1`: the kernel virtual top of its trap stack.
    pub trap_stack_top: u64,
    /// Its `TPIDR_EL1`: the kernel virtual address of its record.
    pub core_record: u64,
}

/// The affinity of a slot no core claims: it has bits outside
/// [`libcpu::smp::AFFINITY_MASK`], so no core's affinity matches it.
pub const UNUSED: u64 = u64::MAX;

const UNUSED_SLOT: SecondaryBoot = SecondaryBoot {
    affinity: UNUSED,
    el2_stack_top: 0,
    ttbr0: 0,
    ttbr1: 0,
    vbar: 0,
    entry: 0,
    trap_stack_top: 0,
    core_record: 0,
};

/// The parameter table `_secondary_entry` searches, by logical index.
#[repr(transparent)]
pub struct SecondaryBootTable(UnsafeCell<[SecondaryBoot; MAX_CPUS]>);

// SAFETY: the boot core writes the table before it releases any core, and the released
// cores only read it.
unsafe impl Sync for SecondaryBootTable {}

/// The secondary cores' parameters, in Kickstart's image (initialised data, so a core
/// that is never published finds no slot even though only the boot core clears BSS).
pub static SECONDARY_BOOT: SecondaryBootTable =
    SecondaryBootTable(UnsafeCell::new([UNUSED_SLOT; MAX_CPUS]));

/// Bit `index` is set by core `index` once it is at EL1 with its MMU and caches on.
/// Zeroed with Kickstart's BSS by the boot core.
static ONLINE: AtomicU64 = AtomicU64::new(0);

/// The cores whose release the start method accepted, by logical index. Stored at EL2,
/// read at EL1 by the boot core only.
static RELEASED: AtomicU64 = AtomicU64::new(0);

/// How long the boot core waits for the released cores, in milliseconds.
const ONLINE_TIMEOUT_MS: u64 = 1000;

/// What every secondary core shares with the boot core.
#[derive(Debug, Clone, Copy)]
pub struct SharedBoot {
    /// `TTBR0_EL1`.
    pub ttbr0: u64,
    /// `TTBR1_EL1`.
    pub ttbr1: u64,
    /// `VBAR_EL1`.
    pub vbar: u64,
}

/// Physical address of core `index`'s slot.
fn slot(index: usize) -> *mut SecondaryBoot {
    // SAFETY: `index` is below `MAX_CPUS`, inside the table.
    unsafe { SECONDARY_BOOT.0.get().cast::<SecondaryBoot>().add(index) }
}

/// Write every secondary core's parameters and clean the table to the Point of
/// Coherency, before any core is released. `trap_stack_top(index)` is the kernel virtual
/// top of core `index`'s trap stack.
fn publish(cores: &Cores, shared: SharedBoot, trap_stack_top: impl Fn(usize) -> u64) {
    for (index, &affinity) in cores.affinities().iter().enumerate() {
        if index == cores.boot_index() {
            continue;
        }
        let boot = SecondaryBoot {
            affinity,
            el2_stack_top: cores.stack_physical_top(index),
            ttbr0: shared.ttbr0,
            ttbr1: shared.ttbr1,
            vbar: shared.vbar,
            entry: secondary_el1 as *const () as u64,
            trap_stack_top: trap_stack_top(index),
            core_record: cores.record_virt(index),
        };
        // SAFETY: only the boot core runs, and it has released no core yet; the MMU is
        // off, so the table's link address is its physical address.
        unsafe {
            slot(index).write_volatile(boot);
        }
    }
    let start = SECONDARY_BOOT.0.get() as u64;
    // SAFETY: the table is Kickstart's own data, reached as is with the MMU off.
    unsafe {
        libcpu::cache::clean_data_range(start, start + size_of::<SecondaryBootTable>() as u64);
    }
}

/// Publish the secondary cores' parameters and release each of them through the start
/// method. Runs at EL2 on the boot core, after the tables, records and stacks are final.
pub fn release(
    cores: &Cores,
    start_method: &LoadedCpuStart,
    shared: SharedBoot,
    trap_stack_top: impl Fn(usize) -> u64,
) {
    publish(cores, shared, trap_stack_top);
    let mut released = 0_u64;
    for index in 0..cores.affinities().len() {
        if index == cores.boot_index() {
            continue;
        }
        let cpu = u32::try_from(index).expect("at most MAX_CPUS cores");
        let status = start_method.start(cpu, entry_address(), slot(index) as u64);
        if status == StartStatus::Success {
            released |= 1 << index;
        } else {
            crate::early_println!("🥾 Core {index} not released: {status:?}");
        }
    }
    RELEASED.store(released, Ordering::Relaxed);
    crate::early_println!("🥾 Released cores {released:#06b}");
}

/// At EL1 on the boot core: wait until every released core is online, at most
/// [`ONLINE_TIMEOUT_MS`], and report. Returns the online cores, by logical index.
pub fn wait_online() -> u64 {
    let released = RELEASED.load(Ordering::Relaxed);
    let ticks = CNTFRQ_EL0.get() * ONLINE_TIMEOUT_MS / 1000;
    let start = CNTPCT_EL0.get();
    let mut online = ONLINE.load(Ordering::Acquire);
    while online & released != released && CNTPCT_EL0.get() - start < ticks {
        core::hint::spin_loop();
        online = ONLINE.load(Ordering::Acquire);
    }
    if online & released == released {
        crate::early_println!("🥾 Cores online {online:#06b}");
    } else {
        crate::early_println!(
            "🥾 Cores online {online:#06b} of released {released:#06b} after {ONLINE_TIMEOUT_MS} ms"
        );
    }
    online
}

/// Physical address a secondary core is released to.
fn entry_address() -> u64 {
    unsafe extern "C" {
        fn _secondary_entry() -> !;
    }
    _secondary_entry as *const () as u64
}

// The released core: its affinity from MPIDR_EL1 (as `CoreAffinity::from_mpidr`), the
// matching slot of SECONDARY_BOOT, that slot's EL2 stack, then `secondary_el2(slot)`.
// A core with no slot parks.
global_asm!(
    ".section .text.secondary_entry, \"ax\"",
    ".global _secondary_entry",
    ".type _secondary_entry, function",
    "_secondary_entry:",
    "    mrs x9, MPIDR_EL1",
    "    and x10, x9, {LOW_MASK}",
    "    ubfx x9, x9, {AFF3_SHIFT}, {AFF3_BITS}",
    "    orr x9, x10, x9, lsl {AFF3_SHIFT}",
    "    adrp x10, {TABLE}",
    "    add x10, x10, :lo12:{TABLE}",
    "    mov x11, {SLOTS}",
    "1:  cbz x11, 3f",
    "    ldr x12, [x10, {AFFINITY}]",
    "    cmp x12, x9",
    "    b.eq 2f",
    "    add x10, x10, {SLOT_BYTES}",
    "    sub x11, x11, 1",
    "    b 1b",
    "2:  ldr x12, [x10, {STACK}]",
    "    mov sp, x12",
    "    mov x0, x10",
    "    bl {RUST}",
    "3:  wfe",
    "    b 3b",
    ".size _secondary_entry, . - _secondary_entry",
    LOW_MASK = const LOW_AFFINITY_MASK,
    AFF3_SHIFT = const MPIDR_EL1::Aff3.shift,
    AFF3_BITS = const MPIDR_EL1::Aff3.mask.count_ones(),
    TABLE = sym SECONDARY_BOOT,
    SLOTS = const MAX_CPUS,
    AFFINITY = const offset_of!(SecondaryBoot, affinity),
    SLOT_BYTES = const size_of::<SecondaryBoot>(),
    STACK = const offset_of!(SecondaryBoot, el2_stack_top),
    RUST = sym secondary_el2,
);

/// A secondary core at EL2 on its own stack: set its EL2 registers and drop to EL1 with
/// the MMU on.
extern "C" fn secondary_el2(boot: &SecondaryBoot) -> ! {
    libboot::arch::aarch64::boot::configure_el2();
    // The firmware and QEMU release the cores at EL2; elsewhere EL1 cannot be set up.
    if CurrentEL.get() != CurrentEL::EL::EL2.value {
        endless_sleep()
    }
    // SAFETY: at EL2 on SP_EL2 on the core `boot` names (its affinity matched); the
    // tables, vectors and stacks are the boot core's live ones and this core's own,
    // published before its release; this core's EL2 call chain is abandoned here.
    unsafe {
        el_switch::enable_mmu_and_drop_to_el1(
            boot.ttbr0,
            boot.ttbr1,
            boot.vbar,
            boot.entry,
            None,
            boot.trap_stack_top,
            boot.core_record,
        )
    }
}

/// A secondary core's first code at EL1 (`EL1h`, on its trap stack, interrupts masked):
/// check its record, report online, park. It prints nothing: the console is the boot
/// core's.
extern "C" fn secondary_el1() -> ! {
    let record = nucleus::core_record::current();
    if record.affinity == CoreAffinity::current().bits() {
        ONLINE.fetch_or(1 << record.index, Ordering::Release);
    }
    endless_sleep()
}
