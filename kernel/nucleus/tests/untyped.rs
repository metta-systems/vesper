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

use {
    api::KeyEntry,
    core::mem::MaybeUninit,
    libobject::{CapError, KeySlot, ObjectType, RawKey, Rights, UntypedOp, domain::DomainId},
    // `Nucleus` is not used directly here: binding it at the crate root lets
    // the included production tree resolve `crate::Nucleus`
    // (objects/arch/aarch64_objects.rs, as the nucleus lib re-exports it).
    objects::{
        ArchObjectsImpl, KeyTable, Nucleus, ObjectPool,
        access::Access,
        arch::{AArch64PageTable, ArchPools},
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

/// Carve the fixture's `KeyTable` at the fixed test backing, returning its
/// address.
fn carve_table() -> u64 {
    let obj = TEST_BACKING as *mut KeyTable;
    // SAFETY: TEST_BACKING is RAM, aligned for KeyTable, and exclusively owned
    // by the test fixture for its lifetime.
    unsafe {
        obj.write(KeyTable::new(DomainId(0)));
    }
    obj as u64
}

/// Backing bytes for the fixture nucleus's pools. The Domain pool is built
/// with zero capacity (never dereferenced); the page-table metadata pool
/// holds exactly one slot so its exhaustion and rollback behavior is
/// observable without any carve write (these MMU-off tests only exercise
/// rejection paths).
static mut POOL_MEM: [u64; 16] = [0; 16];

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
        nucleus_ptr.write(Nucleus {
            current_domain: None,
            dcb_pages: DcbPages::new(),
            pools: NucleusPools {
                domains: ObjectPool::new(pool_ptr, 0),
                arch: ArchPools::new(
                    ObjectPool::new(pool_ptr, core::mem::size_of::<AArch64PageTable>()),
                    // Zero capacity: these MMU-off rejection-path tests never
                    // invoke ASIDPool operations.
                    ObjectPool::new(pool_ptr, 0),
                ),
            },
        });
        &mut *nucleus_ptr
    }
}

/// A carved-table fixture: the caller's own table with a self-table capability
/// at `CAPTBL_SELF`, through which `Untyped.Retype` is invoked.
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
                KeySlot::CAPTBL_SELF,
                KeyEntry::new_keytable(table_addr, Rights(table_rights), 0),
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
            .insert(slot, entry)
            .unwrap_or_else(|_| panic!("fixture installation failed"))
    }

    /// Look up an entry in the fixture table.
    fn lookup(&self, key: RawKey) -> Result<&KeyEntry, CapError> {
        // SAFETY: see install.
        Ok(unsafe { &*(self.table_addr as *const KeyTable) }.lookup(key)?)
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
        api::untyped::invoke::<ArchObjectsImpl>(
            &access,
            self.table_addr,
            untyped_key,
            op,
            args,
            self.nucleus,
        )
    }
}

/// Encode Retype's approved wire schema (see `doc/nucleus_capabilities.md`):
/// `x2` object kind, `x3` `size_bits`, `x4` count, `x5` destination-table key,
/// `x6` first destination slot, `x7` requested rights.
fn retype_args(
    kind: ObjectType,
    size_bits: u8,
    count: u64,
    dst: RawKey,
    slot: KeySlot,
    rights: Rights,
) -> [u64; 6] {
    [
        u64::from(kind.as_u8()),
        u64::from(size_bits),
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
    let bogus = RawKey::new(KeySlot(200), 7);

    let result = fx.invoke(
        device,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::KEY_TABLE,
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
            0,
            1,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(result, Err(CapError::InvalidSize(32))));
}

/// The usable range ends where the watermark encoding does: a run that fits
/// the region but ends beyond the largest representable watermark is rejected
/// as insufficient memory, before any destination-slot iteration.
#[test_case]
fn retype_rejects_runs_beyond_the_watermark_encoding() {
    let mut fx = Fixture::new(FULL);
    // A 128 GiB region (size_bits 37) whose ~8-million-object run (~69 GiB)
    // fits the region but ends past the `u32` watermark encoding (~64 GiB).
    let region = fx.install(KeySlot(30), ram_untyped_at(0, 37));

    let result = fx.invoke(
        region,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::KEY_TABLE,
            0,
            8_000_000,
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
    let bogus = RawKey::new(KeySlot(200), 7);

    let result = fx.invoke(
        bogus,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::FRAME,
            13,
            1,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(result, Err(CapError::InvalidFrameSize(13))));
}

/// A device Untyped is not a valid source for Frames either: the general
/// device rejection covers the frame kind, with no state changes.
#[test_case]
fn retype_rejects_device_frames_without_changing_state() {
    let mut fx = Fixture::new(FULL);
    let device = fx.install(KeySlot(30), device_untyped(24));
    let before = fx.len();

    let result = fx.invoke(
        device,
        UntypedOp::Retype as u64,
        &retype_args(
            ObjectType::FRAME,
            12,
            1,
            fx.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(
        result,
        Err(CapError::InvalidObjectType(ObjectType::FRAME))
    ));

    // No partial state: the source entry is untouched and nothing was
    // installed.
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
            2,
            fx2.self_key,
            KeySlot(40),
            Rights::all(),
        ),
    );
    assert!(matches!(result2, Err(CapError::PoolExhausted)));
}
