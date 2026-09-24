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
    crate::{boot_info::BOOT_INFO, embed::NUCLEUS_SET_ANCHOR_VIRT, print_my_sp},
    libaddress::{PhysAddr, align},
    liblocking::interface::Mutex,
    libobject::{CapError, KeySlot, ObjectType, RawKey, Rights, domain::DomainId},
    nucleus::{
        api::key_entry::{KeyEntry, RegionPayload},
        objects::{
            ArchObjects, ArchObjectsImpl, EventCount, ExecutionContext, KeyTable, Notification,
            Nucleus, NucleusObject, ObjectPool, PendingPool, Scheduler, Thread,
            access::{ObjectId, PoolTag},
            arch::ArchPools,
            domain::DcbPages,
            nucleus::NucleusPools,
        },
    },
};

/// Size (as log2) of the boot Untyped region carved for the initial kernel
/// state: 16 MiB, sized for the nucleus, its pools, the boot `KeyTable`, and
/// the runtime Retype carves the boot continuation performs.
const BOOT_UNTYPED_SIZE_BITS: u8 = 24;

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

/// The boot Thread's `KeyTable` capacity exponent: 256 entries (the historical
/// fixed size; the well-known bootstrap slots and the boot test's Retype
/// destinations fit comfortably).
pub const BOOT_TABLE_SIZE_BITS: u8 = 8;

/// The boot table's guard (guarded key-space package, selected 2026-09-23):
/// Kickstart picks a nonzero value so every hand-constructed boot key
/// exercises the guard machinery. It fits the 24 guard bits of a 256-entry
/// table's table-relative address.
pub const BOOT_TABLE_GUARD: u32 = 0xC0F_FEE;

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
    // (the same unused-watermark allocation Retype performs at runtime). The
    // carve covers the variable-size layout: header plus 2^size_bits entries
    // and incarnation counters.
    let keytable_paddr = carve_region(boot, KeyTable::carve_size(BOOT_TABLE_SIZE_BITS))?;
    let keytable_ptr = keytable_paddr.user_to_kernel().as_mut_ptr::<u8>();
    // SAFETY: carve_region reserved the full carve size from the boot
    // Untyped's unused watermark range at the table's 32-byte alignment, and
    // the direct map makes the bytes kernel-dereferenceable.
    unsafe {
        KeyTable::initialize(keytable_ptr, DomainId(0), BOOT_TABLE_SIZE_BITS);
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

/// The boot-time kernel state the post-boot continuation continues from.
///
/// Shared by the real kickstart kernel and the kicktest e2e boot-test kernel:
/// everything here is real boot state (no test fixtures), sized by the
/// [`PoolCapacities`] the caller passes to [`bootstrap_nucleus`].
pub struct BootState {
    /// The boot-carved, live initial [`Nucleus`]. Exclusively boot-owned; the
    /// inert nucleus reads it through the anchor recorded at bootstrap.
    pub nucleus: &'static mut Nucleus<ArchObjectsImpl>,
    /// Kernel-window address of the boot Thread's carved, variable-size
    /// `KeyTable`.
    pub keytable_addr: u64,
    /// The boot `AddressSpace`'s pool identity (the protection/mapping
    /// context the boot Thread executes in).
    pub boot_as_id: ObjectId,
    /// Key of the boot Thread's self-table capability (slot `SELF_KEYTABLE`).
    pub self_table_key: RawKey,
    /// Key of the boot Untyped grant (slot `BOOT_UNTYPED`).
    pub boot_untyped_key: RawKey,
    /// Key of the debug console grant (slot `DEBUG_CONSOLE`), `debug_kernel`
    /// only.
    #[cfg(feature = "debug_kernel")]
    pub debug_console_key: RawKey,
}

/// Bootstrap the initial kernel state and return the boot continuation's
/// starting point.
///
/// Carves the boot Untyped, builds the initial [`Nucleus`] and its pools,
/// allocates the boot `AddressSpace` and boot Thread, installs the bootstrap
/// grants (self-table, boot Untyped, self-AddressSpace, boot Thread, boot ASID
/// pool, and the debug console under `debug_kernel`), and records the nucleus
/// anchor the inert nucleus reads on syscall entry.
///
/// The `capacities` size the carved pools for the caller's continuation: the
/// real kickstart passes its own needs; kicktest passes the e2e suite's
/// fixture extents (a Bounce Thread, fixture `AddressSpace`s, and the
/// mapping-chain page-table pool).
pub fn bootstrap_nucleus(capacities: &PoolCapacities) -> BootState {
    // Allocate a power-of-2 boot region for the boot Untyped.
    let boot_region = BOOT_INFO
        .lock(|bi| bi.alloc_region(usize::from(BOOT_UNTYPED_SIZE_BITS), "Boot Untyped"))
        .expect("no free region for the boot Untyped");

    // Create the boot Untyped capability over that region.
    let mut boot_untyped = KeyEntry::new_untyped(
        boot_region.as_u64(),
        BOOT_UNTYPED_SIZE_BITS,
        false,
        Rights::all(),
    );

    // Carve the initial Nucleus + pools from the boot Untyped's watermark.
    let Ok(boot_payload) = boot_untyped.as_region_mut() else {
        panic!("boot Untyped is not a region")
    };
    let Ok((nucleus_ptr, keytable_addr)) =
        build_initial_nucleus::<ArchObjectsImpl>(boot_payload, capacities)
    else {
        panic!("failed to build the initial nucleus")
    };

    // SAFETY: nucleus_ptr points to the freshly carved, exclusively-owned region.
    let nucleus: &'static mut Nucleus<ArchObjectsImpl> = unsafe { &mut *nucleus_ptr };
    // The boot Thread is the first (index 0) allocation; make it current.
    nucleus.current_thread = Some(0);

    // Allocate the boot AddressSpace (the protection/mapping context) and the
    // boot Thread that executes in it; the Thread's KeyTable was carved and
    // initialized kernel-privately by build_initial_nucleus.
    let boot_as_id = nucleus
        .pools
        .arch
        .address_spaces
        .allocate(ArchObjectsImpl::new_address_space())
        .expect("no boot AddressSpace slot")
        .0;
    let boot_thread_id = nucleus
        .pools
        .threads
        .allocate(Thread {
            keytable_addr,
            address_space: boot_as_id,
            context: ExecutionContext::Running,
        })
        .expect("no boot Thread slot")
        .0;

    // Install the boot Thread's self-table capability, its AddressSpace
    // (the bootstrap-era mapping context for `PageTable.Map`/`Frame.Map`),
    // the boot Thread itself, and the boot Untyped as the first grants.
    // SAFETY: keytable_addr names the freshly carved, live boot KeyTable.
    let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
    let self_table_key = boot_table
        .insert(
            KeySlot::SELF_KEYTABLE,
            KeyEntry::new_keytable(
                keytable_addr,
                BOOT_TABLE_GUARD,
                BOOT_TABLE_SIZE_BITS,
                Rights::all(),
                0,
            ),
            BOOT_TABLE_GUARD,
        )
        .unwrap_or_else(|failure| {
            panic!("boot self-table install failed: {:?}", failure.error.code())
        });
    let boot_untyped_key = boot_table
        .insert(KeySlot::BOOT_UNTYPED, boot_untyped, BOOT_TABLE_GUARD)
        .unwrap_or_else(|failure| {
            panic!("boot Untyped install failed: {:?}", failure.error.code())
        });
    let _boot_as_key = boot_table
        .insert(
            KeySlot::SELF_ADDRESS_SPACE,
            KeyEntry::new::<<ArchObjectsImpl as ArchObjects>::AddressSpace>(
                boot_as_id,
                Rights::all(),
                0,
            ),
            BOOT_TABLE_GUARD,
        )
        .unwrap_or_else(|failure| {
            panic!(
                "boot AddressSpace install failed: {:?}",
                failure.error.code()
            )
        });
    let _boot_thread_key = boot_table
        .insert(
            KeySlot(50),
            KeyEntry::new::<Thread>(boot_thread_id, Rights::all(), 0),
            BOOT_TABLE_GUARD,
        )
        .unwrap_or_else(|failure| panic!("boot Thread install failed: {:?}", failure.error.code()));

    // Provision the boot ASID pool (seL4-style, selected 2026-09-15): ASIDs
    // are a hardware namespace, not memory-backed, so the pool is carved and
    // initialized kernel-privately here rather than Retype-created. Its
    // capability is the authoritative grant the bootstrap builder assigns
    // hardware translation contexts from.
    let boot_asid_pool_id = nucleus
        .pools
        .arch
        .asid_pools
        .allocate(ArchObjectsImpl::new_asid_pool())
        .expect("no boot ASID-pool slot")
        .0;
    let _boot_asid_pool_key = boot_table
        .insert(
            KeySlot::BOOT_ASID_POOL,
            KeyEntry::new::<<ArchObjectsImpl as ArchObjects>::ASIDPool>(
                boot_asid_pool_id,
                Rights::all(),
                0,
            ),
            BOOT_TABLE_GUARD,
        )
        .unwrap_or_else(|failure| {
            panic!("boot ASID-pool install failed: {:?}", failure.error.code())
        });

    // Install the debug console grant (debug_kernel), the boot Thread's
    // console authority.
    #[cfg(feature = "debug_kernel")]
    let debug_console_key = boot_table
        .insert(
            KeySlot::DEBUG_CONSOLE,
            KeyEntry::from_id(
                ObjectType::DEBUG_CONSOLE,
                ObjectId {
                    pool: PoolTag::Region,
                    index: 0,
                    generation: 0,
                },
                Rights::all(),
                0,
            ),
            BOOT_TABLE_GUARD,
        )
        .unwrap_or_else(|failure| {
            panic!("debug console install failed: {:?}", failure.error.code())
        });

    // Record the carved Nucleus address for the inert nucleus.
    // SAFETY: The paired nucleus image is loaded and mapped; the setter is a
    // boot-only one-shot write before any syscall.
    unsafe {
        let setter = core::mem::transmute::<u64, unsafe extern "C" fn(*mut Nucleus<ArchObjectsImpl>)>(
            NUCLEUS_SET_ANCHOR_VIRT,
        );
        setter(nucleus_ptr);
    }
    print_my_sp();

    BootState {
        nucleus,
        keytable_addr,
        boot_as_id,
        self_table_key,
        boot_untyped_key,
        #[cfg(feature = "debug_kernel")]
        debug_console_key,
    }
}
