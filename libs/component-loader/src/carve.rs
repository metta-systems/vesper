//! Kernel-private carves from the boot Untyped.
//!
//! Boot and trusted test code create the objects they set up before any
//! component runs — the boot and fixture Threads, the idle Thread, the boot
//! and trusted fixture `AddressSpace`s, the root Scheduler — directly in the
//! boot Untyped's unused watermark range, through the same reserve-then-advance
//! path as `Untyped.Retype`. The watermark advances past each carve, so
//! nothing else is ever carved there.

use {
    crate::keys::BOOT_TABLE_GUARD,
    libaddress::PhysAddr,
    libobject::RawKey,
    nucleus::{
        api::key_entry::KeyEntry,
        objects::{
            KeyTable,
            carved::{Carved, CarvedId, CarvedObject},
        },
    },
};

/// Minimum alignment of a carve: the 16-byte watermark granularity.
const CARVE_ALIGN_MIN: usize = 16;

/// Carve `object` from the boot Untyped (the capability `boot_untyped_key` in
/// the boot `KeyTable` at `keytable_addr`) and return its identity.
pub fn carve_object<T: CarvedObject>(
    keytable_addr: u64,
    boot_untyped_key: RawKey,
    object: T,
) -> CarvedId {
    let address = carve_bytes(
        keytable_addr,
        boot_untyped_key,
        Carved::<T>::SIZE,
        Carved::<T>::ALIGN,
    );
    // SAFETY: `carve_bytes` reserved the bytes, aligned and sized for the
    // record, exclusively for this object.
    unsafe { Carved::<T>::initialize(address, object) }
}

/// Reserve `size` bytes aligned to `align` from the boot Untyped and return
/// their kernel address. The watermark advances past them, so nothing else
/// is ever carved there; the caller initializes them.
pub fn carve_bytes(
    keytable_addr: u64,
    boot_untyped_key: RawKey,
    size: usize,
    align: usize,
) -> *mut u8 {
    // SAFETY: the boot table is the live carved boot KeyTable; this borrow
    // ends before any capability invocation.
    let table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
    let region = *table
        .lookup(boot_untyped_key, BOOT_TABLE_GUARD)
        .and_then(KeyEntry::as_untyped)
        .unwrap_or_else(|error| panic!("boot Untyped lookup failed: {:?}", error.code()));
    let align = align.max(CARVE_ALIGN_MIN);
    let size = size.next_multiple_of(align);
    let reservation = region
        .reserve(u64::try_from(align).unwrap(), u64::try_from(size).unwrap())
        .unwrap_or_else(|error| panic!("boot Untyped exhausted: {:?}", error.code()));
    let paddr = region.paddr + u64::try_from(reservation.start).unwrap();
    table
        .advance_untyped_watermark(boot_untyped_key, BOOT_TABLE_GUARD, reservation.end)
        .unwrap_or_else(|error| panic!("boot Untyped watermark failed: {:?}", error.code()));
    PhysAddr::new(paddr).user_to_kernel().as_mut_ptr::<u8>()
}
