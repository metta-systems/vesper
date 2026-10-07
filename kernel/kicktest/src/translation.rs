//! Trusted `EL1t` source/Bounce translation provisioning and observations.
//!
//! This is a two-Thread functional fixture, not PPC or an EL0 confinement proof.
//! All low leaves are explicit capability mappings. TTBR1 remains the invariant
//! kernel/direct map, including Bounce's accounted high `SP_EL0` stack.
//! The generic provisioning steps live in `libkicktest`.

pub use libkicktest::paging::read_leaf;
use {
    super::kicktest_run,
    aarch64_cpu::registers::{Readable, TCR_EL1, TTBR0_EL1, TTBR1_EL1, VBAR_EL1},
    core::{
        arch::asm,
        sync::atomic::{AtomicU64, Ordering},
    },
    kickstart::bootstrap::RetainedInitMemory,
    libaddress::PhysAddr,
    libexception::arch::aarch64::SavedContext,
    libkicktest::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        keys::boot_key,
        paging::{ADDR_MASK, PAGE, image_table_count},
    },
    libobject::{
        CapError, EventCountOp, FrameKey, InvalidKeyReason, KeyTableKey, NotificationOp, RawKey,
        Rights, decode_syscall_result,
    },
    libqemu::semihosting as semi,
    nucleus::objects::KeyTable,
};

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
pub const PROBE_VA: u64 = 0x1800_0000;
/// Bounce-only PPC target stack: `PPC_STACK_PAGES` accounted Frames mapped
/// directly above the probe page, inside the same Bounce L3 table.
pub const PPC_STACK_VA: u64 = PROBE_VA + PAGE;
pub const PPC_STACK_PAGES: u64 = 2;
const BOUNCE_PPC_STACK_FRAMES: u32 = 210;
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

/// Preserve bootstrap nG/global coverage before replacing its ASID-0 root.
pub fn observe_bootstrap() {
    let ttbr0 = TTBR0_EL1.get();
    let ttbr1 = TTBR1_EL1.get();
    assert_eq!(ttbr0 >> 48, 0, "bootstrap is the reserved ASID-0 context");
    let pc = kicktest_run as *const u8 as u64;
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

/// Source/Bounce provisioning on top of the shared [`Builder`]: the image
/// closure in both roots, the probe pages and Bounce's PPC stack.
pub struct Provisioner<'a> {
    pub builder: &'a Builder<'a>,
    pub source_as: RawKey,
    pub bounce_as: RawKey,
}

impl Provisioner<'_> {
    /// Populate both complete image closures and the retained source stack,
    /// then the per-root probe pages and Bounce's PPC stack.
    pub fn provision(&self, source_prefix: [RawKey; 3], bounce_table: RawKey) -> RawKey {
        let builder = self.builder;
        let count = image_table_count(builder.retained);
        let bounce_prefix = builder.carve_tables(BOUNCE_ROOT, 3);
        Builder::map_table(bounce_prefix, self.bounce_as, 0);
        let bounce_l1 = boot_key(BOUNCE_ROOT + 1, bounce_prefix.incarnation());
        let bounce_l2 = boot_key(BOUNCE_ROOT + 2, bounce_prefix.incarnation());
        Builder::map_table(bounce_l1, bounce_prefix, 0);
        Builder::map_table(bounce_l2, bounce_l1, 0);
        Builder::map_table(source_prefix[1], source_prefix[0], 0);
        Builder::map_table(source_prefix[2], source_prefix[1], 0);
        let image = builder.map_retained_image(
            &[
                ImageTarget {
                    address_space: self.source_as,
                    l2: source_prefix[2],
                    first_table_slot: SOURCE_IMAGE_TABLES,
                },
                ImageTarget {
                    address_space: self.bounce_as,
                    l2: bounce_l2,
                    first_table_slot: BOUNCE_IMAGE_TABLES,
                },
            ],
            &ImageArchive {
                slot: IMAGE_TABLE_SLOT,
                guard: IMAGE_TABLE_GUARD,
                grant_scratch: SOURCE_SCRATCH,
                copy_scratch: BOUNCE_SCRATCH,
            },
        );

        Builder::map_table(
            builder.carve_tables(SOURCE_PROBE_TABLE, 1),
            source_prefix[2],
            PROBE_VA,
        );
        Builder::map_table(
            builder.carve_tables(BOUNCE_PROBE_TABLE, 1),
            bounce_l2,
            PROBE_VA,
        );
        let source_probe = builder.retype_frames(SOURCE_PROBE_FRAME, 2);
        let bounce_probe = boot_key(BOUNCE_PROBE_FRAME, source_probe.incarnation());
        let mut physical = [0; 2];
        for (index, (key, target, marker)) in [
            (source_probe, self.source_as, SOURCE_MARKER),
            (bounce_probe, self.bounce_as, BOUNCE_MARKER),
        ]
        .into_iter()
        .enumerate()
        {
            let paddr = Builder::frame_paddr(key);
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

        // Bounce's PPC target stack: ordinary RW Frames in Bounce's root only.
        // Whether target code fits in its published headroom is the
        // component's concern (guard pages), not checked here.
        let stack_count = u32::try_from(PPC_STACK_PAGES).unwrap();
        let first_stack_frame = builder.retype_frames(BOUNCE_PPC_STACK_FRAMES, stack_count);
        Builder::map_frames(
            BOUNCE_PPC_STACK_FRAMES,
            first_stack_frame.incarnation(),
            stack_count,
            self.bounce_as,
            PPC_STACK_VA,
            Rights(Rights::READ | Rights::WRITE),
        );

        let local_bounce_probe = builder
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
        let (image_start, image_end) = builder.retained.image();
        let (stack_start, stack_end) = builder.retained.stack();
        semi::println!(
            "Translation backing: image=[{image_start:#x},{image_end:#x}) {} pages x2; source stack=[{stack_start:#x},{stack_end:#x}) {} pages; archive=2^{} entries, occupied={}, carve={} bytes; installed_tables={}, metadata_entries={}, capacity={}",
            image.image_pages,
            image.stack_pages,
            image.archive_bits,
            image.archived,
            KeyTable::carve_size(image.archive_bits),
            8 + 2 * count,
            9 + 2 * count,
            page_table_capacity(builder.retained)
        );
        bounce_prefix
    }
}

/// Verify the full page-granular dependency closure before either root runs.
pub fn verify_retained(retained: &RetainedInitMemory, source_root: u64, bounce_root: u64) {
    verify_retained_image(retained, &[source_root, bounce_root]);
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

/// The bound source TTBR0 value (root | ASID 1).
pub fn source_ttbr() -> u64 {
    SOURCE_TTBR.load(Ordering::Acquire)
}

/// The bound Bounce TTBR0 value (root | ASID 2).
pub fn bounce_ttbr() -> u64 {
    BOUNCE_TTBR.load(Ordering::Acquire)
}

/// The Bounce probe page's first word, read through the invariant direct map
/// without touching `PROBE_VA` or the observation round counters.
pub fn bounce_probe_word() -> u64 {
    // SAFETY: the retained, accounted Bounce probe Frame stays mapped in the
    // high direct map for the fixture's lifetime.
    unsafe {
        PhysAddr::new(BOUNCE_BACKING.load(Ordering::Acquire))
            .user_to_kernel()
            .as_ptr::<u64>()
            .read_volatile()
    }
}
