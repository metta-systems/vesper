//! Trusted `EL1t` source/Bounce translation provisioning and observations.
//!
//! This is a two-Thread functional fixture, not PPC or an EL0 confinement proof.
//! All low leaves are explicit capability mappings. TTBR1 remains the invariant
//! kernel/direct map, including Bounce's accounted high `SP_EL0` stack.

use {
    super::{BOOT_TABLE_GUARD, boot_key},
    aarch64_cpu::registers::{Readable, TCR_EL1, TTBR0_EL1, TTBR1_EL1, VBAR_EL1},
    core::{
        arch::asm,
        sync::atomic::{AtomicU64, Ordering},
    },
    kickstart::bootstrap::RetainedInitMemory,
    libaddress::PhysAddr,
    libexception::arch::aarch64::SavedContext,
    libobject::{
        CapError, EventCountOp, FrameKey, InvalidKeyReason, KeySlot, KeyTableKey, NotificationOp,
        ObjectType, PageTableKey, RawKey, Rights, UntypedKey, decode_syscall_result,
    },
    libqemu::semihosting as semi,
    nucleus::{api::key_entry::KeyEntry, objects::KeyTable},
};

const PAGE: u64 = 4096;
const LEAF_SPAN: u64 = 2 * 1024 * 1024;
const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;
const IMAGE_TABLE_GUARD: u32 = 0x135;
const IMAGE_TABLE_SLOT: u32 = 68;
const SOURCE_SCRATCH: u32 = 70;
const BOUNCE_SCRATCH: u32 = 71;
const BOUNCE_ROOT: u32 = 72;
const SOURCE_PROBE_TABLE: u32 = 76;
const BOUNCE_PROBE_TABLE: u32 = 77;
const SOURCE_PROBE_FRAME: u32 = 78;
const BOUNCE_PROBE_FRAME: u32 = 79;
const SOURCE_IMAGE_TABLES: u32 = 80;
const BOUNCE_IMAGE_TABLES: u32 = 88;
const PROBE_VA: u64 = 0x1800_0000;
const SOURCE_MARKER: u64 = 0x5352_4300_0000_0000;
const BOUNCE_MARKER: u64 = 0x424E_4300_0000_0000;

static SOURCE_TTBR: AtomicU64 = AtomicU64::new(0);
static BOUNCE_TTBR: AtomicU64 = AtomicU64::new(0);
static KERNEL_TTBR: AtomicU64 = AtomicU64::new(0);
static SOURCE_BACKING: AtomicU64 = AtomicU64::new(0);
static BOUNCE_BACKING: AtomicU64 = AtomicU64::new(0);
static SOURCE_KEY: AtomicU64 = AtomicU64::new(0);
static BOUNCE_KEY: AtomicU64 = AtomicU64::new(0);
static SOURCE_ROUNDS: AtomicU64 = AtomicU64::new(0);
static BOUNCE_ROUNDS: AtomicU64 = AtomicU64::new(0);

/// Exact metadata capacity: source prefix + later test L3 (4), Bounce prefix
/// (3), image L3s in both roots (2*N), probe L3s (2), retirement roots (2)
/// with a disposable L1/L2 chain (2), and the existing twelve-entry pool
/// refill. Unmap does not free metadata.
pub fn page_table_capacity(retained: &RetainedInitMemory) -> usize {
    25 + 2 * image_table_count(retained)
}

fn image_table_count(retained: &RetainedInitMemory) -> usize {
    let (start, end) = retained.image();
    let (stack_start, stack_end) = retained.stack();
    assert_eq!(start, 0x80000, "fixture expects the linked init base");
    assert_eq!(stack_start, PAGE);
    assert_eq!(stack_end, start);
    let count = usize::try_from(end.div_ceil(LEAF_SPAN)).unwrap();
    assert!(
        count <= 8,
        "image exceeds the fixture's eight L3 slots per root"
    );
    count
}

/// Walk retained bootstrap/runtime tables without holding references over SVC.
pub fn read_leaf(ttbr: u64, vaddr: u64) -> (u32, u64) {
    let mut table_paddr = ttbr & ADDR_MASK;
    for level in 0..4 {
        let slot = ((vaddr >> (39 - 9 * level)) & 0x1ff) as usize;
        // SAFETY: callers supply retained roots; every subsequent page is a
        // valid table descriptor in accounted private backing. Slots are 9-bit.
        let entry = unsafe {
            PhysAddr::new(table_paddr)
                .user_to_kernel()
                .as_ptr::<u64>()
                .add(slot)
                .read_volatile()
        };
        assert!(entry & 1 != 0, "missing L{level} descriptor for {vaddr:#x}");
        if level == 3 {
            assert_eq!(entry & 3, 3);
            return (level, entry);
        }
        if entry & 2 == 0 {
            assert_ne!(level, 0);
            return (level, entry);
        }
        table_paddr = entry & ADDR_MASK;
    }
    unreachable!("four-level walk must reach a leaf")
}

/// Preserve bootstrap nG/global coverage before replacing its ASID-0 root.
pub fn observe_bootstrap() {
    let ttbr0 = TTBR0_EL1.get();
    let ttbr1 = TTBR1_EL1.get();
    assert_eq!(ttbr0 >> 48, 0, "bootstrap is the reserved ASID-0 context");
    let pc = super::kicktest_run as *const u8 as u64;
    let (level, leaf) = read_leaf(ttbr0, pc);
    assert_eq!(level, 2);
    assert_eq!(leaf & 3, 1);
    assert_eq!(leaf & 0x0000_FFFF_FFE0_0000, pc & 0x0000_FFFF_FFE0_0000);
    assert_ne!(leaf & (1 << 11), 0, "bootstrap low leaves must be nG");
    let vector = VBAR_EL1.get();
    assert_eq!(vector >> 48, 0xffff);
    assert_eq!(read_leaf(ttbr1, vector).1 & (1 << 11), 0);
    let root_direct = PhysAddr::new(ttbr0 & ADDR_MASK).user_to_kernel().as_u64();
    let (level, leaf) = read_leaf(ttbr1, root_direct);
    assert_eq!(level, 2);
    assert_eq!(leaf & 0x0000_FFFF_FFE0_0000, ttbr0 & 0x0000_FFFF_FFE0_0000);
    assert_eq!(leaf & (1 << 11), 0, "invariant high leaves remain global");
    assert_eq!(TCR_EL1.get() & (1 << 22), 0, "TTBR0 supplies the ASID");
    KERNEL_TTBR.store(ttbr1, Ordering::Release);
}

/// Explicit, bounded boot provisioning through existing wrappers. The direct
/// table address is used only for bootstrap-origin grants, never across SVC.
pub struct Provisioner<'a> {
    pub untyped: &'a UntypedKey,
    pub self_table: &'a KeyTableKey,
    pub boot_table_addr: u64,
    pub retained: &'a RetainedInitMemory,
    pub source_as: RawKey,
    pub bounce_as: RawKey,
}

impl Provisioner<'_> {
    fn carve_tables(&self, first: u32, count: u32) -> RawKey {
        self.untyped
            .retype(
                ObjectType::PAGE_TABLE,
                12,
                0,
                count,
                self.self_table,
                first,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("fixture PageTable Retype failed: {:?}", error.code()))
    }

    fn map_table(key: RawKey, parent: RawKey, vaddr: u64) {
        PageTableKey::from_key(key)
            .map(parent, vaddr)
            .unwrap_or_else(|error| panic!("fixture PageTable.Map failed: {:?}", error.code()));
    }

    /// Populate both complete image closures and the retained source stack.
    /// The mapped source origin and separately derived Bounce cap are moved
    /// into a charged archive table, preserving every mapping record. Scratch
    /// slots are reused with their returned incarnations, never guessed keys.
    /// Move is not Frame deprovisioning: no backing, capability, or mapping is
    /// discarded, revoked, reset, or reclaimed by this scratch-slot transport.
    pub fn provision(&self, source_prefix: [RawKey; 3], bounce_table: RawKey) -> RawKey {
        let count = image_table_count(self.retained);
        let bounce_prefix = self.carve_tables(BOUNCE_ROOT, 3);
        Self::map_table(bounce_prefix, self.bounce_as, 0);
        let bounce_l1 = boot_key(BOUNCE_ROOT + 1, bounce_prefix.incarnation());
        let bounce_l2 = boot_key(BOUNCE_ROOT + 2, bounce_prefix.incarnation());
        Self::map_table(bounce_l1, bounce_prefix, 0);
        Self::map_table(bounce_l2, bounce_l1, 0);
        Self::map_table(source_prefix[1], source_prefix[0], 0);
        Self::map_table(source_prefix[2], source_prefix[1], 0);
        for (first, parent) in [
            (SOURCE_IMAGE_TABLES, source_prefix[2]),
            (BOUNCE_IMAGE_TABLES, bounce_l2),
        ] {
            let tables = self.carve_tables(first, u32::try_from(count).unwrap());
            for index in 0..count {
                Self::map_table(
                    boot_key(first + u32::try_from(index).unwrap(), tables.incarnation()),
                    parent,
                    u64::try_from(index).unwrap() * LEAF_SPAN,
                );
            }
        }

        let (image_start, image_end) = self.retained.image();
        let (stack_start, stack_end) = self.retained.stack();
        let image_pages = (image_end - image_start) / PAGE;
        let stack_pages = (stack_end - stack_start) / PAGE;
        let entries = 1 + 2 * image_pages + stack_pages; // slot zero is reserved
        let bits = u8::try_from(entries.next_power_of_two().trailing_zeros()).unwrap();
        let archive_key = self
            .untyped
            .retype(
                ObjectType::KEY_TABLE,
                bits,
                IMAGE_TABLE_GUARD,
                1,
                self.self_table,
                IMAGE_TABLE_SLOT,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("image archive Retype failed: {:?}", error.code()));
        let archive = KeyTableKey::from_key(archive_key);
        let (archive_addr, source_id, bounce_id) = {
            // SAFETY: retained initialized boot table, borrowed only to copy
            // bootstrap-issued authority metadata before the mapping calls.
            let table = unsafe { &*(self.boot_table_addr as *const KeyTable) };
            let entry = table
                .lookup(archive_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|error| {
                    panic!("archive authority lookup failed: {:?}", error.code())
                });
            assert_eq!(
                entry.keytable_guard_and_size().ok(),
                Some((IMAGE_TABLE_GUARD, bits))
            );
            let archive_addr = entry
                .keytable_address()
                .unwrap_or_else(|error| panic!("archive is not a KeyTable: {:?}", error.code()));
            let source_id = table
                .lookup(self.source_as, BOOT_TABLE_GUARD)
                .and_then(KeyEntry::object_id)
                .unwrap_or_else(|error| {
                    panic!("source identity lookup failed: {:?}", error.code())
                });
            let bounce_id = table
                .lookup(self.bounce_as, BOOT_TABLE_GUARD)
                .and_then(KeyEntry::object_id)
                .unwrap_or_else(|error| {
                    panic!("Bounce identity lookup failed: {:?}", error.code())
                });
            (archive_addr, source_id, bounce_id)
        };
        let mut next_slot = 1;
        for (start, end, shared) in [
            (image_start, image_end, true),
            (stack_start, stack_end, false),
        ] {
            for paddr in (start..end).step_by(usize::try_from(PAGE).unwrap()) {
                let source = {
                    // SAFETY: this is the initialized private boot carve. No
                    // table reference survives the following capability calls.
                    let table = unsafe { &mut *(self.boot_table_addr as *mut KeyTable) };
                    self.retained.grant_page(
                        table,
                        KeySlot(SOURCE_SCRATCH),
                        BOOT_TABLE_GUARD,
                        paddr,
                    )
                };
                let bounce = shared.then(|| {
                    self.self_table
                        .copy_derive(source, self.self_table, BOUNCE_SCRATCH, Rights::all())
                        .unwrap_or_else(|error| {
                            panic!("image CopyDerive failed: {:?}", error.code())
                        })
                });
                if let Some(key) = bounce {
                    // Copy did not install a descriptor or inherit a mapping.
                    assert!(matches!(
                        FrameKey::from_key(key).unmap(),
                        Err(CapError::NotMapped)
                    ));
                }
                let rights =
                    Rights(Rights::READ | Rights::WRITE | if shared { Rights::EXECUTE } else { 0 });
                for (key, target) in [(Some(source), self.source_as), (bounce, self.bounce_as)] {
                    if let Some(key) = key {
                        FrameKey::from_key(key)
                            .map(target, paddr, rights, 0)
                            .unwrap_or_else(|error| {
                                panic!(
                                    "retained Frame.Map at {paddr:#x} failed: {:?}",
                                    error.code()
                                )
                            });
                        let archived = self
                            .self_table
                            .transfer(key, &archive, next_slot)
                            .unwrap_or_else(|error| {
                                panic!("mapped image Move failed: {:?}", error.code())
                            });
                        // SAFETY: archive_addr came from the live capability to
                        // the full private Retype carve. This borrow ends before
                        // the next SVC; Move must preserve the mapping record.
                        let table = unsafe { &*(archive_addr as *const KeyTable) };
                        let frame = table
                            .lookup(archived, IMAGE_TABLE_GUARD)
                            .and_then(KeyEntry::as_frame)
                            .unwrap_or_else(|error| {
                                panic!("archived mapping missing: {:?}", error.code())
                            });
                        let mapping = frame.mapping().expect("Move lost the mapping record");
                        assert_eq!(frame.paddr, paddr);
                        assert_eq!(mapping.vaddr, paddr);
                        assert_eq!(
                            mapping.address_space,
                            if target == self.source_as {
                                source_id
                            } else {
                                bounce_id
                            }
                        );
                        next_slot += 1;
                    }
                }
            }
        }
        assert_eq!(u64::from(next_slot), entries);
        {
            // SAFETY: full initialized retained private archive carve; no SVC
            // or mutable table access occurs while this borrow is live.
            let table = unsafe { &*(archive_addr as *const KeyTable) };
            assert_eq!(table.capacity(), 1_usize << bits);
            assert_eq!(u64::try_from(table.len()).unwrap(), entries - 1);
        }

        Self::map_table(
            self.carve_tables(SOURCE_PROBE_TABLE, 1),
            source_prefix[2],
            PROBE_VA,
        );
        Self::map_table(
            self.carve_tables(BOUNCE_PROBE_TABLE, 1),
            bounce_l2,
            PROBE_VA,
        );
        let source_probe = self
            .untyped
            .retype(
                ObjectType::FRAME,
                12,
                0,
                2,
                self.self_table,
                SOURCE_PROBE_FRAME,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("probe Frame Retype failed: {:?}", error.code()));
        let bounce_probe = boot_key(BOUNCE_PROBE_FRAME, source_probe.incarnation());
        let mut physical = [0; 2];
        for (index, (key, target, marker)) in [
            (source_probe, self.source_as, SOURCE_MARKER),
            (bounce_probe, self.bounce_as, BOUNCE_MARKER),
        ]
        .into_iter()
        .enumerate()
        {
            let (paddr, size) = FrameKey::from_key(key)
                .get_extent()
                .unwrap_or_else(|error| panic!("probe GetExtent failed: {:?}", error.code()));
            assert_eq!(size, PAGE);
            physical[index] = paddr;
            // SAFETY: this freshly sanitized Frame is accounted and retained;
            // the trusted fixture accesses it via the invariant direct map.
            unsafe {
                PhysAddr::new(paddr)
                    .user_to_kernel()
                    .as_mut_ptr::<u64>()
                    .write_volatile(marker);
            }
            FrameKey::from_key(key)
                .map(target, PROBE_VA, Rights(Rights::READ | Rights::WRITE), 0)
                .unwrap_or_else(|error| panic!("probe Frame.Map failed: {:?}", error.code()));
        }
        assert_ne!(physical[0], physical[1]);
        let local_bounce_probe = self
            .self_table
            .copy_derive(
                bounce_probe,
                &KeyTableKey::from_key(bounce_table),
                8,
                Rights(Rights::READ | Rights::GRANT),
            )
            .unwrap_or_else(|error| panic!("Bounce probe grant failed: {:?}", error.code()));
        SOURCE_BACKING.store(physical[0], Ordering::Release);
        BOUNCE_BACKING.store(physical[1], Ordering::Release);
        SOURCE_KEY.store(source_probe.to_wire(), Ordering::Release);
        BOUNCE_KEY.store(local_bounce_probe.to_wire(), Ordering::Release);
        semi::println!(
            "Translation backing: image=[{image_start:#x},{image_end:#x}) {image_pages} pages x2; source stack=[{stack_start:#x},{stack_end:#x}) {stack_pages} pages; archive=2^{bits} entries, occupied={}, carve={} bytes; installed_tables={}, metadata_entries={}, capacity={}",
            entries - 1,
            KeyTable::carve_size(bits),
            8 + 2 * count,
            9 + 2 * count,
            page_table_capacity(self.retained)
        );
        bounce_prefix
    }
}

/// Verify the full page-granular dependency closure before either root runs.
pub fn verify_retained(retained: &RetainedInitMemory, source_root: u64, bounce_root: u64) {
    let (image_start, image_end) = retained.image();
    let (stack_start, stack_end) = retained.stack();
    for root in [source_root, bounce_root] {
        for paddr in (image_start..image_end).step_by(usize::try_from(PAGE).unwrap()) {
            let (level, leaf) = read_leaf(root, paddr);
            assert_eq!(level, 3);
            assert_eq!(leaf & ADDR_MASK, paddr);
            assert_ne!(leaf & (1 << 11), 0);
            assert_eq!(
                leaf & ((1 << 53) | (1 << 54) | (3 << 6)),
                0,
                "trusted RW+X image must execute at EL1, not grant EL0 RW+X"
            );
        }
    }
    for paddr in (stack_start..stack_end).step_by(usize::try_from(PAGE).unwrap()) {
        let (level, leaf) = read_leaf(source_root, paddr);
        assert_eq!(level, 3);
        assert_eq!(leaf & ADDR_MASK, paddr);
        assert_ne!(leaf & (1 << 11), 0);
        assert_eq!(leaf & (3 << 6), 1 << 6);
        assert_eq!(leaf & ((1 << 53) | (1 << 54)), (1 << 53) | (1 << 54));
    }
}

/// Publish exact bound contexts before the first runnable handoff.
pub fn bind_contexts(source_root: u64, source_asid: u16, bounce_root: u64, bounce_asid: u16) {
    assert_eq!(source_asid, 1);
    assert_eq!(bounce_asid, 2);
    assert_ne!(source_root, bounce_root);
    SOURCE_TTBR.store(
        source_root | (u64::from(source_asid) << 48),
        Ordering::Release,
    );
    BOUNCE_TTBR.store(
        bounce_root | (u64::from(bounce_asid) << 48),
        Ordering::Release,
    );
}

pub fn observe_source() {
    observe(false);
}
pub fn observe_bounce() {
    observe(true);
}

fn observe(bounce: bool) {
    let (
        expected,
        backing,
        key,
        rounds,
        marker,
        foreign_key,
        foreign_backing,
        foreign_rounds,
        foreign_marker,
    ) = if bounce {
        (
            &BOUNCE_TTBR,
            &BOUNCE_BACKING,
            &BOUNCE_KEY,
            &BOUNCE_ROUNDS,
            BOUNCE_MARKER,
            &SOURCE_KEY,
            &SOURCE_BACKING,
            &SOURCE_ROUNDS,
            SOURCE_MARKER,
        )
    } else {
        (
            &SOURCE_TTBR,
            &SOURCE_BACKING,
            &SOURCE_KEY,
            &SOURCE_ROUNDS,
            SOURCE_MARKER,
            &BOUNCE_KEY,
            &BOUNCE_BACKING,
            &BOUNCE_ROUNDS,
            BOUNCE_MARKER,
        )
    };
    assert_eq!(
        TTBR0_EL1.get(),
        expected.load(Ordering::Acquire),
        "selected Thread root/ASID mismatch"
    );
    assert_eq!(
        TTBR1_EL1.get(),
        KERNEL_TTBR.load(Ordering::Acquire),
        "kernel map changed"
    );
    let local_key = RawKey::from_wire(key.load(Ordering::Acquire));
    let paddr = backing.load(Ordering::Acquire);
    assert_eq!(
        FrameKey::from_key(local_key).get_extent().ok(),
        Some((paddr, PAGE))
    );
    let foreign = RawKey::from_wire(foreign_key.load(Ordering::Acquire));
    assert!(
        matches!(FrameKey::from_key(foreign).get_extent(), Err(CapError::InvalidKey {
        key, reason: InvalidKeyReason::GuardMismatch, operand: 0,
    }) if key == foreign)
    );
    let round = rounds.load(Ordering::Acquire);
    // SAFETY: the verified current root maps PROBE_VA to this retained writable
    // Frame. Volatile accesses warm the same low VA in both distinct ASIDs;
    // no test-side switch, reactivation, or TLBI hides scheduler mistakes.
    unsafe {
        let low = PROBE_VA as *mut u64;
        assert_eq!(low.read_volatile(), marker | round);
        low.write_volatile(marker | (round + 1));
        assert_eq!(low.read_volatile(), marker | (round + 1));
        assert_eq!(
            PhysAddr::new(paddr)
                .user_to_kernel()
                .as_ptr::<u64>()
                .read_volatile(),
            marker | (round + 1)
        );
        assert_eq!(
            PhysAddr::new(foreign_backing.load(Ordering::Acquire))
                .user_to_kernel()
                .as_ptr::<u64>()
                .read_volatile(),
            foreign_marker | foreign_rounds.load(Ordering::Acquire)
        );
    }
    rounds.store(round + 1, Ordering::Release);
    let (level, leaf) = read_leaf(expected.load(Ordering::Acquire), PROBE_VA);
    assert_eq!(level, 3);
    assert_eq!(leaf & ADDR_MASK, paddr);
    assert_ne!(leaf & (1 << 11), 0);
    semi::println!(
        "Translation observation: Bounce={bounce} round={} TTBR0={:#x} backing={paddr:#x}",
        round + 1,
        TTBR0_EL1.get()
    );
}

/// Observe the same live sentinels in a parked Thread's owned continuation.
/// This complements the assembly checks performed after actual resumption.
pub fn assert_parked_registers(saved: &SavedContext, key: RawKey) {
    assert_eq!(saved.gpr[19], key.to_wire());
    for (index, expected) in [
        0x2020, 0x2121, 0x2222, 0x2323, 0x2424, 0x2525, 0x2626, 0x2727, 0x2828,
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(saved.gpr[20 + index], expected);
    }
    assert_eq!(saved.gpr[29], saved.sp);
    assert_eq!(saved.lr, 0x3030);
}

pub fn assert_rounds() {
    assert_eq!(SOURCE_ROUNDS.load(Ordering::Acquire), 4);
    assert_eq!(BOUNCE_ROUNDS.load(Ordering::Acquire), 3);
}

/// Test-local ordinary wait transport, not an Invocation/PPC ABI. Spill the
/// compiler's x19-x30, put the caller-local wait key in x19, live sentinels in
/// x20-x28/x30 and the exact SVC execution SP in x29. Verify them against a
/// private execution-stack copy before restoring the compiler's
/// registers. No source continuation lives on `SP_EL1`; this spill is userspace.
#[inline(never)]
fn wait_with_registers(key: RawKey, op: u64, arg0: u64, arg1: u64) -> Result<u64, CapError> {
    let status: u64;
    let word1: u64;
    let word2: u64;
    let intact: u64;
    // SAFETY: the entire aligned 112-byte execution-stack spill is created and
    // removed in this block; original callee-saved registers/LR are restored
    // before Rust resumes. SVC uses the existing two-word wait ABI. x8/x9 and
    // flags are declared clobbered, and all unused argument words are zero.
    unsafe {
        asm!(
            "sub sp, sp, #112",
            "stp x19, x20, [sp, #0]", "stp x21, x22, [sp, #16]",
            "stp x23, x24, [sp, #32]", "stp x25, x26, [sp, #48]",
            "stp x27, x28, [sp, #64]", "stp x29, x30, [sp, #80]",
            "str x0, [sp, #96]", "mov x19, x0", "mov x20, #0x2020", "mov x21, #0x2121",
            "mov x22, #0x2222", "mov x23, #0x2323", "mov x24, #0x2424",
            "mov x25, #0x2525", "mov x26, #0x2626", "mov x27, #0x2727",
            "mov x28, #0x2828", "mov x29, sp", "mov x30, #0x3030",
            "svc #0",
            "mov x8, #0", "ldr x9, [sp, #96]", "cmp x19, x9", "b.ne 2f",
            "mov x9, #0x2020", "cmp x20, x9", "b.ne 2f",
            "mov x9, #0x2121", "cmp x21, x9", "b.ne 2f",
            "mov x9, #0x2222", "cmp x22, x9", "b.ne 2f",
            "mov x9, #0x2323", "cmp x23, x9", "b.ne 2f",
            "mov x9, #0x2424", "cmp x24, x9", "b.ne 2f",
            "mov x9, #0x2525", "cmp x25, x9", "b.ne 2f",
            "mov x9, #0x2626", "cmp x26, x9", "b.ne 2f",
            "mov x9, #0x2727", "cmp x27, x9", "b.ne 2f",
            "mov x9, #0x2828", "cmp x28, x9", "b.ne 2f",
            "mov x9, sp", "cmp x29, x9", "b.ne 2f",
            "mov x9, #0x3030", "cmp x30, x9", "b.ne 2f", "mov x8, #1",
            "2:",
            "ldp x19, x20, [sp, #0]", "ldp x21, x22, [sp, #16]",
            "ldp x23, x24, [sp, #32]", "ldp x25, x26, [sp, #48]",
            "ldp x27, x28, [sp, #64]", "ldp x29, x30, [sp, #80]",
            "add sp, sp, #112",
            inlateout("x0") key.to_wire() => status,
            inlateout("x1") op => word1,
            inlateout("x2") arg0 => word2,
            in("x3") arg1, in("x4") 0_u64, in("x5") 0_u64,
            in("x6") 0_u64, in("x7") 0_u64,
            lateout("x8") intact, lateout("x9") _,
        );
    }
    assert_eq!(
        intact, 1,
        "wait resumption corrupted execution SP or x19-x30"
    );
    decode_syscall_result((status, word1, word2)).map(|(value, _)| value)
}

pub fn notification_wait(key: RawKey) -> Result<u64, CapError> {
    wait_with_registers(key, NotificationOp::Wait as u64, u64::MAX, 0)
}

pub fn event_count_await(key: RawKey, target: u64) -> Result<u64, CapError> {
    wait_with_registers(key, EventCountOp::Await as u64, target, u64::MAX)
}
