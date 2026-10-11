//! Boot-time construction of the initial kernel state.
//!
//! This is one-time boot code, owned by Kickstart, not by the inert nucleus.
//! It carves the initial [`Nucleus`] and its object pools from the boot
//! Untyped's unused watermark range — each pool one variable-size carve of
//! per-slot metadata and object storage, sized by the bootstrap capacity —
//! writes the `Nucleus` into carved memory, and returns its address. Kickstart
//! then records that address in the nucleus's `NUCLEUS_ANCHOR` (resolved by
//! symbol lookup) before the nucleus runs.
//!
//! Carving here is one-shot boot code: a failed carve aborts the boot, so a
//! partially advanced watermark is not recovered. Runtime Retype (which must be
//! transactional) is separate; see `nucleus::api::untyped`.

use {
    crate::{
        boot_info::{BOOT_INFO, DeviceUntypedRange},
        embed::{NUCLEUS_SET_ANCHOR_VIRT, NUCLEUS_SET_PLATFORM_VIRT},
        print_my_sp, privileged,
    },
    core::cell::UnsafeCell,
    libaddress::PhysAddr,
    libexception::arch::aarch64::SavedContext,
    liblocking::interface::Mutex,
    libmapping::MemAttributes,
    libobject::{
        CapError, KeySlot, ObjectType, RawKey, Rights,
        domain::DomainId,
        time::{Budget, HIGHEST_PRIORITY, INFINITE_BUDGET},
    },
    libqemu::semihosting as semi,
    nucleus::{
        api::key_entry::{KeyEntry, RegionPayload},
        objects::{
            ArchObjects, ArchObjectsImpl, ExecutionContext, InvocationStack, KeyTable, Nucleus,
            ObjectPool, PendingPool, RunQueue, Scheduler, Thread, Time,
            access::{ObjectId, PoolTag},
            arch::ArchPools,
            carved::{Carved, CarvedId, CarvedObject},
            domain::DcbPages,
            irq::{IrqLine, IrqTable},
            nucleus::NucleusPools,
            thread::ThreadId,
        },
    },
};

/// Size (as log2) of the boot Untyped region carved for the initial kernel
/// state: 16 MiB, sized for the nucleus, its pools, the boot `KeyTable`, and
/// the runtime Retype carves the boot continuation performs.
const BOOT_UNTYPED_SIZE_BITS: u8 = 24;

/// Budget of the boot Time at `KeySlot::BOOT_TIME`: infinite with the
/// `infinite-boot-time` feature, otherwise a large finite amount (one hour of
/// CPU time). Nothing replenishes a finite boot budget yet.
#[cfg(feature = "infinite-boot-time")]
pub const BOOT_TIME_BUDGET_NS: Budget = INFINITE_BUDGET;
/// Budget of the boot Time at `KeySlot::BOOT_TIME`: infinite with the
/// `infinite-boot-time` feature, otherwise a large finite amount (one hour of
/// CPU time). Nothing replenishes a finite boot budget yet.
#[cfg(not(feature = "infinite-boot-time"))]
pub const BOOT_TIME_BUDGET_NS: Budget = 3_600 * 1_000_000_000;

/// Alignment for every boot carve. Page alignment guarantees the 16-byte
/// watermark granularity and object alignment for all pool types.
const CARVE_ALIGN: u64 = 4096;

/// Minimum alignment of a carved object: the 16-byte watermark granularity.
const CARVE_ALIGN_MIN: usize = 16;

/// Carve `size` bytes from the boot Untyped's unused watermark range.
///
/// Advances the watermark only on success, so a failed carve leaves the
/// Untyped unchanged. Returns the physical address of the carved region.
fn carve_region(boot: &mut RegionPayload, size: usize) -> Result<PhysAddr, CapError> {
    let reservation = boot.reserve(CARVE_ALIGN, u64::try_from(size).unwrap())?;
    let paddr = PhysAddr::new(boot.paddr + u64::try_from(reservation.start).unwrap());
    boot.set_watermark_bytes(reservation.end);
    Ok(paddr)
}

/// Capacities for the initial pool extents carved at boot. Each pool is one
/// variable-size carve from the boot Untyped — the per-slot metadata and the
/// object storage together, sized solely by the capacity given here.
pub struct PoolCapacities {
    /// Number of `AddressSpace` slots in the initial architecture address-space
    /// pool. Address spaces are the protection/mapping-context objects;
    /// boot-carved, not Retype-creatable yet.
    pub address_spaces: usize,
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
    let nucleus_paddr = carve_region(boot, core::mem::size_of::<Nucleus<A>>())?;
    let nucleus_ptr = nucleus_paddr.user_to_kernel().as_mut_ptr::<Nucleus<A>>();

    let address_spaces = ObjectPool::<A::AddressSpace>::carve(boot, capacities.address_spaces)?;
    let page_tables = ObjectPool::<A::PageTable>::carve(boot, capacities.page_tables)?;
    let asid_pools = ObjectPool::<A::ASIDPool>::carve(boot, capacities.asid_pools)?;

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
        run_queue: RunQueue::new(),
        idle_thread: None,
        irq_table: IrqTable::empty(),
        ticks: 0,
        pools: NucleusPools::<A> {
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

/// Accounted linked init image and retained low `EL1t` execution stack.
///
/// These occupied boot extents are not allocation sources: Retype would zero
/// live code/data/stack. The bootstrap builder may issue content-preserving
/// Frame authority over their pages instead. They remain reserved in `BOOT_INFO`
/// and must never be reclaimed while either execution or a mapping survives.
pub struct RetainedInitMemory {
    image: (u64, u64),
    stack: (u64, u64),
}

impl RetainedInitMemory {
    /// Page-aligned physical/identity-virtual bounds of the entire linked image,
    /// including text, rodata (and embedded dependencies), data and BSS.
    pub fn image(&self) -> (u64, u64) {
        self.image
    }

    /// Page-aligned bounds of the reserved low `SP_EL0` execution stack.
    pub fn stack(&self) -> (u64, u64) {
        self.stack
    }

    /// Issue one bootstrap-origin Frame grant without initializing its contents.
    ///
    /// The one-time builder issues each retained page once, then uses checked
    /// `CopyDerive` for peers. This is boot authority, not a runtime creation ABI;
    /// it neither creates an `Untyped` over occupied bytes nor advances an
    /// unrelated allocation watermark. Mapping and derivation remain separate.
    pub fn grant_page(
        &self,
        table: &mut KeyTable,
        slot: KeySlot,
        guard: u32,
        paddr: u64,
    ) -> RawKey {
        assert_eq!(paddr & 4095, 0, "retained Frame must be page-aligned");
        let end = paddr
            .checked_add(4096)
            .expect("retained Frame extent overflow");
        assert!(
            [self.image, self.stack]
                .iter()
                .any(|&(start, limit)| start <= paddr && end <= limit),
            "Frame grant must lie wholly inside retained init backing"
        );
        table
            .insert(
                slot,
                KeyEntry::new_frame(paddr, 12, false, Rights::all()),
                guard,
            )
            .unwrap_or_else(|failure| {
                panic!("retained Frame grant failed: {:?}", failure.error.code())
            })
    }
}

/// Discover retained init backing from linker bounds and verify boot accounting.
///
/// The Kickstart overlay already reserves the linked image; the execution
/// stack has its own occupied record. No allocator or Retype may allocate
/// these extents. The overlay's historical droppable hint is not a reclamation
/// protocol: this continuation retains its image under accepted-leak lifetime.
pub fn retained_init_memory() -> RetainedInitMemory {
    unsafe extern "C" {
        static __INIT_START: UnsafeCell<()>;
        static __INIT_END: UnsafeCell<()>;
        static __STACK_BOTTOM: UnsafeCell<()>;
        static __STACK_TOP: UnsafeCell<()>;
    }
    // SAFETY: only take the addresses of linker-defined bounds, without reading
    // their contents. The linked init image uses physical identity addresses.
    let (image, stack) = unsafe {
        (
            (__INIT_START.get() as u64, __INIT_END.get() as u64),
            (__STACK_BOTTOM.get() as u64, __STACK_TOP.get() as u64),
        )
    };
    for (start, end) in [image, stack] {
        assert!(start < end);
        assert_eq!((start | end) & 4095, 0);
    }
    assert!(
        stack.1 <= image.0,
        "retained image and stack must not overlap"
    );
    BOOT_INFO.lock(|bi| {
        for ((start, end), name) in [(image, "🥾 Kickstart"), (stack, "Init execution stack")] {
            assert!(
                bi.used_regions().any(|region| {
                    region.name == name
                        && region.start_inclusive.as_u64() <= start
                        && end <= region.end_exclusive.as_u64()
                }),
                "retained {name} extent is not covered by occupied boot accounting"
            );
        }
        assert!(bi.free_regions().all(|region| {
            [image, stack].iter().all(|&(start, end)| {
                region.end_exclusive.as_u64() <= start || end <= region.start_inclusive.as_u64()
            })
        }));
    });
    RetainedInitMemory { image, stack }
}

/// The boot-time kernel state the post-boot continuation continues from.
///
/// Shared by the real kickstart kernel and the `kernel/tests` e2e kernels:
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
    /// Key of the boot Time grant (slot `BOOT_TIME`).
    pub boot_time_key: RawKey,
    /// Key of the root Scheduler grant (slot `ROOT_SCHEDULER`).
    pub root_scheduler_key: RawKey,
    /// Key of the `IRQControl` grant (slot `IRQ_CONTROL`).
    pub irq_control_key: RawKey,
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
/// pool, boot Time, and the debug console under `debug_kernel`), and records the nucleus
/// anchor the inert nucleus reads on syscall entry.
///
/// The `capacities` size the carved pools for the caller's continuation: the
/// real kickstart passes its own needs; each e2e test kernel passes its own
/// fixture extents (fixture Threads and `AddressSpace`s, page tables).
/// Execution stack of the idle Thread. It only runs `wfi`; interrupts are
/// taken on the shared `SP_EL1` trap stack.
#[repr(C, align(16))]
struct IdleStack(UnsafeCell<[u8; IDLE_STACK_BYTES]>);

const IDLE_STACK_BYTES: usize = 1024;

// SAFETY: only the idle Thread uses the stack, through its SP.
unsafe impl Sync for IdleStack {}

static IDLE_STACK: IdleStack = IdleStack(UnsafeCell::new([0; IDLE_STACK_BYTES]));

/// The idle Thread's body: wait for the next interrupt, forever.
extern "C" fn idle_loop() -> ! {
    loop {
        aarch64_cpu::asm::wfi();
    }
}

/// Carve `object` from the boot Untyped (the capability `boot_untyped_key` in
/// the boot `KeyTable` at `keytable_addr`), kernel-privately, through the
/// same reserve-then-advance path as `Untyped.Retype`. Boot and trusted test
/// code use it for objects they set up before any component runs.
pub fn carve_boot_object<T: CarvedObject>(
    keytable_addr: u64,
    boot_untyped_key: RawKey,
    object: T,
) -> CarvedId {
    let address = carve_boot_bytes(
        keytable_addr,
        boot_untyped_key,
        Carved::<T>::SIZE,
        Carved::<T>::ALIGN,
    );
    // SAFETY: `carve_boot_bytes` reserved the bytes, aligned and sized for
    // the record, exclusively for this object.
    unsafe { Carved::<T>::initialize(address, object) }
}

/// Reserve `size` bytes aligned to `align` from the boot Untyped and return
/// their kernel address. The watermark advances past them, so nothing else
/// is ever carved there; the caller initializes them.
fn carve_boot_bytes(
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

/// Carve the records of the interrupt-controller component's `line_count`
/// lines from the boot Untyped and give them to the nucleus.
#[expect(
    clippy::cast_ptr_alignment,
    reason = "`carve_boot_bytes` aligns the carve to `IrqLine`"
)]
fn carve_irq_table(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    keytable_addr: u64,
    boot_untyped_key: RawKey,
    line_count: u32,
) {
    let size = core::mem::size_of::<IrqLine>() * line_count as usize;
    let lines = carve_boot_bytes(
        keytable_addr,
        boot_untyped_key,
        size,
        core::mem::align_of::<IrqLine>(),
    );
    // SAFETY: the bytes were just carved, aligned and sized for `line_count`
    // records, and nothing else uses them.
    nucleus.irq_table = unsafe { IrqTable::initialize(lines.cast::<IrqLine>(), line_count) };
}

/// Create the idle Thread in `address_space` (which must map the boot image):
/// an interruptible `EL1t` Thread the nucleus selects when nothing else is
/// runnable and never queues. Its memory comes from the boot Untyped.
pub fn spawn_idle(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    keytable_addr: u64,
    boot_untyped_key: RawKey,
    address_space: ObjectId,
) -> ThreadId {
    assert!(
        nucleus.idle_thread.is_none(),
        "the idle Thread already exists"
    );
    let stack_top = IDLE_STACK.0.get() as u64 + IDLE_STACK_BYTES as u64;
    let idle = carve_boot_object(
        keytable_addr,
        boot_untyped_key,
        Thread::new(
            address_space,
            ExecutionContext::NotStarted {
                saved: SavedContext::el1t_interruptible(idle_loop as *const () as u64, stack_top),
            },
        ),
    );
    nucleus.idle_thread = Some(idle);
    idle
}

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

    // Carve the boot Time from the boot Untyped, like any Retype-created
    // object: the root budget on core 0, whose chain names itself at the most
    // urgent priority. Retype only creates empty Times, so every other budget
    // is transferred out of this one.
    let boot_time_paddr = carve_region(boot_payload, Carved::<Time>::SIZE)
        .unwrap_or_else(|error| panic!("boot Time carve failed: {:?}", error.code()));
    let boot_time_address = boot_time_paddr.user_to_kernel().as_mut_ptr::<u8>();
    // SAFETY: carve_region reserved the record from the boot Untyped's unused
    // watermark range at page alignment, and the direct map makes it
    // kernel-writable.
    let boot_time_id = unsafe { Carved::<Time>::initialize(boot_time_address, Time::empty(0)) };
    // SAFETY: the record was initialized just above and nothing else names it.
    unsafe {
        *Carved::<Time>::validate(boot_time_id).expect("boot Time just carved") =
            Time::root(0, boot_time_id, BOOT_TIME_BUDGET_NS, HIGHEST_PRIORITY);
    }

    // SAFETY: nucleus_ptr points to the freshly carved, exclusively-owned region.
    let nucleus: &'static mut Nucleus<ArchObjectsImpl> = unsafe { &mut *nucleus_ptr };

    // Allocate the boot AddressSpace (the protection/mapping context) and the
    // boot Thread that executes in it; the Thread's KeyTable was carved and
    // initialized kernel-privately by build_initial_nucleus.
    // Implementation status: the table belongs to the AddressSpace and is
    // bound at its provisioning, independently of creating the boot Thread.
    // SAFETY: build_initial_nucleus initialized the full private boot carve.
    // Boot allocations remain accounted below the Untyped watermark and are
    // never relocated, reclaimed, or reinitialized while a binding is live.
    let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
    // SAFETY: the initialized boot table has the retained backing described above.
    // Provisioning installs the Slot(1) Return sentinel before the binding
    // exists, so the boot AddressSpace has it before it can be activated.
    let boot_table_binding = unsafe { boot_table.bind_address_space() }
        .unwrap_or_else(|error| panic!("boot table provisioning failed: {:?}", error.code()));
    let boot_as_id = nucleus
        .pools
        .arch
        .address_spaces
        .allocate(ArchObjectsImpl::new_address_space(boot_table_binding))
        .expect("no boot AddressSpace slot")
        .0;
    // Carve the boot Thread from the boot Untyped, like every Thread.
    let boot_thread_paddr = carve_region(boot_payload, Carved::<Thread>::SIZE)
        .unwrap_or_else(|error| panic!("boot Thread carve failed: {:?}", error.code()));
    // SAFETY: carve_region reserved the record from the boot Untyped's unused
    // watermark range at page alignment, and the direct map makes it
    // kernel-writable.
    let boot_thread_id = unsafe {
        Carved::<Thread>::initialize(
            boot_thread_paddr.user_to_kernel().as_mut_ptr::<u8>(),
            Thread::new(boot_as_id, ExecutionContext::Running),
        )
    };
    // The boot Thread is the first allocation; make it current.
    nucleus.current_thread = Some(boot_thread_id);

    // Install the boot Thread's self-table capability, its AddressSpace
    // (the bootstrap-era mapping context for `PageTable.Map`/`Frame.Map`),
    // the boot Thread itself, and the boot Untyped as the first grants.
    // keytable_addr names the freshly carved, live boot KeyTable.
    // Implementation status: boot_table was resolved above for AddressSpace
    // provisioning; installing these grants does not establish the binding.
    let self_table_key = boot_table
        .insert(
            KeySlot::SELF_KEYTABLE,
            KeyEntry::new_keytable(
                boot_table_binding.address(),
                BOOT_TABLE_GUARD,
                boot_table_binding.size_bits(),
                Rights::all(),
                0,
            ),
            BOOT_TABLE_GUARD,
        )
        .unwrap_or_else(|failure| {
            panic!("boot self-table install failed: {:?}", failure.error.code())
        });
    // Current-relative Thread.Return authority already sits at the well-known
    // Slot(1): `bind_address_space` installed it during provisioning above. It
    // names no Thread and grants no management rights.
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
            KeyEntry::new_thread(boot_thread_id, Rights::all(), 0),
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

    let boot_time_key = boot_table
        .insert(
            KeySlot::BOOT_TIME,
            KeyEntry::new_carved(boot_time_id, Rights::all(), 0),
            BOOT_TABLE_GUARD,
        )
        .unwrap_or_else(|failure| panic!("boot Time install failed: {:?}", failure.error.code()));

    // The root Scheduler, carved from the boot Untyped: the only authority
    // that configures and starts Threads created by `Untyped.Retype`.
    let root_scheduler_id = carve_boot_object(keytable_addr, boot_untyped_key, Scheduler::root());
    let root_scheduler_key = boot_table
        .insert(
            KeySlot::ROOT_SCHEDULER,
            KeyEntry::new_carved(root_scheduler_id, Rights::all(), 0),
            BOOT_TABLE_GUARD,
        )
        .unwrap_or_else(|failure| {
            panic!("root Scheduler install failed: {:?}", failure.error.code())
        });

    // IRQControl: the only authority that issues IRQHandlers; IRQ authority
    // never comes from memory.
    let irq_control_key = boot_table
        .insert(
            KeySlot::IRQ_CONTROL,
            KeyEntry::new_irq_control(Rights::all()),
            BOOT_TABLE_GUARD,
        )
        .unwrap_or_else(|failure| panic!("IRQControl install failed: {:?}", failure.error.code()));

    install_device_untypeds(boot_table);

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
            )
            .unwrap_or_else(|error| panic!("debug console entry failed: {:?}", error.code())),
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

    // Install the interrupt controller kickstart loaded, now that the kernel
    // half is live.
    if let Some(handoff) = privileged::handoff() {
        // SAFETY: the paired nucleus is mapped; the installer is a boot-only
        // one-shot call before any interrupt is unmasked, and `handoff`
        // describes the component and MMIO kickstart mapped.
        let timer_line = unsafe {
            let install = core::mem::transmute::<
                u64,
                unsafe extern "C" fn(*const libirqchip::PlatformHandoff) -> u32,
            >(NUCLEUS_SET_PLATFORM_VIRT);
            install(handoff)
        };
        assert_ne!(
            timer_line,
            u32::MAX,
            "the nucleus refused the interrupt controller"
        );
        // SAFETY: the nucleus accepted the component, so its relocated
        // operation table is mapped at `ops`.
        let line_count = unsafe { (*(handoff.ops as *const libirqchip::IrqChipOps)).line_count };
        carve_irq_table(nucleus, keytable_addr, boot_untyped_key, line_count);
        semi::println!(
            "🥾 Interrupt controller installed: {line_count} lines, physical timer line {timer_line}"
        );
    }
    print_my_sp();

    BootState {
        nucleus,
        keytable_addr,
        boot_as_id,
        self_table_key,
        boot_untyped_key,
        boot_time_key,
        root_scheduler_key,
        irq_control_key,
        #[cfg(feature = "debug_kernel")]
        debug_console_key,
    }
}

/// First boot-table slot of the device Untypeds (`BOOT_INFO` records the
/// range actually used).
pub const DEVICE_UNTYPED_FIRST_SLOT: u32 = 160;
/// Boot-table slots reserved for device Untypeds.
pub const DEVICE_UNTYPED_SLOTS: u32 = 40;

/// Device memory is handed out in whole pages.
const DEVICE_PAGE: u64 = 4096;
/// Most page runs the handout tracks while splitting.
const MAX_DEVICE_RUNS: usize = 64;

/// Fixed-capacity list of `[start, end)` page runs, kept sorted by start.
struct PageRuns {
    runs: [(u64, u64); MAX_DEVICE_RUNS],
    count: usize,
}

impl PageRuns {
    const fn new() -> Self {
        Self {
            runs: [(0, 0); MAX_DEVICE_RUNS],
            count: 0,
        }
    }

    fn as_slice(&self) -> &[(u64, u64)] {
        &self.runs[..self.count]
    }

    fn push(&mut self, run: (u64, u64)) {
        assert!(self.count < MAX_DEVICE_RUNS, "too many device page runs");
        self.runs[self.count] = run;
        self.count += 1;
    }
}

/// The whole pages covering `[start, start + size)`.
fn page_span(start: u64, size: u64) -> (u64, u64) {
    (
        start & !(DEVICE_PAGE - 1),
        (start + size).next_multiple_of(DEVICE_PAGE),
    )
}

/// The page runs of the enabled device-tree register ranges in `BOOT_INFO`,
/// with overlapping runs merged, minus every page that touches a range the
/// kernel drives itself (`kernel_owned`, as `(start, size)`).
fn device_page_runs(kernel_owned: &[(u64, u64)]) -> PageRuns {
    let mut spans = PageRuns::new();
    BOOT_INFO.lock(|boot_info| {
        for region in boot_info
            .used_regions()
            .filter(|region| region.attributes.mem_attributes == MemAttributes::Device)
        {
            let start = region.start_inclusive.as_u64();
            spans.push(page_span(start, region.end_exclusive.as_u64() - start));
        }
    });
    spans.runs[..spans.count].sort_unstable_by_key(|run| run.0);

    // Merge runs that share pages.
    let mut merged = PageRuns::new();
    for &(start, end) in spans.as_slice() {
        match merged.runs[..merged.count].last_mut() {
            Some(last) if start < last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }

    // Withhold every page touching a kernel-owned range.
    let mut handed = PageRuns::new();
    for &(start, end) in merged.as_slice() {
        let mut cursor = start;
        while cursor < end {
            // The earliest kernel-owned span starting inside the rest of the run.
            let cut = kernel_owned
                .iter()
                .map(|&(owned, size)| page_span(owned, size))
                .filter(|&(owned_start, owned_end)| owned_start < end && owned_end > cursor)
                .min_by_key(|&(owned_start, _)| owned_start);
            match cut {
                Some((owned_start, owned_end)) => {
                    if owned_start > cursor {
                        handed.push((cursor, owned_start));
                    }
                    cursor = owned_end;
                }
                None => {
                    handed.push((cursor, end));
                    cursor = end;
                }
            }
        }
    }
    handed
}

/// Install a device Untyped for every enabled device-tree register range that
/// the kernel does not drive itself, in consecutive boot-table slots from
/// [`DEVICE_UNTYPED_FIRST_SLOT`], and record the range in `BOOT_INFO`.
///
/// Each page run is split into the largest aligned power-of-two blocks, so
/// the Untypeds never overlap.
fn install_device_untypeds(boot_table: &mut KeyTable) {
    let mut count = 0_u32;
    for &(start, end) in device_page_runs(privileged::kernel_owned_ranges()).as_slice() {
        let mut block = start;
        while block < end {
            let align_bits = block.trailing_zeros();
            let length_bits = 63 - (end - block).leading_zeros();
            let size_bits = align_bits.min(length_bits);
            assert!(
                count < DEVICE_UNTYPED_SLOTS,
                "more device Untypeds than reserved slots"
            );
            boot_table
                .insert(
                    KeySlot(DEVICE_UNTYPED_FIRST_SLOT + count),
                    KeyEntry::new_untyped(
                        block,
                        u8::try_from(size_bits).expect("block size fits u8"),
                        true,
                        Rights::all(),
                    ),
                    BOOT_TABLE_GUARD,
                )
                .unwrap_or_else(|failure| {
                    panic!("device Untyped install failed: {:?}", failure.error.code())
                });
            count += 1;
            block += 1 << size_bits;
        }
    }
    BOOT_INFO.lock(|boot_info| {
        boot_info.device_untypeds = DeviceUntypedRange {
            first_slot: DEVICE_UNTYPED_FIRST_SLOT,
            count,
        };
    });
    semi::println!(
        "🥾 {count} device Untypeds in boot slots {DEVICE_UNTYPED_FIRST_SLOT}..{}",
        DEVICE_UNTYPED_FIRST_SLOT + count
    );
}

pub use crate::boot_info::{DeviceUntypedRange as DeviceUntypeds, InterruptControllers};

/// Where Kickstart installed the device Untypeds (empty before bootstrap).
pub fn device_untypeds() -> DeviceUntypeds {
    BOOT_INFO.lock(|boot_info| boot_info.device_untypeds)
}

/// The interrupt controllers `IRQControl.Issue` specifiers are relative to,
/// by phandle in controller-index order (empty without a component).
pub fn interrupt_controllers() -> InterruptControllers {
    BOOT_INFO.lock(|boot_info| boot_info.interrupt_controllers)
}

/// `(physical start, size)` of every MMIO range the kernel drives itself;
/// no device Untyped covers any of their pages.
pub fn kernel_owned_device_ranges() -> &'static [(u64, u64)] {
    privileged::kernel_owned_ranges()
}
