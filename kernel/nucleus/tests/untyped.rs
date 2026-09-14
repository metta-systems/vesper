//! Nucleus `Untyped` handler tests: the Retype transaction's rejection paths.
//!
//! The embedded test binary runs with the MMU off, so these tests exercise
//! only paths that reject before the kernel-private carve write (which goes
//! through the physical direct map); the successful carve is covered by the
//! kickstart boot test through the real SVC path.

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
    libobject::{CapError, KeySlot, ObjectType, RawKey, Rights, UntypedOp, domain::DomainId},
    // `Nucleus` is not used directly here: binding it at the crate root lets
    // the included production tree resolve `crate::Nucleus`
    // (objects/arch/aarch64_objects.rs, as the nucleus lib re-exports it).
    objects::{KeyTable, Nucleus, access::Access},
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

/// A carved-table fixture: the caller's own table with a self-table capability
/// at `CAPTBL_SELF`, through which `Untyped.Retype` is invoked.
struct Fixture {
    table_addr: u64,
    self_key: RawKey,
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
        &self,
        untyped_key: RawKey,
        op: u64,
        args: &[u64; 6],
    ) -> Result<(u64, u64), CapError> {
        // SAFETY: test-only; no overlapping access context.
        let access = unsafe { Access::new() };
        api::untyped::invoke(&access, self.table_addr, untyped_key, op, args)
    }
}

/// Encode Retype's approved wire schema (see `doc/nucleus_capabilities.md`):
/// `x2` object kind, `x3` `size_bits`, `x4` count, `x5` destination-table key,
/// `x6` first destination slot, `x7` requested rights.
fn retype_args(count: u64, dst: RawKey, slot: KeySlot, rights: Rights) -> [u64; 6] {
    [
        u64::from(ObjectType::KEY_TABLE.as_u8()),
        0,
        count,
        dst.to_wire(),
        u64::from(slot.0),
        u64::from(rights.bits()),
    ]
}

/// A mock Untyped over normal RAM at 768 MiB (inside QEMU's 1 GiB, clear of
/// the fixture backing); never written by these rejection-path tests.
fn ram_untyped(size_bits: u8) -> KeyEntry {
    KeyEntry::new_untyped(0x3000_0000, size_bits, false, Rights::all())
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
        &retype_args(1, fx.self_key, KeySlot(40), Rights::all()),
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
        &retype_args(1, fx.self_key, KeySlot(40), Rights::all()),
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
        &retype_args(1, fx.self_key, KeySlot(40), Rights::all()),
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
        &retype_args(1, bogus, KeySlot(40), Rights::all()),
    );
    assert!(matches!(
        result,
        Err(CapError::InvalidObjectType(ObjectType::KEY_TABLE))
    ));
}
