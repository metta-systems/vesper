//! Nucleus `Untyped` handler tests: the Retype transaction's rejection paths.
//!
//! The embedded test binary runs with the MMU off, so these tests exercise
//! only paths that reject before the kernel-private carve write or frame
//! sanitization (both go through the physical direct map); the successful
//! carve is covered by the kickstart boot test through the real SVC path.

#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(format_args_nl)]
#![feature(likely_unlikely)]
#![test_runner(libtest::test_runner)]
#![reexport_test_harness_main = "test_main"]

#[path = "../../../tests/common/mod.rs"]
mod common;

// Compile the production module trees, not replacement capability/handler models.
#[allow(unused)]
#[path = "../src/api/mod.rs"]
mod api;
#[allow(unused)]
#[path = "../src/objects/mod.rs"]
mod objects;

// The interrupt-controller component as the IRQ operations reach it.
#[allow(dead_code)]
#[path = "../src/platform.rs"]
mod platform;

// Only the clock is used here; the tick is armed by the nucleus binary.
#[allow(dead_code)]
#[path = "../src/timer.rs"]
mod timer;

use {
    api::KeyEntry,
    core::mem::MaybeUninit,
    libobject::{
        CapError, KeySlot, ObjectType, RawKey, Rights, TimeOp, UntypedOp, domain::DomainId,
    },
    // `Nucleus` is not used directly here: binding it at the crate root lets
    // the included production tree resolve `crate::Nucleus`
    // (objects/arch/aarch64_objects.rs, as the nucleus lib re-exports it).
    objects::{
        ArchObjectsImpl, KeyTable, Nucleus, ObjectPool,
        access::Access,
        arch::{AArch64PageTable, ArchPools},
        carved::{Carved, CarvedId},
        domain::DcbPages,
        nucleus::NucleusPools,
    },
};

// ═══════════════════════════════════════════════════════════════════
// FIXTURES
// ═══════════════════════════════════════════════════════════════════

/// Table-management permissions for the fixture's self-table capability.
const FULL: u8 = Rights::DERIVE | Rights::REMOVE | Rights::INSTALL;

/// Fixed RAM address for the test-carved `KeyTable` (QEMU rpi3: 1 GiB RAM at 0).
///
/// The test binary loads at `0x80000` and the DTB sits at `0x8000000`; 512 MiB
/// is clear of both. Carving from a fixed address keeps the large `KeyTable`
/// storage out of the test's stack frame.
const TEST_BACKING: u64 = 0x2000_0000;

/// Fixture-table capacity exponent: the historical 256-slot table.
const SIZE_BITS: u8 = 8;

/// The fixture table's guard (nonzero, exercising the guarded key-space
/// machinery: every fixture key carries it above the slot index). It fits the
/// 24 guard bits of a 256-entry table's table-relative address.
const FIXTURE_GUARD: u32 = 0x600_D00;

/// Carve the fixture's `KeyTable` at the fixed test backing, returning its
/// address.
fn carve_table() -> u64 {
    // SAFETY: TEST_BACKING is RAM, 32-byte aligned (a 512 MiB boundary), and
    // exclusively owned by the test fixture for its lifetime; the fixed
    // backing covers the full variable-size carve.
    unsafe {
        KeyTable::initialize(TEST_BACKING as *mut u8, DomainId(0), SIZE_BITS);
    }
    TEST_BACKING
}

/// Backing bytes for the fixture nucleus's pools (u128 elements give the
/// 16-byte minimum carve alignment). The Thread pool is built with zero
/// capacity (never dereferenced); the page-table metadata pool holds
/// exactly one slot so its exhaustion and rollback behavior is observable
/// without any carve write (these MMU-off tests only exercise rejection
/// paths).
static mut POOL_MEM: [u128; 8] = [0; 8];

/// Backing standing in for Untyped memory for the Time operation tests: a
/// carved root Time plus two more. Retype itself writes through the direct
/// map, which these MMU-off tests cannot reach, so they carve here directly.
#[repr(C, align(64))]
struct TimeBacking([u8; 4096]);
static mut TIME_MEM: TimeBacking = TimeBacking([0; 4096]);

/// A minimal fixture nucleus. Tests run sequentially and each fixture
/// re-initializes the same static storage before use.
fn fixture_nucleus() -> &'static mut Nucleus<ArchObjectsImpl> {
    static mut NUCLEUS_MEM: MaybeUninit<Nucleus<ArchObjectsImpl>> = MaybeUninit::uninit();
    // SAFETY: POOL_MEM and NUCLEUS_MEM are exclusively owned by the fixture;
    // initialization happens before any use, and tests are sequential. Raw
    // pointers avoid mutable references to statics (edition 2024).
    unsafe {
        let nucleus_ptr = (&raw mut NUCLEUS_MEM).cast::<Nucleus<ArchObjectsImpl>>();
        let pool_ptr = (&raw mut POOL_MEM).cast::<u8>();
        assert!(ObjectPool::<AArch64PageTable>::carve_size(1) <= core::mem::size_of::<[u128; 8]>());
        nucleus_ptr.write(Nucleus {
            current_thread: None,
            dcb_pages: DcbPages::new(),
            pending: crate::objects::PendingPool::new(),
            run_queue: crate::objects::RunQueue::new(),
            idle_thread: None,
            irq_table: crate::objects::irq::IrqTable::empty(),
            ticks: 0,
            pools: NucleusPools {
                arch: ArchPools::new(
                    ObjectPool::initialize(pool_ptr, 1),
                    // Zero capacity: these MMU-off rejection-path tests never
                    // invoke AddressSpace or ASIDPool operations.
                    ObjectPool::initialize(pool_ptr, 0),
                    ObjectPool::initialize(pool_ptr, 0),
                ),
            },
        });
        &mut *nucleus_ptr
    }
}

/// A carved-table fixture: the caller's own table with a self-table capability
/// at `SELF_KEYTABLE`, through which `Untyped.Retype` is invoked.
struct Fixture {
    table_addr: u64,
    self_key: RawKey,
    nucleus: &'static mut Nucleus<ArchObjectsImpl>,
}

impl Fixture {
    fn new(table_rights: u8) -> Self {
        let table_addr = carve_table();
        // SAFETY: table_addr names the freshly carved, live fixture table.
        let self_key = unsafe { &mut *(table_addr as *mut KeyTable) }
            .insert(
                KeySlot::SELF_KEYTABLE,
                KeyEntry::new_keytable(
                    table_addr,
                    FIXTURE_GUARD,
                    SIZE_BITS,
                    Rights(table_rights),
                    0,
                ),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|_| panic!("self-table installation failed"));
        Self {
            table_addr,
            self_key,
            nucleus: fixture_nucleus(),
        }
    }

    /// Install an entry into the fixture table.
    fn install(&mut self, slot: KeySlot, entry: KeyEntry) -> RawKey {
        // SAFETY: the address names a live carved table owned by the fixture.
        unsafe { &mut *(self.table_addr as *mut KeyTable) }
            .insert(slot, entry, FIXTURE_GUARD)
            .unwrap_or_else(|_| panic!("fixture installation failed"))
    }

    /// Look up an entry in the fixture table.
    fn lookup(&self, key: RawKey) -> Result<&KeyEntry, CapError> {
        // SAFETY: see install.
        Ok(unsafe { &*(self.table_addr as *const KeyTable) }.lookup(key, FIXTURE_GUARD)?)
    }

    /// The fixture table's live-entry count.
    fn len(&self) -> usize {
        // SAFETY: see install.
        unsafe { &*(self.table_addr as *const KeyTable) }.len()
    }

    /// Invoke the Untyped handler against the fixture.
    fn invoke(
        &mut self,
        untyped_key: RawKey,
        op: u64,
        args: &[u64; 6],
    ) -> Result<(u64, u64), CapError> {
        // SAFETY: test-only; no overlapping access context.
        let access = unsafe { Access::new() };
        let caller = crate::objects::key_table::CallerTable {
            addr: self.table_addr,
            guard: FIXTURE_GUARD,
        };
        api::untyped::invoke::<ArchObjectsImpl>(
            &access,
            caller,
            untyped_key,
            op,
            args,
            self.nucleus,
        )
    }
}

/// Encode Retype's approved wire schema (see `doc/capabilities-contract.md`):
/// `x2` object kind, `x3` `size_bits` with the table guard packed in bits 39:8
/// (KeyTable kind only; zero for every other kind), `x4` count, `x5`
/// destination-table key, `x6` first destination slot, `x7` requested rights.
fn retype_args(
    kind: ObjectType,
    size_bits: u8,
    guard: u32,
    count: u64,
    dst: RawKey,
    slot: KeySlot,
    rights: Rights,
) -> [u64; 6] {
    [
        u64::from(kind.as_u8()),
        (u64::from(guard) << 8) | u64::from(size_bits),
        count,
        dst.to_wire(),
        u64::from(slot.0),
        u64::from(rights.bits()),
    ]
}

/// A mock Untyped over normal RAM at 768 MiB (inside QEMU's 1 GiB, clear of
/// the fixture backing); never written by these rejection-path tests.
fn ram_untyped(size_bits: u8) -> KeyEntry {
    ram_untyped_at(0x3000_0000, size_bits)
}

/// A mock Untyped over normal RAM at an explicit base address.
fn ram_untyped_at(paddr: u64, size_bits: u8) -> KeyEntry {
    KeyEntry::new_untyped(paddr, size_bits, false, Rights::all())
}

/// A mock device-memory Untyped over rpi3 SoC peripherals (GPIO window at
/// `0x3F00_0000`, 16 MiB).
fn device_untyped(size_bits: u8) -> KeyEntry {
    KeyEntry::new_untyped(0x3F00_0000, size_bits, true, Rights::all())
}

// ═══════════════════════════════════════════════════════════════════
// DEVICE UNTYPED RETYPE
// ═══════════════════════════════════════════════════════════════════

/// A device Untyped is not a valid Retype source: no creatable kind is
/// device-capable, so the carve is rejected before any state changes.
#[test_case]
fn retype_rejects_device_untypeds_without_changing_state() {
    let mut fx = Fixture::new(FULL);
    let device = fx.install(KeySlot(30), device_untyped(24));
    let before = fx.len();

    let result = fx.invoke(
        device,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::KEY_TABLE,
            8,
            0,
            1,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(
        result,
        Err(CapError::InvalidObjectType(ObjectType::KEY_TABLE))
    ));

    // No partial state: the source entry is untouched (still a device
    // Untyped with its watermark at zero) and nothing was installed.
    assert_eq!(fx.len(), before);
    let entry = fx
        .lookup(device)
        .unwrap_or_else(|_| panic!("device entry missing"));
    let region = entry
        .as_untyped()
        .unwrap_or_else(|_| panic!("entry is not an Untyped"));
    assert!(region.is_device);
    assert_eq!(region.watermark_bytes(), 0);
}

/// The device restriction is reported even when the region could not fit the
/// carve anyway: source validation precedes the reservation.
#[test_case]
fn retype_device_rejection_precedes_capacity_validation() {
    let mut fx = Fixture::new(FULL);
    // Sixteen bytes cannot fit a KeyTable, but the device flag is still the
    // reported reason.
    let device = fx.install(KeySlot(30), device_untyped(4));

    let result = fx.invoke(
        device,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::KEY_TABLE,
            8,
            0,
            1,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(
        result,
        Err(CapError::InvalidObjectType(ObjectType::KEY_TABLE))
    ));
}

/// The same too-small region as normal RAM passes the device check and fails
/// at the reservation instead, proving the rejections above come from the
/// device flag rather than the size.
#[test_case]
fn retype_from_ram_reaches_capacity_validation() {
    let mut fx = Fixture::new(FULL);
    let tiny = fx.install(KeySlot(30), ram_untyped(4));
    let before = fx.len();

    let result = fx.invoke(
        tiny,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::KEY_TABLE,
            8,
            0,
            1,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(result, Err(CapError::InsufficientMemory)));

    // The reservation failed before any destination install.
    assert_eq!(fx.len(), before);
    let entry = fx
        .lookup(tiny)
        .unwrap_or_else(|_| panic!("ram entry missing"));
    let region = entry
        .as_untyped()
        .unwrap_or_else(|_| panic!("entry is not an Untyped"));
    assert!(!region.is_device);
    assert_eq!(region.watermark_bytes(), 0);
}

/// Source validation precedes destination resolution: an unissued
/// destination-table key is not reported when the source is already rejected.
#[test_case]
fn retype_device_rejection_precedes_destination_resolution() {
    let mut fx = Fixture::new(FULL);
    let device = fx.install(KeySlot(30), device_untyped(24));
    // A destination key that was never issued.
    let bogus = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, 200, 7);

    let result = fx.invoke(
        device,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::KEY_TABLE,
            8,
            0,
            1,
            bogus,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(
        result,
        Err(CapError::InvalidObjectType(ObjectType::KEY_TABLE))
    ));
}

// ═══════════════════════════════════════════════════════════════════
// EXTENT REPRESENTABILITY AND RANGE
// ═══════════════════════════════════════════════════════════════════

/// Regions whose size is not representable (`size_bits` at or above the
/// address width) are rejected with the region's own size instead of
/// panicking in the size shift.
#[test_case]
fn retype_rejects_unrepresentable_region_sizes() {
    for size_bits in [64_u8, 100, u8::MAX] {
        let mut fx = Fixture::new(FULL);
        let huge = fx.install(KeySlot(30), ram_untyped_at(0x3000_0000, size_bits));

        let result = fx.invoke(
            huge,
            UntypedOp::Retype as u64,
            &retype_args(
                ObjectType::KEY_TABLE,
                8,
                0,
                1,
                fx.self_key,
                KeySlot(40),
                Rights::all(),
            ),
        );
        assert!(
            matches!(result, Err(CapError::InvalidSize(size)) if size == usize::from(size_bits)),
            "size_bits {size_bits} must be rejected with its own size"
        );
    }
}

/// Regions whose extent overflows the physical address space (base + size
/// beyond `u64`) are rejected with the region's own size.
#[test_case]
fn retype_rejects_unrepresentable_region_extents() {
    let mut fx = Fixture::new(FULL);
    // Base near the top of the address space: adding a 4 GiB size overflows.
    let region = fx.install(KeySlot(30), ram_untyped_at(u64::MAX - 1024, 32));

    let result = fx.invoke(
        region,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::KEY_TABLE,
            8,
            0,
            1,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(result, Err(CapError::InvalidSize(32))));
}

/// The usable range ends where the watermark encoding does: a bounded run
/// (≤ 256 objects) that fits the region but ends beyond the largest
/// representable watermark is rejected as insufficient memory, before any
/// destination-slot iteration.
#[test_case]
fn retype_rejects_runs_beyond_the_watermark_encoding() {
    let mut fx = Fixture::new(FULL);
    // A 128 GiB region (size_bits 37) whose 65-object run of 1 GiB frames
    // (~65 GiB) fits the region but ends past the `u32` watermark encoding
    // (~64 GiB) — reachable within the batch bound.
    let region = fx.install(KeySlot(30), ram_untyped_at(0, 37));

    let result = fx.invoke(
        region,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::FRAME,
            30,
            0,
            65,
            fx.self_key,
            KeySlot(0),
            Rights::all(),
        ),
    );
    assert!(matches!(result, Err(CapError::InsufficientMemory)));
}

// ═══════════════════════════════════════════════════════════════════
// FRAME RETYPE
// ═══════════════════════════════════════════════════════════════════

/// Frame `size_bits` is architecture-validated: only the AArch64 granule
/// sizes (12/21/30) are creatable, and the rejection names the requested
/// size.
#[test_case]
fn retype_rejects_nongranular_frame_sizes() {
    for size_bits in [0_u8, 1, 13, 22, 31, u8::MAX] {
        let mut fx = Fixture::new(FULL);
        let ram = fx.install(KeySlot(30), ram_untyped(24));

        let result = fx.invoke(
            ram,
            UntypedOp::Retype as u64,
            &retype_args(
                ObjectType::FRAME,
                size_bits,
                0,
                1,
                fx.self_key,
                KeySlot(40),
                Rights::all(),
            ),
        );
        assert!(
            matches!(result, Err(CapError::InvalidFrameSize(size)) if size == usize::from(size_bits)),
            "size_bits {size_bits} must be rejected with its own size"
        );
    }
}

/// Frame-size validation precedes source resolution: an unissued source key
/// is not reported when the size is already rejected.
#[test_case]
fn retype_frame_size_rejection_precedes_source_resolution() {
    let mut fx = Fixture::new(FULL);
    // A source key that was never issued.
    let bogus = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, 200, 7);

    let result = fx.invoke(
        bogus,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::FRAME,
            13,
            0,
            1,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(result, Err(CapError::InvalidFrameSize(13))));
}

/// A device Untyped carves device Frames without touching the registers: the
/// carve writes no bytes (so it runs MMU-off), the Frame records `is_device`,
/// and the source watermark advances by the frame size.
#[test_case]
fn retype_carves_device_frames_without_touching_them() {
    let mut fx = Fixture::new(FULL);
    let device = fx.install(KeySlot(30), device_untyped(24));
    let before = fx.len();

    let (frame_wire, _) = fx
        .invoke(
            device,
            UntypedOp::Retype as u64,
            &retype_args(
                ObjectType::FRAME,
                12,
                0,
                2,
                fx.self_key,
                KeySlot(40),
                Rights::all(),
            ),
        )
        .unwrap_or_else(|e| panic!("device Frame retype failed: {:?}", e.code()));
    assert_eq!(fx.len(), before + 2);
    let frame = *fx
        .lookup(RawKey::from_wire(frame_wire))
        .unwrap_or_else(|_| panic!("device Frame missing"))
        .as_frame()
        .unwrap_or_else(|_| panic!("entry is not a Frame"));
    assert!(frame.is_device());
    assert_eq!(frame.paddr, 0x3F00_0000);
    let region = *fx
        .lookup(device)
        .unwrap_or_else(|_| panic!("device entry missing"))
        .as_untyped()
        .unwrap_or_else(|_| panic!("entry is not an Untyped"));
    assert!(region.is_device);
    assert_eq!(region.watermark_bytes(), 2 * 4096);
}

/// A 4 KiB frame that cannot fit the region is rejected at the reservation
/// with the source accounting unchanged (the RAM contrast for frames).
#[test_case]
fn retype_frame_from_ram_reaches_capacity_validation() {
    let mut fx = Fixture::new(FULL);
    // Sixteen bytes cannot fit a 4 KiB frame, but the RAM source passes the
    // device check and fails at the reservation instead.
    let tiny = fx.install(KeySlot(30), ram_untyped(4));
    let before = fx.len();

    let result = fx.invoke(
        tiny,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::FRAME,
            12,
            0,
            1,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(result, Err(CapError::InsufficientMemory)));

    // The reservation failed before any destination install.
    assert_eq!(fx.len(), before);
    let entry = fx
        .lookup(tiny)
        .unwrap_or_else(|_| panic!("ram entry missing"));
    let region = entry
        .as_untyped()
        .unwrap_or_else(|_| panic!("entry is not an Untyped"));
    assert!(!region.is_device);
    assert_eq!(region.watermark_bytes(), 0);
}

// ═══════════════════════════════════════════════════════════════════
// PAGE TABLE RETYPE
// ═══════════════════════════════════════════════════════════════════

/// A PageTable is a fixed 4 KiB architecture carve: other size_bits are
/// rejected with the architecture's own error, leaving the state unchanged.
#[test_case]
fn retype_rejects_non_arch_page_table_sizes_without_changing_state() {
    let mut fx = Fixture::new(FULL);
    let ram = fx.install(KeySlot(30), ram_untyped(24));
    let before = fx.len();

    let result = fx.invoke(
        ram,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::PAGE_TABLE,
            13,
            0,
            1,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(result, Err(CapError::InvalidSize(13))));

    // Validation failed before any reservation or destination install.
    assert_eq!(fx.len(), before);
    let region = fx
        .lookup(ram)
        .unwrap_or_else(|_| panic!("ram entry missing"))
        .as_untyped()
        .unwrap_or_else(|_| panic!("entry is not an Untyped"));
    assert_eq!(region.watermark_bytes(), 0);
}

/// A device Untyped is not a valid source for PageTable carves either: the
/// general device-source rejection covers every creatable kind.
#[test_case]
fn retype_rejects_device_untypeds_for_page_tables() {
    let mut fx = Fixture::new(FULL);
    let device = fx.install(KeySlot(30), device_untyped(24));
    let before = fx.len();

    let result = fx.invoke(
        device,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::PAGE_TABLE,
            12,
            0,
            1,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(
        result,
        Err(CapError::InvalidObjectType(ObjectType::PAGE_TABLE))
    ));
    assert_eq!(fx.len(), before);
}

/// A PageTable batch that cannot fit the metadata pool is rejected with
/// `PoolExhausted`, and the partially allocated slots are released: the
/// failure precedes any carve write or destination install, and a fresh
/// fixture exhausts at the same point (the pool capacity did not leak).
#[test_case]
fn retype_page_table_batch_releases_partial_pool_slots_on_exhaustion() {
    let mut fx = Fixture::new(FULL);
    let ram = fx.install(KeySlot(30), ram_untyped(24));
    let before = fx.len();

    // The fixture's page-table metadata pool holds exactly one slot, so a
    // batch of two exhausts it after the first allocation.
    let result = fx.invoke(
        ram,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::PAGE_TABLE,
            12,
            0,
            2,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(result, Err(CapError::PoolExhausted)));

    // The rollback released the first metadata slot and installed nothing.
    assert_eq!(fx.len(), before);
    let region = fx
        .lookup(ram)
        .unwrap_or_else(|_| panic!("ram entry missing"))
        .as_untyped()
        .unwrap_or_else(|_| panic!("entry is not an Untyped"));
    assert_eq!(region.watermark_bytes(), 0);

    // The released slot is usable again: a fresh fixture batch of two still
    // exhausts at the same point.
    let mut fx2 = Fixture::new(FULL);
    let ram2 = fx2.install(KeySlot(30), ram_untyped(24));
    let result2 = fx2.invoke(
        ram2,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::PAGE_TABLE,
            12,
            0,
            2,
            fx2.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(result2, Err(CapError::PoolExhausted)));
}

// ═══════════════════════════════════════════════════════════════
// NOTIFICATION, EVENTCOUNT AND TIME RETYPE (carved from the Untyped)
// ═══════════════════════════════════════════════════════════════

/// The kinds carved from the invoking Untyped's memory with `size_bits`
/// reserved zero. Their successful Retype writes through the direct map, so
/// it runs in the boot test kernels; these MMU-off tests cover the
/// rejections, which happen before any byte is written.
const CARVED_KINDS: [ObjectType; 4] = [
    ObjectType::NOTIFICATION,
    ObjectType::EVENT_COUNT,
    ObjectType::TIME,
    ObjectType::THREAD,
];

/// Nonzero `size_bits`, a device source and an Untyped too small for the
/// batch are rejected with the table and the watermark unchanged.
#[test_case]
fn retype_carved_kinds_reject_size_bits_device_sources_and_small_untypeds() {
    for kind in CARVED_KINDS {
        let mut fx = Fixture::new(FULL);
        let ram = fx.install(KeySlot(30), ram_untyped(24));
        let device = fx.install(KeySlot(31), device_untyped(24));
        // 64 bytes: smaller than any carved record.
        let small = fx.install(KeySlot(32), ram_untyped(6));
        let before = fx.len();
        let retype = |fx: &mut Fixture, source, size_bits, count| {
            fx.invoke(
                source,
                UntypedOp::Retype as u64,
                &retype_args(
                    kind,
                    size_bits,
                    0,
                    count,
                    fx.self_key,
                    KeySlot(40),
                    Rights::all(),
                ),
            )
        };
        assert!(matches!(
            retype(&mut fx, ram, 12, 1),
            Err(CapError::InvalidSize(12))
        ));
        assert!(matches!(
            retype(&mut fx, device, 0, 1),
            Err(CapError::InvalidObjectType(rejected)) if rejected == kind
        ));
        assert!(matches!(
            retype(&mut fx, small, 0, 1),
            Err(CapError::InsufficientMemory)
        ));
        assert_eq!(fx.len(), before);
        for source in [ram, small] {
            let region = fx
                .lookup(source)
                .unwrap_or_else(|_| panic!("untyped entry missing"))
                .as_untyped()
                .unwrap_or_else(|_| panic!("entry is not an Untyped"));
            assert_eq!(region.watermark_bytes(), 0);
        }
    }
}

// ═══════════════════════════════════════════════════════════════
// UNTYPED SPLIT (allowlisted 2026-09-23)
// ═══════════════════════════════════════════════════════════════

/// A split writes no Untyped bytes — the child capability is an inline
/// region — so the successful Retype is exercisable MMU-off. The child
/// records the aligned carve base, its own `size_bits`, a zero watermark,
/// and the requested rights; the parent's watermark advances by exactly
/// the child region.
#[test_case]
fn retype_untyped_split_succeeds_and_installs_child_regions() {
    let mut fx = Fixture::new(FULL);
    let ram = fx.install(KeySlot(30), ram_untyped(24));
    let before = fx.len();

    let result = fx.invoke(
        ram,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::UNTYPED,
            8,
            0,
            1,
            fx.self_key,
            KeySlot(40),
            Rights(Rights::READ | Rights::WRITE),
        ),
    );
    let (first_wire, second) =
        result.unwrap_or_else(|e| panic!("untyped split failed: {:?}", e.code()));
    assert_eq!(second, 0);
    assert_eq!(fx.len(), before + 1);

    let child = RawKey::from_wire(first_wire);
    assert_eq!(
        child.slot(),
        KeySlot((FIXTURE_GUARD << u32::from(SIZE_BITS)) | 40)
    );
    let entry = fx
        .lookup(child)
        .unwrap_or_else(|_| panic!("split child missing"));
    assert_eq!(entry.object_type(), ObjectType::UNTYPED);
    assert_eq!(entry.rights().bits(), Rights::READ | Rights::WRITE);
    let region = entry
        .as_untyped()
        .unwrap_or_else(|_| panic!("split child is not a region"));
    assert_eq!(region.paddr, 0x3000_0000);
    assert_eq!(region.size_bits, 8);
    assert!(!region.is_device);
    assert_eq!(region.watermark_bytes(), 0, "child starts fully unused");

    // The parent advanced by exactly the child region (256 bytes).
    let parent = fx
        .lookup(ram)
        .unwrap_or_else(|_| panic!("ram entry missing"))
        .as_untyped()
        .unwrap_or_else(|_| panic!("entry is not an Untyped"));
    assert_eq!(parent.watermark_bytes(), 256);
}

/// A batch split installs consecutive children at consecutive carve
/// addresses with no gap or overlap, and each child starts unused.
#[test_case]
fn retype_untyped_batch_split_installs_consecutive_children() {
    let mut fx = Fixture::new(FULL);
    let ram = fx.install(KeySlot(30), ram_untyped(24));
    let before = fx.len();

    let result = fx.invoke(
        ram,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::UNTYPED,
            8,
            0,
            3,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    let (first_wire, _) =
        result.unwrap_or_else(|e| panic!("untyped batch split failed: {:?}", e.code()));
    assert_eq!(fx.len(), before + 3);

    let first = RawKey::from_wire(first_wire);
    for (index, slot) in [(0_u64, KeySlot(40)), (1, KeySlot(41)), (2, KeySlot(42))] {
        let key = if index == 0 {
            first
        } else {
            RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, slot.0, first.incarnation())
        };
        let region = fx
            .lookup(key)
            .unwrap_or_else(|_| panic!("split child missing at {slot:?}"))
            .as_untyped()
            .unwrap_or_else(|_| panic!("split child at {slot:?} is not a region"));
        assert_eq!(region.paddr, 0x3000_0000 + 256 * index);
        assert_eq!(region.size_bits, 8);
        assert_eq!(region.watermark_bytes(), 0);
    }

    // The parent advanced by exactly the three child regions.
    let parent = fx
        .lookup(ram)
        .unwrap_or_else(|_| panic!("ram entry missing"))
        .as_untyped()
        .unwrap_or_else(|_| panic!("entry is not an Untyped"));
    assert_eq!(parent.watermark_bytes(), 3 * 256);
}

/// A split child is itself a working Retype source: a further split
/// allocates from the child's own watermark range.
#[test_case]
fn retype_untyped_split_child_is_a_working_source() {
    let mut fx = Fixture::new(FULL);
    let ram = fx.install(KeySlot(30), ram_untyped(24));

    let (child_wire, _) = fx
        .invoke(
            ram,
            UntypedOp::Retype as u64,
            &retype_args(
                ObjectType::UNTYPED,
                8,
                0,
                1,
                fx.self_key,
                KeySlot(40),
                Rights::all(),
            ),
        )
        .unwrap_or_else(|e| panic!("untyped split failed: {:?}", e.code()));
    let child = RawKey::from_wire(child_wire);

    // A further split from the child carves the grandchild at the child's
    // own watermark and advances it.
    let (grandchild_wire, _) = fx
        .invoke(
            child,
            UntypedOp::Retype as u64,
            &retype_args(
                ObjectType::UNTYPED,
                4,
                0,
                1,
                fx.self_key,
                KeySlot(42),
                Rights::all(),
            ),
        )
        .unwrap_or_else(|e| panic!("split from child failed: {:?}", e.code()));
    let grandchild = fx
        .lookup(RawKey::from_wire(grandchild_wire))
        .unwrap_or_else(|_| panic!("grandchild entry missing"))
        .as_untyped()
        .unwrap_or_else(|_| panic!("grandchild entry is not a region"));
    assert_eq!(grandchild.paddr, 0x3000_0000);
    assert_eq!(grandchild.size_bits, 4);
    let region = fx
        .lookup(child)
        .unwrap_or_else(|_| panic!("child entry missing"))
        .as_untyped()
        .unwrap_or_else(|_| panic!("child entry is not a region"));
    assert_eq!(region.watermark_bytes(), 16);
}

/// A child smaller than the watermark encoding granularity is rejected
/// with its own `size_bits`: a sub-granular region could not keep its
/// committed carve ends encodable.
#[test_case]
fn retype_untyped_split_rejects_subgranular_sizes() {
    let mut fx = Fixture::new(FULL);
    let ram = fx.install(KeySlot(30), ram_untyped(24));
    let before = fx.len();

    for size_bits in 0_u8..4 {
        let result = fx.invoke(
            ram,
            UntypedOp::Retype as u64,
            &retype_args(
                ObjectType::UNTYPED,
                size_bits,
                0,
                1,
                fx.self_key,
                KeySlot(40),
                Rights::all(),
            ),
        );
        assert!(
            matches!(result, Err(CapError::InvalidSize(size)) if size == usize::from(size_bits)),
            "size_bits {size_bits} must be rejected with its own exponent"
        );
    }

    // No state changed across the rejections.
    assert_eq!(fx.len(), before);
    let region = fx
        .lookup(ram)
        .unwrap_or_else(|_| panic!("ram entry missing"))
        .as_untyped()
        .unwrap_or_else(|_| panic!("entry is not an Untyped"));
    assert_eq!(region.watermark_bytes(), 0);
}

/// Unrepresentable child sizes are rejected with the child's own
/// `size_bits` before the source is even resolved.
#[test_case]
fn retype_untyped_split_rejects_unrepresentable_sizes() {
    let mut fx = Fixture::new(FULL);
    let ram = fx.install(KeySlot(30), ram_untyped(24));
    let before = fx.len();

    for size_bits in [64_u8, 255] {
        let result = fx.invoke(
            ram,
            UntypedOp::Retype as u64,
            &retype_args(
                ObjectType::UNTYPED,
                size_bits,
                0,
                1,
                fx.self_key,
                KeySlot(40),
                Rights::all(),
            ),
        );
        assert!(
            matches!(result, Err(CapError::InvalidSize(size)) if size == usize::from(size_bits))
        );
    }
    assert_eq!(fx.len(), before);
}

/// A child that cannot fit the source's unused range is rejected with
/// `InsufficientMemory`; an exact-fit split consumes the whole region and
/// the next split of the same size no longer fits. Both rejections leave
/// the source unchanged.
#[test_case]
fn retype_untyped_split_enforces_the_fit_and_failure_atomicity() {
    let mut fx = Fixture::new(FULL);
    // A 16-byte region: exactly one minimum-size child fits.
    let ram = fx.install(KeySlot(30), ram_untyped(4));
    let before = fx.len();

    let oversized = fx.invoke(
        ram,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::UNTYPED,
            5,
            0,
            1,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(oversized, Err(CapError::InsufficientMemory)));
    assert_eq!(fx.len(), before);
    let region = fx
        .lookup(ram)
        .unwrap_or_else(|_| panic!("ram entry missing"))
        .as_untyped()
        .unwrap_or_else(|_| panic!("entry is not an Untyped"));
    assert_eq!(region.watermark_bytes(), 0);

    // The exact fit succeeds and consumes the whole region.
    let (exact_wire, _) = fx
        .invoke(
            ram,
            UntypedOp::Retype as u64,
            &retype_args(
                ObjectType::UNTYPED,
                4,
                0,
                1,
                fx.self_key,
                KeySlot(40),
                Rights::all(),
            ),
        )
        .unwrap_or_else(|e| panic!("exact-fit split failed: {:?}", e.code()));
    let exact_child = fx
        .lookup(RawKey::from_wire(exact_wire))
        .unwrap_or_else(|_| panic!("exact-fit child missing"))
        .as_untyped()
        .unwrap_or_else(|_| panic!("exact-fit child is not a region"));
    assert_eq!(exact_child.paddr, 0x3000_0000);
    assert_eq!(exact_child.size_bits, 4);
    assert_eq!(exact_child.watermark_bytes(), 0);
    let region = fx
        .lookup(ram)
        .unwrap_or_else(|_| panic!("ram entry missing"))
        .as_untyped()
        .unwrap_or_else(|_| panic!("entry is not an Untyped"));
    assert_eq!(region.watermark_bytes(), 16);

    // The exhausted source rejects a further same-size split: the
    // reservation fails before destination pre-validation, and the source
    // and the table are unchanged.
    let exhausted = fx.invoke(
        ram,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::UNTYPED,
            4,
            0,
            1,
            fx.self_key,
            KeySlot(41),
            Rights::all(),
        ),
    );
    assert!(matches!(exhausted, Err(CapError::InsufficientMemory)));
    let region = fx
        .lookup(ram)
        .unwrap_or_else(|_| panic!("ram entry missing"))
        .as_untyped()
        .unwrap_or_else(|_| panic!("entry is not an Untyped"));
    assert_eq!(region.watermark_bytes(), 16);
    assert_eq!(fx.len(), before + 1);

    // An occupied destination slot is rejected at pre-validation before
    // any carve — this needs a source with room, since the reservation
    // above runs first: the split leaves the source and table unchanged.
    let mut fx2 = Fixture::new(FULL);
    let ram2 = fx2.install(KeySlot(30), ram_untyped(24));
    let before2 = fx2.len();
    let (first_wire, _) = fx2
        .invoke(
            ram2,
            UntypedOp::Retype as u64,
            &retype_args(
                ObjectType::UNTYPED,
                8,
                0,
                1,
                fx2.self_key,
                KeySlot(40),
                Rights::all(),
            ),
        )
        .unwrap_or_else(|e| panic!("split into slot 40 failed: {:?}", e.code()));
    assert_eq!(
        RawKey::from_wire(first_wire).slot(),
        KeySlot((FIXTURE_GUARD << u32::from(SIZE_BITS)) | 40)
    );
    assert_eq!(fx2.len(), before2 + 1);
    let occupied = fx2.invoke(
        ram2,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::UNTYPED,
            8,
            0,
            1,
            fx2.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(occupied, Err(CapError::SlotOccupied(KeySlot(40)))));
    let region = fx2
        .lookup(ram2)
        .unwrap_or_else(|_| panic!("ram entry missing"))
        .as_untyped()
        .unwrap_or_else(|_| panic!("entry is not an Untyped"));
    assert_eq!(region.watermark_bytes(), 256);
    assert_eq!(fx2.len(), before2 + 1);
}

/// A device Untyped may be split (the one device-source carve): the
/// `is_device` flag propagates to the children, who remain device sources
/// for every other kind.
#[test_case]
fn retype_untyped_split_from_device_propagates_the_device_flag() {
    let mut fx = Fixture::new(FULL);
    let device = fx.install(KeySlot(30), device_untyped(24));
    let before = fx.len();

    let (child_wire, _) = fx
        .invoke(
            device,
            UntypedOp::Retype as u64,
            &retype_args(
                ObjectType::UNTYPED,
                8,
                0,
                1,
                fx.self_key,
                KeySlot(40),
                Rights::all(),
            ),
        )
        .unwrap_or_else(|e| panic!("device split failed: {:?}", e.code()));
    assert_eq!(fx.len(), before + 1);

    let child = RawKey::from_wire(child_wire);
    let region = fx
        .lookup(child)
        .unwrap_or_else(|_| panic!("device child missing"))
        .as_untyped()
        .unwrap_or_else(|_| panic!("device child is not a region"));
    assert_eq!(region.paddr, 0x3F00_0000);
    assert_eq!(region.size_bits, 8);
    assert!(region.is_device, "the device flag must propagate");
    assert_eq!(region.watermark_bytes(), 0);

    // The device child is still a device source: every other kind is
    // rejected from it without any state change.
    let rejected = fx.invoke(
        child,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::KEY_TABLE,
            8,
            0,
            1,
            fx.self_key,
            KeySlot(41),
            Rights::all(),
        ),
    );
    assert!(matches!(
        rejected,
        Err(CapError::InvalidObjectType(ObjectType::KEY_TABLE))
    ));

    // Splitting the device child again works and keeps the flag.
    let (grandchild_wire, _) = fx
        .invoke(
            child,
            UntypedOp::Retype as u64,
            &retype_args(
                ObjectType::UNTYPED,
                4,
                0,
                1,
                fx.self_key,
                KeySlot(41),
                Rights::all(),
            ),
        )
        .unwrap_or_else(|e| panic!("device re-split failed: {:?}", e.code()));
    let grandchild = fx
        .lookup(RawKey::from_wire(grandchild_wire))
        .unwrap_or_else(|_| panic!("device grandchild missing"))
        .as_untyped()
        .unwrap_or_else(|_| panic!("device grandchild is not a region"));
    assert_eq!(grandchild.paddr, 0x3F00_0000);
    assert_eq!(grandchild.size_bits, 4);
    assert!(grandchild.is_device);
}

// ═══════════════════════════════════════════════════════════════
// KEYTABLE RETYPE SCHEMA (guarded key-space package, 2026-09-23)
// ═══════════════════════════════════════════════════════════════

/// A successful KeyTable carve writes the table at the carve (header,
/// entries, counters), so it is covered by the boot test through the real
/// SVC path; these tests exercise the new schema's rejection paths, which
/// all precede any carve write.
#[test_case]
fn retype_keytable_rejects_out_of_range_capacity_exponents() {
    let mut fx = Fixture::new(FULL);
    let ram = fx.install(KeySlot(30), ram_untyped(24));
    let before = fx.len();

    // size_bits selects the capacity (1..=20): zero and twenty-one are
    // rejected with the requested exponent.
    for size_bits in [0_u8, 21, 255] {
        let result = fx.invoke(
            ram,
            UntypedOp::Retype as u64,
            &retype_args(
                ObjectType::KEY_TABLE,
                size_bits,
                0,
                1,
                fx.self_key,
                KeySlot(40),
                Rights::all(),
            ),
        );
        assert!(
            matches!(result, Err(CapError::InvalidSize(size)) if size == usize::from(size_bits)),
            "size_bits {size_bits} must be rejected with its own exponent"
        );
    }
    assert_eq!(fx.len(), before);
}

/// The packed guard must fit the table-relative address layout —
/// `32 − size_bits` bits above the slot index — and bits 63:40 of `x3` are
/// reserved zero.
#[test_case]
fn retype_keytable_rejects_guards_that_do_not_fit_the_layout() {
    let mut fx = Fixture::new(FULL);
    let ram = fx.install(KeySlot(30), ram_untyped(24));
    let before = fx.len();

    // A 256-entry table leaves 24 guard bits: 2^24 does not fit.
    let result = fx.invoke(
        ram,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::KEY_TABLE,
            8,
            1 << 24,
            1,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(result, Err(CapError::InvalidSize(8))));

    // Bits 63:40 of the packed word are reserved zero.
    let mut args = retype_args(
        ObjectType::KEY_TABLE,
        8,
        0,
        1,
        fx.self_key,
        KeySlot(40),
        Rights::all(),
    );
    args[1] |= 1_u64 << 40;
    let result = fx.invoke(ram, UntypedOp::Retype as u64, &args);
    assert!(matches!(result, Err(CapError::InvalidSize(8))));

    // Every rejection preceded any carve write: nothing was installed.
    assert_eq!(fx.len(), before);
}

/// Every non-KeyTable kind requires bits 63:8 of `x3` zero (the strict zero
/// convention): a packed guard word is malformed input for them.
#[test_case]
fn retype_rejects_guard_words_for_non_keytable_kinds() {
    let mut fx = Fixture::new(FULL);
    let ram = fx.install(KeySlot(30), ram_untyped(24));
    let before = fx.len();

    let result = fx.invoke(
        ram,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::FRAME,
            12,
            1,
            1,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(result, Err(CapError::InvalidOperation)));
    assert_eq!(fx.len(), before);
}

/// Requested rights outside `Rights::all()` are malformed input: Retype
/// installs the requested mask verbatim on the new origin capability, so an
/// undefined bit must never reach the table.
#[test_case]
fn retype_rejects_undefined_rights_bits() {
    let mut fx = Fixture::new(FULL);
    let ram = fx.install(KeySlot(30), ram_untyped(24));
    let before = fx.len();

    for rights_word in [0x40_u64, 0x80, 0xFF, 1 << 8, u64::MAX] {
        let mut args = retype_args(
            ObjectType::FRAME,
            12,
            0,
            1,
            fx.self_key,
            KeySlot(40),
            Rights::empty(),
        );
        args[5] = rights_word;
        let result = fx.invoke(ram, UntypedOp::Retype as u64, &args);
        assert!(
            matches!(result, Err(CapError::InvalidOperation)),
            "rights word {rights_word:#x} must be rejected"
        );
    }
    assert_eq!(fx.len(), before);
}

/// A Retype batch is bounded (`MAX_RETYPE_BATCH`, 256): the transaction's
/// defensive rollback records are stack arrays sized by the bound. Larger
/// counts are malformed input.
#[test_case]
fn retype_rejects_batches_beyond_the_bound() {
    let mut fx = Fixture::new(FULL);
    let ram = fx.install(KeySlot(30), ram_untyped(24));
    let before = fx.len();

    let result = fx.invoke(
        ram,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::NOTIFICATION,
            0,
            0,
            257,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(result, Err(CapError::InvalidOperation)));
    assert_eq!(fx.len(), before);
}

// ═══════════════════════════════════════════════════════════════
// TIME
// ═══════════════════════════════════════════════════════════════

/// Bytes of [`TIME_MEM`] each fixture Time takes.
const TIME_STRIDE: usize = 512;

impl Fixture {
    /// Carve `time` into fixture slot `index` of [`TIME_MEM`] (standing in
    /// for an Untyped carve) and install its capability at `slot`.
    fn install_time(
        &mut self,
        slot: KeySlot,
        index: usize,
        rights: Rights,
        time: impl FnOnce(CarvedId) -> crate::objects::Time,
    ) -> RawKey {
        assert!(Carved::<crate::objects::Time>::SIZE <= TIME_STRIDE);
        assert!((index + 1) * TIME_STRIDE <= core::mem::size_of::<TimeBacking>());
        // SAFETY: the backing is exclusively owned by the sequential tests,
        // each index is carved once per test, and the stride keeps records
        // apart and 64-byte aligned.
        let id = unsafe {
            let address = (&raw mut TIME_MEM).cast::<u8>().add(index * TIME_STRIDE);
            let id =
                Carved::<crate::objects::Time>::initialize(address, crate::objects::Time::empty(0));
            *Carved::<crate::objects::Time>::validate(id).expect("Time just carved") = time(id);
            id
        };
        self.install(slot, KeyEntry::new_carved(id, rights, 0))
    }

    /// Install a root Time (budget `budget_ns`, chain naming itself at
    /// priority 0) at `slot` with `rights`, as Kickstart does for the boot
    /// Time.
    fn install_root_time(
        &mut self,
        slot: KeySlot,
        index: usize,
        budget_ns: u64,
        rights: Rights,
    ) -> RawKey {
        self.install_time(slot, index, rights, |id| {
            crate::objects::Time::root(0, id, budget_ns, 0)
        })
    }

    /// Install an empty Time at `slot`.
    fn install_empty_time(&mut self, slot: KeySlot, index: usize) -> RawKey {
        self.install_time(slot, index, Rights::all(), |_| {
            crate::objects::Time::empty(0)
        })
    }

    /// Invoke the Time handler against the fixture.
    fn invoke_time(
        &mut self,
        key: RawKey,
        op: u64,
        args: &[u64; 6],
    ) -> Result<(u64, u64), CapError> {
        // SAFETY: test-only; no overlapping access context.
        let access = unsafe { Access::new() };
        let caller = crate::objects::key_table::CallerTable {
            addr: self.table_addr,
            guard: FIXTURE_GUARD,
        };
        api::time::invoke(&access, caller, key, op, args)
    }

    fn query(&mut self, key: RawKey) -> (u64, u64) {
        self.invoke_time(key, TimeOp::Query as u64, &[0; 6])
            .unwrap_or_else(|e| panic!("query failed: {:?}", e.code()))
    }
}

/// Transfer moves budget and records the sender's priority; Merge moves
/// everything back and empties the source.
#[test_case]
fn time_transfer_query_and_merge_conserve_budget() {
    let mut fx = Fixture::new(FULL);
    let root = fx.install_root_time(KeySlot(17), 0, 1_000, Rights::all());
    let child = fx.install_empty_time(KeySlot(40), 1);

    assert_eq!(fx.query(root), (1_000, 0));
    assert_eq!(fx.query(child), (0, u64::from(u16::MAX)));

    fx.invoke_time(
        root,
        TimeOp::Transfer as u64,
        &[child.to_wire(), 400, 0x8000, 0, 0, 0],
    )
    .unwrap_or_else(|e| panic!("transfer failed: {:?}", e.code()));
    assert_eq!(fx.query(root), (600, 0));
    assert_eq!(fx.query(child), (400, 0x8000));

    fx.invoke_time(
        root,
        TimeOp::Merge as u64,
        &[child.to_wire(), 0, 0, 0, 0, 0],
    )
    .unwrap_or_else(|e| panic!("merge failed: {:?}", e.code()));
    assert_eq!(fx.query(root), (1_000, 0));
    assert_eq!(fx.query(child), (0, u64::from(u16::MAX)));
}

/// Every rejected Transfer leaves both Times unchanged.
#[test_case]
fn time_transfer_failures_change_nothing() {
    let mut fx = Fixture::new(FULL);
    let root = fx.install_root_time(KeySlot(17), 0, 100, Rights::all());
    let child = fx.install_empty_time(KeySlot(40), 1);
    let read_only = fx.install_root_time(KeySlot(41), 2, 100, Rights(Rights::READ));
    let transfer = |fx: &mut Fixture, source: RawKey, args: [u64; 6]| {
        fx.invoke_time(source, TimeOp::Transfer as u64, &args)
    };
    let to_child = child.to_wire();
    let table_key = fx.self_key.to_wire();

    assert!(matches!(
        transfer(&mut fx, root, [to_child, 101, 1, 0, 0, 0]),
        Err(CapError::InsufficientBudget { remaining: 100 })
    ));
    assert!(matches!(
        transfer(&mut fx, root, [to_child, 0, 1, 0, 0, 0]),
        Err(CapError::InvalidOperation)
    ));
    assert!(matches!(
        transfer(&mut fx, root, [to_child, 1, 0x1_0000, 0, 0, 0]),
        Err(CapError::InvalidOperation)
    ));
    assert!(matches!(
        transfer(&mut fx, root, [to_child, 1, 1, 1, 0, 0]),
        Err(CapError::InvalidOperation)
    ));
    // The same Time as both operands.
    assert!(matches!(
        transfer(&mut fx, root, [root.to_wire(), 1, 1, 0, 0, 0]),
        Err(CapError::InvalidOperation)
    ));
    // WRITE is needed on the source and on the destination.
    assert!(matches!(
        transfer(&mut fx, read_only, [to_child, 1, 1, 0, 0, 0]),
        Err(CapError::InsufficientRights)
    ));
    assert!(matches!(
        transfer(&mut fx, root, [read_only.to_wire(), 1, 1, 0, 0, 0]),
        Err(CapError::InsufficientRights)
    ));
    // The destination must be a Time.
    assert!(matches!(
        transfer(&mut fx, root, [table_key, 1, 1, 0, 0, 0]),
        Err(CapError::TypeMismatch {
            expected: ObjectType::TIME,
            ..
        })
    ));

    assert_eq!(fx.query(root), (100, 0));
    assert_eq!(fx.query(child), (0, u64::from(u16::MAX)));
}
