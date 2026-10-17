//! The Bounce fixture: a second trusted `EL1t` context in its own
//! `AddressSpace`, sharing the boot Thread's linked image.
//!
//! Bounce is a two-Thread functional fixture, not an EL0 component: it gets
//! its own `KeyTable`, `AddressSpace`, translation root and ASID, with the
//! retained init image mapped into both the boot ("source") root and its own
//! (see [`crate::translation`]). Suites that switch to it (blocking waits) run
//! a Bounce Thread; suites that migrate into it (PPC) only use its
//! `AddressSpace`. Everything here is bootstrap-era fixture mechanism.

use {
    crate::translation,
    aarch64_cpu::registers::{Readable, TTBR0_EL1},
    core::{
        arch::asm,
        sync::atomic::{AtomicU64, Ordering},
    },
    libcomponent_loader::{
        builder::{Builder, ExecutionStack},
        keys::{BOOT_TABLE_GUARD, BOOT_TABLE_SIZE_BITS, SlotCursor, boot_key, table_slot},
    },
    libobject::{
        ASIDPoolKey, CapError, KeySlot, KeyTableKey, ObjectType, PageTableKey, RawKey, Rights,
        address_space::AddressSpaceKey,
    },
    nucleus::{
        api::key_entry::KeyEntry,
        objects::{
            ArchObjectsImpl, ExecutionContext, KeyTable, Nucleus,
            arch_objects::{AddressSpaceId, AddressSpaceObject, bound_address_space},
            key_table::KeyTableBinding,
        },
    },
};

/// The guard of the tables the suites carve at runtime (Bounce's among them):
/// distinct from the boot table's guard so cross-table key confusion is
/// exercised. Fits the 24 guard bits of a 256-entry table's address.
pub const TEST_TABLE_GUARD: u32 = 0x6E_ED42;

/// The slot half of a key in one of the runtime-carved test tables.
pub fn test_slot(index: u32) -> KeySlot {
    table_slot(TEST_TABLE_GUARD, index)
}

/// The shared high trap stack the boot and Bounce contexts must both use,
/// recorded by [`record_fixture_stacks`]. Test-local state, not syscall ABI.
static FIXTURE_TRAP_SP: AtomicU64 = AtomicU64::new(0);

/// The shared trap stack recorded at boot.
pub fn fixture_trap_sp() -> u64 {
    FIXTURE_TRAP_SP.load(Ordering::Acquire)
}

/// Observe and record the boot context's stacks: returns
/// `(execution SP, shared trap SP)`.
pub fn record_fixture_stacks() -> (u64, u64) {
    let state = fixture_stack_state();
    FIXTURE_TRAP_SP.store(state.1, Ordering::Release);
    state
}

/// Observe the execution SP on `SP_EL0` and the idle shared `SP_EL1` at the same
/// call depth. Reading `SP_EL1` directly at EL1 is not permitted, so briefly
/// select it without touching memory. This trusted fixture runs with DAIF
/// masked; the assertion precedes the temporary stack-selection change.
#[inline(never)]
pub fn fixture_stack_state() -> (u64, u64) {
    let selection: u64;
    let execution_sp: u64;
    let daif: u64;
    // SAFETY: read only the current stack selection, SP and interrupt masks.
    unsafe {
        asm!(
            "mrs {selection}, SPSel",
            "mov {execution_sp}, sp",
            "mrs {daif}, DAIF",
            selection = out(reg) selection,
            execution_sp = out(reg) execution_sp,
            daif = out(reg) daif,
            options(nomem, nostack, preserves_flags),
        );
    }
    assert_eq!(
        selection, 0,
        "trusted fixture must execute as EL1t on SP_EL0"
    );
    assert_eq!(
        daif & 0x3c0,
        0x3c0,
        "fixture stack observation requires masked DAIF"
    );
    let trap_sp: u64;
    // SAFETY: all exceptions are masked, this single assembly block accesses
    // no memory or stack, and it restores the original SP_EL0 selection before
    // Rust resumes. It neither calls code nor changes either stack pointer.
    unsafe {
        asm!(
            "msr SPSel, #1",
            "mov {trap_sp}, sp",
            "msr SPSel, #0",
            trap_sp = out(reg) trap_sp,
            options(nomem, nostack, preserves_flags),
        );
    }
    assert_eq!(
        trap_sp >> 48,
        0xFFFF,
        "shared trap stack must be high-mapped"
    );
    assert_eq!(trap_sp & 15, 0, "trap stack must be 16-byte aligned");
    assert_eq!(
        execution_sp & 15,
        0,
        "execution stack must be 16-byte aligned"
    );
    assert_ne!(
        execution_sp, trap_sp,
        "execution and trap stacks must be distinct"
    );
    (execution_sp, trap_sp)
}

/// The boot ("source") Thread is current, home in `source`, running under the
/// bound `root`/`asid`, which `TTBR0_EL1` holds.
pub fn assert_source_selected(
    nucleus: &Nucleus<ArchObjectsImpl>,
    source: AddressSpaceId,
    root: u64,
    asid: u16,
) {
    let thread = nucleus
        .current_thread_ref()
        .unwrap_or_else(|_| panic!("source Thread missing"));
    assert_eq!(thread.address_space, source);
    assert_eq!(thread.context, ExecutionContext::Running);
    let address_space = nucleus
        .address_space(source)
        .expect("source AddressSpace identity invalid");
    assert_eq!(address_space.translation_root, Some(root));
    assert_eq!(address_space.asid, Some(asid));
    assert_eq!(TTBR0_EL1.get(), root | (u64::from(asid) << 48));
}

/// The boot `AddressSpace`'s translation-root prefix and bound ASID.
pub struct SourceChain {
    pub boot_as_key: RawKey,
    pub boot_asid_pool: ASIDPoolKey,
    /// Root, L1 and L2 `PageTable` keys (boot slots 20–22); only the root is
    /// installed here, [`translation::Provisioner`] links L1/L2 under it.
    pub prefix: [RawKey; 3],
    pub asid: u16,
}

/// Carve the boot root/L1/L2 tables, install the root into the boot
/// `AddressSpace` and bind ASID 1 — without the mapping suite's checks.
pub fn provision_source(builder: &Builder<'_>) -> SourceChain {
    let boot_as_key = boot_key(KeySlot::SELF_ADDRESS_SPACE.0, 1);
    let prefix = [20, 21, 22].map(|slot| {
        builder
            .untyped
            .retype(
                ObjectType::PAGE_TABLE,
                12,
                0,
                1,
                builder.self_table,
                KeySlot(slot).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("source PageTable Retype failed: {:?}", error.code()))
    });
    PageTableKey::from_key(prefix[0])
        .map(boot_as_key, 0)
        .unwrap_or_else(|error| panic!("source root PageTable.Map failed: {:?}", error.code()));
    let boot_asid_pool = ASIDPoolKey::from_key(boot_key(KeySlot::BOOT_ASID_POOL.0, 1));
    let asid = boot_asid_pool
        .assign(boot_as_key)
        .unwrap_or_else(|error| panic!("source ASIDPool.Assign failed: {:?}", error.code()));
    assert_eq!(asid, 1);
    SourceChain {
        boot_as_key,
        boot_asid_pool,
        prefix,
        asid,
    }
}

/// A provisioned Bounce fixture.
pub struct Bounce {
    /// Bounce's accounted high execution stack (eight Frames, boot slots
    /// 41–48), for a Bounce Thread.
    pub stack: ExecutionStack,
    pub table_key: RawKey,
    pub table_addr: u64,
    pub table_binding: KeyTableBinding,
    pub as_id: AddressSpaceId,
    /// The boot table's capability to Bounce's `AddressSpace` (boot slot 69).
    pub as_key: RawKey,
    /// Bounce-local `DebugConsole` key (WRITE only).
    pub debug_console_key: RawKey,
    pub source_root: u64,
    pub bounce_root: u64,
    pub bounce_asid: u16,
}

/// Build Bounce on top of `source`: its stack, table (self-table and
/// `DebugConsole` grants), `AddressSpace`, both image closures and
/// probes, ASID 2 — then activate the source context, which must stay
/// selected. `boot_table_binding` is the boot `AddressSpace`'s table.
pub fn provision(
    builder: &Builder<'_>,
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    source: &SourceChain,
    boot_table_binding: KeyTableBinding,
    boot_as_id: AddressSpaceId,
    debug_console_key: RawKey,
    shared_trap_sp: u64,
) -> Bounce {
    let keytable_addr = builder.boot_table_addr;
    let self_table = builder.self_table;

    // Bounce's kernel stack: eight contiguous 4 KiB frames (32 KiB)
    // carved through the public path. The SVC entry path nests several
    // semihosting println buffers (4 KiB each: the syscall entry, the
    // dispatch, and the per-object handler all format one), so a single
    // 4 KiB stack frame overflows into the carves directly below it and
    // corrupts them — observed as the later Frame.Map alias walk
    // following garbage L2 entries into a fault. 32 KiB holds the
    // deepest handler chain with headroom. Full-descending, so the stack
    // top is the last frame's kernel end.
    // Implementation status: these accounted frames now hold Bounce's
    // EL1t execution stack on SP_EL0, not a per-Thread kernel trap stack.
    // All SVC handlers use the shared high SP_EL1 stack observed above.
    // Eight contiguous accounted Frames at boot slots 41–48, used through
    // the high direct map (the builder checks contiguity).
    let stack = builder.execution_stack(8, &mut SlotCursor::starting_at(41));
    assert_ne!(stack.top, shared_trap_sp);

    // Bounce's capability table, carved through the public Retype path.
    // The fixture's slots (40–48) sit outside the later pool-refill
    // test's destination range (24–35), which requires those slots
    // vacant.
    let table_key = builder
        .untyped
        .retype(
            ObjectType::KEY_TABLE,
            BOOT_TABLE_SIZE_BITS,
            TEST_TABLE_GUARD,
            1,
            self_table,
            KeySlot(40).0,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("Bounce KeyTable Retype failed: {:?}", error.code()));
    let table_id = {
        // SAFETY: the boot table is the live carved boot KeyTable.
        let entry = unsafe { &*KeyTable::at_carve(keytable_addr) }
            .lookup(table_key, BOOT_TABLE_GUARD)
            .unwrap_or_else(|_| panic!("Bounce KeyTable entry missing"));
        entry
            .keytable_id()
            .unwrap_or_else(|_| panic!("Bounce KeyTable entry is not carved"))
    };
    let table_addr = table_id.address;
    // The self-table capability anchors Bounce's invocations: the syscall
    // entry sources the caller's own-table guard from this well-known slot.
    // SAFETY: Bounce's table is the freshly carved, live KeyTable.
    unsafe { &mut *KeyTable::at_carve(table_addr) }
        .insert(
            KeySlot::SELF_KEYTABLE,
            KeyEntry::new_keytable(
                table_id,
                TEST_TABLE_GUARD,
                BOOT_TABLE_SIZE_BITS,
                Rights::all(),
                0,
            ),
            TEST_TABLE_GUARD,
        )
        .unwrap_or_else(|failure| {
            panic!("Bounce self-table grant failed: {:?}", failure.error.code())
        });

    // Bounce's DebugConsole: CopyDerive the boot grant into Bounce's
    // table at the well-known slot through the real SVC path, attenuated
    // to WRITE. The returned Bounce-local key goes to component init.
    let bounce_debug_console_key = self_table
        .copy_derive(
            debug_console_key,
            &KeyTableKey::from_key(table_key),
            KeySlot::DEBUG_CONSOLE.0,
            Rights(Rights::WRITE),
        )
        .unwrap_or_else(|error| {
            panic!("Bounce DebugConsole CopyDerive failed: {:?}", error.code())
        });
    assert_eq!(
        bounce_debug_console_key.slot(),
        test_slot(KeySlot::DEBUG_CONSOLE.0)
    );

    // Provisioning binds Bounce's table to a distinct AddressSpace (installing
    // its Slot(1) Return sentinel) before any Thread can run there.
    // SAFETY: Retype initialized the full private carve; its accounted
    // backing is never relocated, reclaimed, or reinitialized while the
    // binding is live, including after Bounce's Thread is retired.
    let table_binding = unsafe { (&mut *KeyTable::at_carve(table_addr)).bind_address_space() }
        .unwrap_or_else(|error| panic!("Bounce table provisioning failed: {:?}", error.code()));
    assert_ne!(table_binding.address(), boot_table_binding.address());
    assert_eq!(table_binding.size_bits(), BOOT_TABLE_SIZE_BITS);
    let as_id = builder.carve_object(bound_address_space::<ArchObjectsImpl>(table_binding));
    let bounce_as = nucleus
        .address_space(as_id)
        .expect("Bounce AddressSpace just carved");
    assert_ne!(as_id, boot_as_id);
    assert_eq!(bounce_as.keytable().unwrap().address(), table_addr);
    assert_eq!(
        bounce_as.keytable().unwrap().size_bits(),
        BOOT_TABLE_SIZE_BITS
    );
    assert!(bounce_as.translation_root.is_none());
    assert!(bounce_as.asid.is_none());
    let as_key = {
        // SAFETY: both initialized private tables have retained accounted
        // carves; the source and target are distinct, and neither borrow
        // survives a capability invocation.
        let boot_table = unsafe { &mut *KeyTable::at_carve(keytable_addr) };
        let key = boot_table
            .insert(
                KeySlot(69),
                KeyEntry::new_carved(as_id, Rights::all(), 0),
                BOOT_TABLE_GUARD,
            )
            .unwrap_or_else(|failure| panic!("Bounce AS grant failed: {:?}", failure.error.code()));
        // SAFETY: distinct retained initialized Bounce table, exclusively
        // borrowed for this bootstrap grant.
        unsafe { &mut *KeyTable::at_carve(table_addr) }
            .insert(
                KeySlot::SELF_ADDRESS_SPACE,
                KeyEntry::new_carved(as_id, Rights::all(), 0),
                TEST_TABLE_GUARD,
            )
            .unwrap_or_else(|failure| {
                panic!("Bounce self-AS grant failed: {:?}", failure.error.code())
            });
        key
    };
    translation::Provisioner {
        builder,
        source_as: source.boot_as_key,
        bounce_as: as_key,
    }
    .provision(source.prefix, table_key);
    let bounce_asid = source
        .boot_asid_pool
        .assign(as_key)
        .unwrap_or_else(|error| panic!("Bounce ASID assignment failed: {:?}", error.code()));
    assert_eq!(bounce_asid, 2);
    let root_of = |id: AddressSpaceId, what: &str| {
        nucleus
            .address_space(id)
            .unwrap_or_else(|| panic!("{what} AddressSpace missing"))
            .translation_root
            .unwrap_or_else(|| panic!("{what} root missing"))
    };
    let source_root = root_of(boot_as_id, "source");
    let bounce_root = root_of(as_id, "Bounce");
    translation::verify_retained(builder.retained, source_root, bounce_root);
    translation::bind_contexts(source_root, source.asid, bounce_root, bounce_asid);
    // The source must run under its own ASID-1 root before the first wait.
    // Never confuse this bound context with the raw ASID-0 bootstrap TTBR.
    AddressSpaceKey::from_key(source.boot_as_key)
        .activate()
        .unwrap_or_else(|error| panic!("early source Activate failed: {:?}", error.code()));
    assert_source_selected(nucleus, boot_as_id, source_root, source.asid);
    translation::observe_source();
    assert!(matches!(
        AddressSpaceKey::from_key(as_key).activate(),
        Err(CapError::InvalidOperation)
    ));
    assert_eq!(
        TTBR0_EL1.get(),
        source_root | (u64::from(source.asid) << 48)
    );
    Bounce {
        stack,
        table_key,
        table_addr,
        table_binding,
        as_id,
        as_key,
        debug_console_key: bounce_debug_console_key,
        source_root,
        bounce_root,
        bounce_asid,
    }
}
