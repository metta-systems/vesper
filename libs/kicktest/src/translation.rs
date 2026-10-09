//! Trusted `EL1t` source/Bounce translation provisioning and observations.
//!
//! This is a two-Thread functional fixture, not PPC or an EL0 confinement proof.
//! All low leaves are explicit capability mappings. TTBR1 remains the invariant
//! kernel/direct map, including Bounce's accounted high `SP_EL0` stack.
//! The generic provisioning steps live in [`crate::builder`]; [`crate::bounce`]
//! builds the Bounce fixture on top of this module.

pub use crate::paging::read_leaf;
use {
    crate::{
        builder::{Builder, ImageArchive, ImageTarget, verify_retained_image},
        keys::boot_key,
        paging::{ADDR_MASK, PAGE, image_table_count},
        registers,
    },
    aarch64_cpu::registers::{Readable, TCR_EL1, TTBR0_EL1, TTBR1_EL1, VBAR_EL1},
    core::sync::atomic::{AtomicU64, Ordering},
    kickstart::bootstrap::RetainedInitMemory,
    libaddress::PhysAddr,
    libexception::arch::aarch64::SavedContext,
    libobject::{
        CapError, EventCountOp, FrameKey, InvalidKeyReason, KeyTableKey, NotificationOp, RawKey,
        Rights, decode_syscall_result,
    },
    libqemu::semihosting as semi,
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
/// `code` is an address in the running low image (the test kernel's entry).
pub fn observe_bootstrap(code: u64) {
    let ttbr0 = TTBR0_EL1.get();
    let ttbr1 = TTBR1_EL1.get();
    assert_eq!(ttbr0 >> 48, 0, "bootstrap is the reserved ASID-0 context");
    let pc = code;
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
        // One archived capability per mapped page per root: the image in both
        // roots, the source stack in the source root only.
        assert_eq!(image.archived, 2 * image.image_pages + image.stack_pages);
        #[cfg(feature = "qemu")]
        {
            let count = image_table_count(builder.retained);
            let (image_start, image_end) = builder.retained.image();
            let (stack_start, stack_end) = builder.retained.stack();
            semi::println!(
                "Translation backing: image=[{image_start:#x},{image_end:#x}) {} pages x2; source stack=[{stack_start:#x},{stack_end:#x}) {} pages; archive=2^{} entries, occupied={}, carve={} bytes; installed_tables={}, metadata_entries={}, capacity={}",
                image.image_pages,
                image.stack_pages,
                image.archive_bits,
                image.archived,
                nucleus::objects::KeyTable::carve_size(image.archive_bits),
                8 + 2 * count,
                9 + 2 * count,
                page_table_capacity(builder.retained)
            );
        }
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
/// This complements the [`registers`] checks performed after actual
/// resumption.
pub fn assert_parked_registers(saved: &SavedContext, key: RawKey) {
    for index in (8..=18).chain(20..=28) {
        assert_eq!(saved.gpr[index], registers::marker(index));
    }
    assert_eq!(saved.gpr[19], key.to_wire());
    assert_eq!(saved.gpr[29], saved.sp);
    assert_eq!(saved.lr, registers::marker(30));
    assert_eq!(saved.spsr_el1 & 0xF000_0000, registers::NZCV_PATTERN);
}

pub fn assert_rounds() {
    assert_eq!(SOURCE_ROUNDS.load(Ordering::Acquire), 4);
    assert_eq!(BOUNCE_ROUNDS.load(Ordering::Acquire), 3);
}

/// Test-local ordinary wait transport, not an Invocation/PPC ABI: one
/// [`registers::invoke`], asserting that the wait's resumption preserved
/// every register outside `x0..x2`, SP and NZCV. No source continuation lives
/// on `SP_EL1`; the probe's spill is userspace.
fn wait_with_registers(key: RawKey, op: u64, arg0: u64, arg1: u64) -> Result<u64, CapError> {
    let submitted = [arg0, arg1, 0, 0, 0, 0];
    let observed = registers::invoke(key, op, submitted);
    observed.assert_preserved(key, submitted);
    decode_syscall_result(observed.result()).map(|(value, _)| value)
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
