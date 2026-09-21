//! Boot-time construction of the initial kernel state.
//!
//! This is one-time boot code, owned by Kickstart, not by the inert nucleus.
//! It carves the initial [`Nucleus`] and its pool backings from the boot
//! Untyped's unused watermark range, writes the `Nucleus` into carved memory,
//! and returns its address. Kickstart then records that address in the
//! nucleus's `NUCLEUS_ANCHOR` (resolved by symbol lookup) before the nucleus
//! runs.
//!
//! Carving here is one-shot boot code: a failed carve aborts the boot, so a
//! partially advanced watermark is not recovered. Runtime Retype (which must be
//! transactional) is separate; see `nucleus::api::untyped`.

use {
    libaddress::{PhysAddr, align},
    libobject::{CapError, domain::DomainId},
    nucleus::{
        api::key_entry::RegionPayload,
        objects::{
            ArchObjects, EventCount, KeyTable, Notification, Nucleus, NucleusObject, ObjectPool,
            PendingPool, Scheduler, Thread, arch::ArchPools, domain::DcbPages,
            nucleus::NucleusPools,
        },
    },
};

/// Alignment for every boot carve. Page alignment guarantees the 16-byte
/// watermark granularity and object alignment for all pool types.
const CARVE_ALIGN: u64 = 4096;

/// Carve `size` bytes from the boot Untyped's unused watermark range.
///
/// Advances the watermark only on success, so a failed carve leaves the
/// Untyped unchanged. Returns the physical address of the carved region.
fn carve_region(boot: &mut RegionPayload, size: usize) -> Result<PhysAddr, CapError> {
    let wm = boot.watermark_bytes();
    let aligned = align::align_up(u64::try_from(wm).unwrap(), CARVE_ALIGN);
    let end = aligned + u64::try_from(size).unwrap();
    if end > u64::try_from(boot.size()).unwrap() {
        return Err(CapError::InsufficientMemory);
    }
    let paddr = PhysAddr::new(boot.paddr + aligned);
    boot.set_watermark_bytes(usize::try_from(end).unwrap());
    Ok(paddr)
}

/// Carve a whole pool of `capacity` objects of type `T` from the boot Untyped.
///
/// The pool's object backing lives in the carved range; its authoritative
/// metadata (`SlotMeta` etc.) lives in the returned [`ObjectPool`] value, which
/// the caller places in the carved [`Nucleus`].
pub fn carve_pool<T: NucleusObject>(
    boot: &mut RegionPayload,
    capacity: usize,
) -> Result<ObjectPool<T>, CapError> {
    if capacity > ObjectPool::<T>::MAX_SLOTS {
        return Err(CapError::InvalidSize(capacity));
    }
    let size = capacity * core::mem::size_of::<T>();
    let paddr = carve_region(boot, size)?;
    let ptr = paddr.user_to_kernel().as_mut_ptr::<u8>();
    // SAFETY: carve_region reserved `size` bytes from the boot Untyped's unused
    // watermark range, and the direct map makes them kernel-dereferenceable.
    Ok(unsafe { ObjectPool::new(ptr, size) })
}

/// Capacities for the initial pool extents carved at boot.
pub struct PoolCapacities {
    /// Number of Thread slots in the initial Thread pool.
    pub threads: usize,
    /// Number of `AddressSpace` slots in the initial architecture address-space
    /// pool. Address spaces are the protection/mapping-context objects;
    /// boot-carved, not Retype-creatable yet.
    pub address_spaces: usize,
    /// Number of Notification slots in the initial Notification pool.
    /// Notifications are pure kernel synchronization state allocated by
    /// `Untyped.Retype` (allowlisted 2026-09-16); the pool backing is
    /// charged here, at bootstrap.
    pub notifications: usize,
    /// Number of `EventCount` slots in the initial `EventCount` pool.
    /// `EventCount`s are pure kernel synchronization state allocated by
    /// `Untyped.Retype` (allowlisted 2026-09-18); the pool backing is
    /// charged here, at bootstrap.
    pub event_counts: usize,
    /// Number of page-table metadata slots in the initial architecture
    /// page-table pool. The pool holds only kernel metadata (carve address,
    /// installation record); the 4 KiB tables themselves are Retype carves
    /// charged to the invoking Untyped.
    pub page_tables: usize,
    /// Number of ASID-pool slots in the initial architecture ASID-pool pool.
    /// ASID pools are boot-provided capability-protected resources (not
    /// Retype-creatable); one slot backs the boot pool.
    pub asid_pools: usize,
}

/// Build the initial [`Nucleus`] in memory carved from the boot Untyped.
///
/// Carves the `Nucleus` struct region, the Thread and `AddressSpace` pool
/// backings, and the boot Thread's `KeyTable` region from `boot`'s unused
/// watermark range, constructs the pools, initializes the boot `KeyTable`
/// kernel-privately, writes the `Nucleus` into the carved region, and returns
/// its address together with the boot `KeyTable`'s kernel address. The caller
/// (boot code) records the nucleus address as the anchor the inert nucleus
/// reads on entry.
pub fn build_initial_nucleus<A: ArchObjects>(
    boot: &mut RegionPayload,
    capacities: &PoolCapacities,
) -> Result<(*mut Nucleus<A>, u64), CapError> {
    if capacities.threads > ObjectPool::<Thread>::MAX_SLOTS {
        return Err(CapError::InvalidSize(capacities.threads));
    }
    if capacities.address_spaces > ObjectPool::<A::AddressSpace>::MAX_SLOTS {
        return Err(CapError::InvalidSize(capacities.address_spaces));
    }
    if capacities.notifications > ObjectPool::<Notification>::MAX_SLOTS {
        return Err(CapError::InvalidSize(capacities.notifications));
    }
    if capacities.event_counts > ObjectPool::<EventCount>::MAX_SLOTS {
        return Err(CapError::InvalidSize(capacities.event_counts));
    }
    if capacities.page_tables > ObjectPool::<A::PageTable>::MAX_SLOTS {
        return Err(CapError::InvalidSize(capacities.page_tables));
    }
    if capacities.asid_pools > ObjectPool::<A::ASIDPool>::MAX_SLOTS {
        return Err(CapError::InvalidSize(capacities.asid_pools));
    }

    let nucleus_paddr = carve_region(boot, core::mem::size_of::<Nucleus<A>>())?;
    let nucleus_ptr = nucleus_paddr.user_to_kernel().as_mut_ptr::<Nucleus<A>>();

    let threads = carve_pool::<Thread>(boot, capacities.threads)?;
    let address_spaces = carve_pool::<A::AddressSpace>(boot, capacities.address_spaces)?;
    let notifications = carve_pool::<Notification>(boot, capacities.notifications)?;
    let event_counts = carve_pool::<EventCount>(boot, capacities.event_counts)?;
    let page_tables = carve_pool::<A::PageTable>(boot, capacities.page_tables)?;
    let asid_pools = carve_pool::<A::ASIDPool>(boot, capacities.asid_pools)?;

    // Carve the boot Thread's KeyTable region and initialize it kernel-privately
    // (the same unused-watermark allocation Retype performs at runtime).
    let keytable_paddr = carve_region(boot, core::mem::size_of::<KeyTable>())?;
    let keytable_ptr = keytable_paddr.user_to_kernel().as_mut_ptr::<KeyTable>();
    // SAFETY: carve_region reserved the bytes from the boot Untyped's unused
    // watermark range, and the direct map makes them kernel-dereferenceable.
    unsafe {
        keytable_ptr.write(KeyTable::new(DomainId(0)));
    }

    let nucleus = Nucleus::<A> {
        current_thread: None,
        dcb_pages: DcbPages::new(),
        pending: PendingPool::new(),
        scheduler: Scheduler::new(),
        pools: NucleusPools::<A> {
            threads,
            notifications,
            event_counts,
            // SAFETY: the architecture pool backings were carved above from
            // the boot Untyped's unused watermark range and are exclusively
            // owned.
            arch: unsafe { ArchPools::new(page_tables, address_spaces, asid_pools) },
        },
    };
    // SAFETY: nucleus_ptr points to the freshly carved, exclusively-owned region.
    unsafe {
        nucleus_ptr.write(nucleus);
    }
    Ok((nucleus_ptr, keytable_ptr as u64))
}
