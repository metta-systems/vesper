//! The memory suite: explicitly managed translation tables and mappings —
//! `PageTable` Retype and installation, ASID binding, `AddressSpace.Retire`
//! (whole-ASID invalidation, ASID release and reuse), Frame.Map/Unmap with
//! the alias policy, Frame cache maintenance, activation making the carved
//! tables hardware-live with observable TLB invalidation, and `PageTable`
//! metadata charged to the invoking Untyped.
//!
//! The Bounce fixture's `AddressSpace` is provisioned (no Bounce Thread runs)
//! so the source context is activated with the retained image mapped, exactly
//! as in the other suites: it occupies `AddressSpace` index 1 and ASID 2.

use {
    aarch64_cpu::registers::{Readable, TCR_EL1, TTBR0_EL1, TTBR1_EL1},
    core::{arch::asm, slice},
    kickstart::bootstrap::{BootState, bootstrap_nucleus, retained_init_memory},
    libaddress::PhysAddr,
    libcomponent_loader::{
        builder::Builder,
        keys::{BOOT_TABLE_GUARD, BOOT_TABLE_SIZE_BITS, boot_key, boot_slot},
    },
    libkicktest::{
        bounce::{
            self, SourceChain, TEST_TABLE_GUARD, assert_source_selected, record_fixture_stacks,
        },
        devices, translation,
    },
    libobject::{
        ASIDPoolKey, CapError, FrameKey, InconsistencyReason, InvalidKeyReason, KeySlot,
        KeyTableKey, ObjectType, PageTableKey, RawKey, Rights, UntypedKey,
        address_space::AddressSpaceKey,
        scheduler::{SchedulerKey, ThreadStart},
        syscall_status::InvalidSelfReference,
    },
    libqemu::semihosting as semi,
    nucleus::{
        api::key_entry::KeyEntry,
        objects::{
            ArchObjectsImpl, KeyTable, Nucleus,
            arch_objects::{AddressSpaceId, AddressSpaceObject, bound_address_space},
            carved::CarvedId,
        },
    },
};

/// Ordinary construction SVC with full-width operands, including slots the
/// typed wrapper cannot represent. This never invokes PPC Call or Return.
fn create_invocation_raw(address_space: RawKey, args: [u64; 6]) -> (u64, u64, u64) {
    let (status, word1, word2): (u64, u64, u64);
    // SAFETY: construction uses the ordinary six-argument capability ABI and
    // returns locally; no execution-context migration or pointer access occurs.
    unsafe {
        asm!(
            "svc #0",
            inlateout("x0") address_space.to_wire() => status,
            inlateout("x1") 3_u64 => word1,
            inlateout("x2") args[0] => word2,
            in("x3") args[1],
            in("x4") args[2],
            in("x5") args[3],
            in("x6") args[4],
            in("x7") args[5],
            options(nostack),
        );
    }
    (status, word1, word2)
}

/// Snapshot only initialized semantic fields, never union padding. Checked
/// lookup checks the occupied slot's incarnation; `NeverIssued` plus `check_insert`
/// checks the vacant slot's zero counter and absence of an installed payload.
#[derive(Debug, PartialEq, Eq)]
struct InvocationDestinationState {
    count: usize,
    target: AddressSpaceId,
    function: u64,
    rights: Rights,
    badge: u16,
    stack: [u64; 3],
}

fn invocation_destination_state(
    table_addr: u64,
    occupied: RawKey,
    vacant: KeySlot,
) -> InvocationDestinationState {
    // SAFETY: callers supply the retained, initialized private boot table.
    // This borrow ends before any subsequent SVC or mutable fixture access.
    let table = unsafe { &*KeyTable::at_carve(table_addr) };
    let entry = table
        .lookup(occupied, BOOT_TABLE_GUARD)
        .unwrap_or_else(|error| panic!("occupied destination changed: {:?}", error.code()));
    let (target, function) = entry
        .invocation_target()
        .unwrap_or_else(|error| panic!("Invocation target changed: {:?}", error.code()));
    let extent = entry
        .invocation_stack_extent()
        .unwrap_or_else(|error| panic!("Invocation extent changed: {:?}", error.code()));
    let never_issued = boot_key(vacant.0, 1);
    assert!(matches!(
        table.lookup(never_issued, BOOT_TABLE_GUARD),
        Err(CapError::InvalidKey {
            key,
            reason: InvalidKeyReason::NeverIssued,
            operand: 0,
        }) if key == never_issued
    ));
    table
        .check_insert(vacant)
        .unwrap_or_else(|error| panic!("vacant destination changed: {:?}", error.code()));
    InvocationDestinationState {
        count: table.len(),
        target,
        function: function.get(),
        rights: entry.rights(),
        badge: entry.badge(),
        stack: [extent.base(), extent.end(), extent.minimum_headroom()],
    }
}

pub fn run() {
    let (_boot_execution_sp, shared_trap_sp) = record_fixture_stacks();
    let retained_init = retained_init_memory();
    translation::observe_bootstrap(crate::run as *const u8 as u64);
    let boot = bootstrap_nucleus();
    let BootState {
        nucleus,
        keytable_addr,
        boot_as_id,
        self_table_key,
        boot_untyped_key,
        debug_console_key,
        root_scheduler_key,
        ..
    } = boot;
    let boot_table_binding = nucleus
        .address_space(boot_as_id)
        .expect("boot AddressSpace missing")
        .keytable()
        .expect("AddressSpace bound");
    let untyped = UntypedKey::from_key(boot_untyped_key);
    let self_table = KeyTableKey::from_key(self_table_key);
    let builder = Builder {
        untyped: &untyped,
        self_table: &self_table,
        boot_table_addr: keytable_addr,
        retained: &retained_init,
    };

    // The Frames the mapping and activation tests use: two 4 KiB frames
    // (boot slots 10/11) and one 2 MiB frame (slot 12). Their Retype checks
    // live in the capability suite.
    let retype_frame = |slot: u32, size_bits: u8| {
        let key = untyped
            .retype(
                ObjectType::FRAME,
                size_bits,
                0,
                1,
                &self_table,
                KeySlot(slot).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("Frame Retype failed: {:?}", error.code()));
        // SAFETY: keytable_addr names the live boot KeyTable; the borrow ends
        // before any further SVC.
        let paddr = unsafe { &*KeyTable::at_carve(keytable_addr) }
            .lookup(key, BOOT_TABLE_GUARD)
            .and_then(KeyEntry::as_frame)
            .unwrap_or_else(|error| panic!("Frame entry missing: {:?}", error.code()))
            .paddr;
        (key, paddr)
    };
    let (frame_key, frame_paddr) = retype_frame(10, 12);
    let (second_frame_key, second_frame_paddr) = retype_frame(11, 12);
    let (large_frame_key, large_frame_paddr) = retype_frame(12, 21);

    // ─────────────────────────────────────────────────────────────────
    // Mapping vertical slice: carved page tables, real
    // descriptor installation, mapping bookkeeping, and teardown.
    // ─────────────────────────────────────────────────────────────────

    // The boot AddressSpace capability (installed at the well-known self
    // slot) is the bootstrap-era mapping context.
    let boot_as_key = boot_key(KeySlot::SELF_ADDRESS_SPACE.0, 1);

    // PageTable Retype: a fixed 4 KiB carve; other size_bits are rejected
    // with the architecture's own error, leaving the slot free.
    assert!(matches!(
        untyped.retype(
            ObjectType::PAGE_TABLE,
            13,
            0,
            1,
            &self_table,
            KeySlot(20).0,
            Rights::all(),
        ),
        Err(CapError::InvalidSize(13))
    ));
    let root_pt_key = untyped
        .retype(
            ObjectType::PAGE_TABLE,
            12,
            0,
            1,
            &self_table,
            KeySlot(20).0,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("root PageTable Retype failed: {:?}", error.code()));
    assert_eq!(root_pt_key.slot(), boot_slot(20));

    // The carved table is sanitized (zeroed) at retype: stale descriptors
    // must never leak prior contents into hardware walks.
    {
        // SAFETY: keytable_addr names the live boot KeyTable.
        let boot_table = unsafe { &*KeyTable::at_carve(keytable_addr) };
        let id = boot_table
            .lookup(root_pt_key, BOOT_TABLE_GUARD)
            .unwrap_or_else(|_| panic!("root PageTable entry missing"))
            .page_table_id()
            .unwrap_or_else(|_| panic!("root PageTable entry has no identity"));
        let paddr = nucleus
            .page_table(id)
            .unwrap_or_else(|| panic!("root PageTable metadata missing"))
            .paddr;
        assert_eq!(paddr % 4096, 0, "the table carve must be 4 KiB-aligned");
        // The table's kernel metadata record is carved from the same
        // Untyped, right after the batch's tables.
        assert_eq!(
            id.address,
            PhysAddr::new(paddr + 4096).user_to_kernel().as_u64(),
            "the metadata record must follow the table"
        );
        // SAFETY: the table lies in the boot Untyped's committed range;
        // the direct map is live.
        let words = unsafe {
            slice::from_raw_parts(PhysAddr::new(paddr).user_to_kernel().as_ptr::<u64>(), 512)
        };
        assert!(
            words.iter().all(|word| *word == 0),
            "page-table contents must be zeroed at retype"
        );
    }

    // Carve the rest of the chain toward vaddr 0x1000_0000.
    let l1_pt_key = untyped
        .retype(
            ObjectType::PAGE_TABLE,
            12,
            0,
            1,
            &self_table,
            KeySlot(21).0,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("L1 PageTable Retype failed: {:?}", error.code()));
    let l2_pt_key = untyped
        .retype(
            ObjectType::PAGE_TABLE,
            12,
            0,
            1,
            &self_table,
            KeySlot(22).0,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("L2 PageTable Retype failed: {:?}", error.code()));
    let l3_pt_key = untyped
        .retype(
            ObjectType::PAGE_TABLE,
            12,
            0,
            1,
            &self_table,
            KeySlot(23).0,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("L3 PageTable Retype failed: {:?}", error.code()));

    // Install the root into the boot AddressSpace (vaddr must be zero).
    //
    // ASID binding through the real SVC path: before a
    // translation root exists, the assignment is rejected — an ASID binds
    // to an AddressSpace's root, not to the AddressSpace in the abstract.
    let boot_asid_pool_key = boot_key(KeySlot::BOOT_ASID_POOL.0, 1);
    let boot_asid_pool = ASIDPoolKey::from_key(boot_asid_pool_key);
    assert!(matches!(
        boot_asid_pool.assign(boot_as_key),
        Err(CapError::NotMapped)
    ));
    // A non-AddressSpace target key is a type mismatch, not a lookup
    // success.
    assert!(matches!(
        boot_asid_pool.assign(boot_untyped_key),
        Err(CapError::TypeMismatch { .. })
    ));

    // AddressSpace activation through the real SVC path: before a
    // translation root exists, activation is rejected — there is no
    // hardware context to install. (A non-AddressSpace invoked key never
    // reaches the handler: dispatch selects the handler by the invoked
    // key's own type.)
    let boot_as = AddressSpaceKey::from_key(boot_as_key);
    assert!(matches!(boot_as.activate(), Err(CapError::NotMapped)));

    let root_pt = PageTableKey::from_key(root_pt_key);
    root_pt
        .map(boot_as_key, 0)
        .unwrap_or_else(|error| panic!("root PageTable.Map failed: {:?}", error.code()));
    // A second root is rejected: the AddressSpace's root slot is occupied.
    assert!(matches!(
        root_pt.map(boot_as_key, 0),
        Err(CapError::AlreadyMapped)
    ));
    {
        let address_space = nucleus
            .address_space(boot_as_id)
            .unwrap_or_else(|| panic!("boot AddressSpace missing"));
        assert!(address_space.translation_root.is_some());
    }

    // A root without a bound ASID still establishes no hardware context.
    assert!(matches!(boot_as.activate(), Err(CapError::NotMapped)));

    // With a root installed, the assignment binds the lowest free ASID
    // (ASID 0 is reserved for the kernel's boot context, so the first
    // grant is 1) and records it on the AddressSpace.
    let bound_asid = boot_asid_pool
        .assign(boot_as_key)
        .unwrap_or_else(|error| panic!("ASIDPool.Assign failed: {:?}", error.code()));
    assert_eq!(bound_asid, 1);
    {
        let address_space = nucleus
            .address_space(boot_as_id)
            .unwrap_or_else(|| panic!("boot AddressSpace missing"));
        assert_eq!(address_space.asid, Some(1));
    }
    // A second assignment to the same AddressSpace is rejected: one ASID
    // per translation context.
    assert!(matches!(
        boot_asid_pool.assign(boot_as_key),
        Err(CapError::AlreadyMapped)
    ));

    // An Invocation into the boot AddressSpace at boot slot 62: the
    // retired-target construction checks below compare against it. Its own
    // construction checks live in the PPC suite.
    let invocation_key = boot_as
        .create_invocation(0x1234, &self_table, KeySlot(62), 0x1000, 0x2000, 0x40)
        .unwrap_or_else(|error| panic!("AddressSpace.CreateInvocation failed: {:?}", error.code()));

    // The Bounce AddressSpace and both image closures: the source context
    // is activated with its image mapped, as the activation tests need.
    let fixture = bounce::provision(
        &builder,
        nucleus,
        &SourceChain {
            boot_as_key,
            boot_asid_pool: ASIDPoolKey::from_key(boot_asid_pool_key),
            prefix: [root_pt_key, l1_pt_key, l2_pt_key],
            asid: bound_asid,
        },
        boot_table_binding,
        boot_as_id,
        debug_console_key,
        shared_trap_sp,
    );
    let bounce_table_binding = fixture.table_binding;
    let source_root = fixture.source_root;
    let bounce_root = fixture.bounce_root;

    // AddressSpace.Retire end-to-end: the address-space teardown —
    // whole-ASID invalidation, ASID release to the originating pool,
    // root/ASID fields cleared, the carved AddressSpace retired — through the real
    // SVC path under `RETIRE` authority.
    // ─────────────────────────────────────────────────────────────────
    {
        // A fixture AddressSpace with its own root and ASID.
        // Provision a distinct initialized table too; its Retype carve
        // stays accounted and is never reclaimed/reinitialized, even
        // after the AddressSpace is retired.
        let fixture_table_key = untyped
            .retype(
                ObjectType::KEY_TABLE,
                BOOT_TABLE_SIZE_BITS,
                TEST_TABLE_GUARD,
                1,
                &self_table,
                KeySlot(66).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| {
                panic!("retirement KeyTable Retype failed: {:?}", error.code())
            });
        let fixture_table_binding = {
            // SAFETY: the live boot table is borrowed only for this lookup.
            let entry = unsafe { &*KeyTable::at_carve(keytable_addr) }
                .lookup(fixture_table_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("retirement KeyTable capability missing"));
            let address = entry
                .keytable_id()
                .unwrap_or_else(|_| panic!("retirement table capability has the wrong kind"))
                .address;
            // SAFETY: the kernel-issued address names the full initialized
            // private Retype carve retained without relocation, reclaim,
            // or reinitialization while any binding is live.
            let binding = unsafe { (&mut *KeyTable::at_carve(address)).bind_address_space() }
                .unwrap_or_else(|error| {
                    panic!("fixture table provisioning failed: {:?}", error.code())
                });
            assert_eq!(
                entry.keytable_guard_and_size().ok(),
                Some((TEST_TABLE_GUARD, binding.size_bits()))
            );
            binding
        };
        assert_ne!(
            fixture_table_binding.address(),
            boot_table_binding.address()
        );
        assert_ne!(
            fixture_table_binding.address(),
            bounce_table_binding.address()
        );
        assert_eq!(fixture_table_binding.size_bits(), BOOT_TABLE_SIZE_BITS);
        let fixture_as_id = builder.carve_object(bound_address_space::<ArchObjectsImpl>(
            fixture_table_binding,
        ));
        assert_ne!(fixture_as_id, boot_as_id);
        let fixture_as_key = {
            // SAFETY: keytable_addr names the live carved boot KeyTable.
            let boot_table = unsafe { &mut *KeyTable::at_carve(keytable_addr) };
            boot_table
                .insert(
                    KeySlot(54),
                    KeyEntry::new_carved(fixture_as_id, Rights::all(), 0),
                    BOOT_TABLE_GUARD,
                )
                .unwrap_or_else(|failure| {
                    panic!(
                        "fixture AddressSpace grant failed: {:?}",
                        failure.error.code()
                    )
                })
        };
        let fixture_as = AddressSpaceKey::from_key(fixture_as_key);

        // Retiring the current caller's own AddressSpace is rejected: the
        // invocation must return to a surviving caller.
        assert!(matches!(
            AddressSpaceKey::from_key(boot_as_key).retire(),
            Err(CapError::InvalidOperation)
        ));

        // A translation root must be torn down first: retire is rejected
        // while one is installed.
        let fixture_root_pt_key = untyped
            .retype(
                ObjectType::PAGE_TABLE,
                12,
                0,
                1,
                &self_table,
                KeySlot(52).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| {
                panic!("fixture root PageTable Retype failed: {:?}", error.code())
            });
        PageTableKey::from_key(fixture_root_pt_key)
            .map(fixture_as_key, 0)
            .unwrap_or_else(|error| {
                panic!("fixture root PageTable.Map failed: {:?}", error.code())
            });
        // Preserve positive innermost-first L2/L1 teardown coverage on a
        // disposable context, never on the source's executing ancestors.
        let fixture_l1_key = untyped
            .retype(
                ObjectType::PAGE_TABLE,
                12,
                0,
                2,
                &self_table,
                96,
                Rights::all(),
            )
            .unwrap_or_else(|error| {
                panic!("fixture intermediates Retype failed: {:?}", error.code())
            });
        let fixture_l1 = PageTableKey::from_key(fixture_l1_key);
        let fixture_l2 = PageTableKey::from_key(boot_key(97, fixture_l1_key.incarnation()));
        fixture_l1
            .map(fixture_root_pt_key, 0)
            .unwrap_or_else(|error| panic!("fixture L1 Map failed: {:?}", error.code()));
        fixture_l2
            .map(fixture_l1_key, 0)
            .unwrap_or_else(|error| panic!("fixture L2 Map failed: {:?}", error.code()));
        let fixture_bound_asid = boot_asid_pool
            .assign(fixture_as_key)
            .unwrap_or_else(|error| panic!("fixture ASIDPool.Assign failed: {:?}", error.code()));
        assert_eq!(
            fixture_bound_asid, 3,
            "the fixture binds the next free ASID"
        );
        assert!(matches!(
            fixture_as.retire(),
            Err(CapError::InvalidOperation)
        ));

        // Unmap the (empty) root, then retire: the ASID is released back
        // to the boot pool and the carved AddressSpace is retired.
        // First reject withdrawal of its nonempty ancestors, then empty
        // the disposable chain innermost-first. This context never executes.
        assert!(matches!(
            fixture_l1.unmap(),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            PageTableKey::from_key(fixture_root_pt_key).unmap(),
            Err(CapError::InvalidOperation)
        ));
        fixture_l2
            .unmap()
            .unwrap_or_else(|error| panic!("fixture L2 Unmap failed: {:?}", error.code()));
        fixture_l1
            .unmap()
            .unwrap_or_else(|error| panic!("fixture L1 Unmap failed: {:?}", error.code()));
        PageTableKey::from_key(fixture_root_pt_key)
            .unmap()
            .unwrap_or_else(|error| {
                panic!("fixture root PageTable.Unmap failed: {:?}", error.code())
            });
        assert!(matches!(
            PageTableKey::from_key(fixture_root_pt_key).unmap(),
            Err(CapError::NotMapped)
        ));
        assert_eq!(
            nucleus
                .address_space(fixture_as_id)
                .expect("fixture AS missing before retirement")
                .translation_root,
            None
        );
        fixture_as.retire().unwrap_or_else(|error| {
            panic!("fixture AddressSpace.Retire failed: {:?}", error.code())
        });
        // Retire bumps the carved generation and marks the record retired:
        // neither the old identity nor a guess at the next one resolves.
        assert!(nucleus.address_space(fixture_as_id).is_none());
        assert!(
            nucleus
                .address_space(CarvedId {
                    generation: fixture_as_id.generation + 1,
                    ..fixture_as_id
                })
                .is_none()
        );
        // The retired AddressSpace's capability names an object that no
        // longer exists: Retire and Activate through it report the retirement
        // against the invoked key.
        let retired_target = CapError::InconsistentKey {
            key: fixture_as_key,
            reason: InconsistencyReason::ObjectRetired,
            operand: 0,
        }
        .code();
        assert_eq!(
            fixture_as.retire().map_err(CapError::code),
            Err(retired_target)
        );
        assert_eq!(
            fixture_as.activate().map_err(CapError::code),
            Err(retired_target)
        );

        // Reuse this genuinely retired target: capability incarnation is
        // unchanged, but its pooled AddressSpace identity is no longer live.
        // Identity rejection wins over zero function, malformed extent and
        // destination readiness without changing either destination.
        let before_stale_rejections =
            invocation_destination_state(keytable_addr, invocation_key, KeySlot(250));
        for function in [0, 0x5678] {
            for slot in [KeySlot(62), KeySlot(250)] {
                assert_eq!(
                    fixture_as
                        .create_invocation(function, &self_table, slot, 0x1001, 0x1001, 0)
                        .map_err(CapError::code),
                    Err(retired_target)
                );
                assert_eq!(
                    invocation_destination_state(keytable_addr, invocation_key, KeySlot(250)),
                    before_stale_rejections
                );
            }
            assert_eq!(
                create_invocation_raw(
                    fixture_as_key,
                    [
                        function,
                        self_table_key.to_wire(),
                        u64::MAX,
                        0x1001,
                        0x1001,
                        0
                    ]
                ),
                retired_target
            );
            assert_eq!(
                invocation_destination_state(keytable_addr, invocation_key, KeySlot(250)),
                before_stale_rejections
            );
        }
        {
            // SAFETY: retained boot table, borrowed only after rejected SVCs.
            let table = unsafe { &*KeyTable::at_carve(keytable_addr) };
            let entry = table
                .lookup(fixture_as_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|error| panic!("stale target cap changed: {:?}", error.code()));
            assert!(matches!(entry.address_space_id(), Ok(id) if id == fixture_as_id));
            assert_eq!(entry.rights(), Rights::all());
        }

        // KeyTable Delete removes the entry naming the retired AddressSpace:
        // it checks only the slot incarnation and never resolves the object.
        let occupied_before = {
            // SAFETY: retained boot table, borrowed only between SVCs.
            unsafe { &*KeyTable::at_carve(keytable_addr) }.len()
        };
        KeyTableKey::from_key(self_table_key)
            .delete(fixture_as_key)
            .unwrap_or_else(|error| {
                panic!(
                    "Delete of a retired AddressSpace's entry failed: {:?}",
                    error.code()
                )
            });
        assert_eq!(
            // SAFETY: as above.
            unsafe { &*KeyTable::at_carve(keytable_addr) }.len(),
            occupied_before - 1
        );
        // The deleted key now fails lookup ahead of the object check, both
        // when invoked and when passed to a second Delete.
        let deleted = |operand| {
            CapError::InconsistentKey {
                key: fixture_as_key,
                reason: InconsistencyReason::CapabilityInvalidated,
                operand,
            }
            .code()
        };
        assert_eq!(fixture_as.retire().map_err(CapError::code), Err(deleted(0)));
        assert_eq!(
            KeyTableKey::from_key(self_table_key)
                .delete(fixture_as_key)
                .map_err(CapError::code),
            Err(deleted(2))
        );

        // The released ASID is the next one granted: a third fixture
        // AddressSpace with a fresh root binds ASID 2 again.
        // Implementation status: Bounce occupies pool index 1, so this
        // AddressSpace is index 3. It gets a fresh table, not the retired
        // fixture's retained carve; ASID reuse does not reset table identity.
        // Implementation status update: retained Bounce owns ASID 2, so
        // this disposable retirement fixture releases and rebinds ASID 3.
        let rebind_table_key = untyped
            .retype(
                ObjectType::KEY_TABLE,
                BOOT_TABLE_SIZE_BITS,
                TEST_TABLE_GUARD,
                1,
                &self_table,
                KeySlot(67).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("rebind KeyTable Retype failed: {:?}", error.code()));
        let rebind_table_binding = {
            // SAFETY: the live boot table is borrowed only for this lookup.
            let entry = unsafe { &*KeyTable::at_carve(keytable_addr) }
                .lookup(rebind_table_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("rebind KeyTable capability missing"));
            let address = entry
                .keytable_id()
                .unwrap_or_else(|_| panic!("rebind table capability has the wrong kind"))
                .address;
            // SAFETY: the kernel-issued address names the full initialized
            // private Retype carve retained without relocation, reclaim,
            // or reinitialization while any binding is live.
            let binding = unsafe { (&mut *KeyTable::at_carve(address)).bind_address_space() }
                .unwrap_or_else(|error| {
                    panic!("fixture table provisioning failed: {:?}", error.code())
                });
            assert_eq!(
                entry.keytable_guard_and_size().ok(),
                Some((TEST_TABLE_GUARD, binding.size_bits()))
            );
            binding
        };
        assert_ne!(rebind_table_binding.address(), boot_table_binding.address());
        assert_ne!(
            rebind_table_binding.address(),
            bounce_table_binding.address()
        );
        assert_ne!(
            rebind_table_binding.address(),
            fixture_table_binding.address()
        );
        assert_eq!(rebind_table_binding.size_bits(), BOOT_TABLE_SIZE_BITS);
        let rebind_as_id =
            builder.carve_object(bound_address_space::<ArchObjectsImpl>(rebind_table_binding));
        assert_ne!(rebind_as_id, fixture_as_id);
        let rebind_as_key = {
            // SAFETY: keytable_addr names the live carved boot KeyTable.
            let boot_table = unsafe { &mut *KeyTable::at_carve(keytable_addr) };
            boot_table
                .insert(
                    KeySlot(55),
                    KeyEntry::new_carved(rebind_as_id, Rights::all(), 0),
                    BOOT_TABLE_GUARD,
                )
                .unwrap_or_else(|failure| {
                    panic!(
                        "rebind AddressSpace grant failed: {:?}",
                        failure.error.code()
                    )
                })
        };
        let rebind_root_pt_key = untyped
            .retype(
                ObjectType::PAGE_TABLE,
                12,
                0,
                1,
                &self_table,
                KeySlot(53).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| {
                panic!("rebind root PageTable Retype failed: {:?}", error.code())
            });
        PageTableKey::from_key(rebind_root_pt_key)
            .map(rebind_as_key, 0)
            .unwrap_or_else(|error| panic!("rebind root PageTable.Map failed: {:?}", error.code()));
        let rebound_asid = boot_asid_pool
            .assign(rebind_as_key)
            .unwrap_or_else(|error| panic!("rebind ASIDPool.Assign failed: {:?}", error.code()));
        assert_eq!(
            rebound_asid, 3,
            "the retired AddressSpace's ASID was released back to the pool"
        );
    }

    // Build the intermediate chain: L1 under the root, L2 under L1,
    // L3 under L2, all selecting the slots for vaddr 0x1000_0000.
    let l1_pt = PageTableKey::from_key(l1_pt_key);
    // Implementation status: L1/L2 already serve the retained source
    // image, stack and switch probe. Only this disposable test L3 is late.
    assert!(matches!(
        l1_pt.map(root_pt_key, 0x1000_0000),
        Err(CapError::AlreadyMapped)
    ));
    let l2_pt = PageTableKey::from_key(l2_pt_key);
    assert!(matches!(
        l2_pt.map(l1_pt_key, 0x1000_0000),
        Err(CapError::AlreadyMapped)
    ));
    let l3_pt = PageTableKey::from_key(l3_pt_key);
    l3_pt
        .map(l2_pt_key, 0x1000_0000)
        .unwrap_or_else(|error| panic!("L3 PageTable.Map failed: {:?}", error.code()));

    // Mapping a table into itself is rejected as an alias.
    assert!(matches!(
        l1_pt.map(l1_pt_key, 0x1000_0000),
        Err(CapError::InvalidOperation)
    ));

    // Frame.Map with a missing intermediate fails with the faulting vaddr.
    let frame = FrameKey::from_key(frame_key);
    assert!(matches!(
        frame.map(
            boot_as_key,
            0x2000_0000,
            Rights(Rights::READ | Rights::WRITE),
            libobject::FrameAttribute::Default
        ),
        Err(CapError::MissingIntermediate { vaddr: 0x2000_0000 })
    ));
    // A misaligned virtual address is rejected with the frame's size.
    assert!(matches!(
        frame.map(
            boot_as_key,
            0x1000_0001,
            Rights(Rights::READ | Rights::WRITE),
            libobject::FrameAttribute::Default
        ),
        Err(CapError::InvalidSize(12))
    ));
    // A RAM Frame maps write-back only: every other attribute is refused.
    assert!(matches!(
        frame.map(
            boot_as_key,
            0x1000_0000,
            Rights(Rights::READ | Rights::WRITE),
            libobject::FrameAttribute::WriteCombining
        ),
        Err(CapError::InvalidOperation)
    ));

    // Real mapping: the walk installs the page descriptor at level 3.
    frame
        .map(
            boot_as_key,
            0x1000_0000,
            Rights(Rights::READ | Rights::WRITE),
            libobject::FrameAttribute::Default,
        )
        .unwrap_or_else(|error| panic!("Frame.Map failed: {:?}", error.code()));
    // A second mapping of the same capability is rejected.
    assert!(matches!(
        frame.map(
            boot_as_key,
            0x1000_0000,
            Rights(Rights::READ | Rights::WRITE),
            libobject::FrameAttribute::Default
        ),
        Err(CapError::AlreadyMapped)
    ));

    // Verify the descriptor chain by hand through the direct map.
    let root_paddr = nucleus
        .address_space(boot_as_id)
        .unwrap_or_else(|| panic!("boot AddressSpace missing"))
        .translation_root
        .expect("translation root missing");
    {
        const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;
        // SAFETY: the tables are carved RAM in the boot Untyped's committed
        // range; the direct map is live.
        let read_entry = |paddr: u64, slot: usize| unsafe {
            *(PhysAddr::new(paddr).user_to_kernel().as_ptr::<u64>()).add(slot)
        };
        let l0e = read_entry(root_paddr, 0);
        assert!(
            l0e & 0b11 == 0b11,
            "L0 entry must be a valid table descriptor"
        );
        let l1_paddr = l0e & ADDR_MASK;
        let l1e = read_entry(l1_paddr, 0);
        assert!(
            l1e & 0b11 == 0b11,
            "L1 entry must be a valid table descriptor"
        );
        let l2_paddr = l1e & ADDR_MASK;
        let l2e = read_entry(l2_paddr, 128);
        assert!(
            l2e & 0b11 == 0b11,
            "L2 entry must be a valid table descriptor"
        );
        let l3_paddr = l2e & ADDR_MASK;
        let pte = read_entry(l3_paddr, 0);
        assert_eq!(pte & ADDR_MASK, frame_paddr, "the PTE must name the frame");
        assert!(pte & 0b1 != 0, "the PTE must be valid");
        assert!(pte & 0b10 != 0, "a level-3 entry must be a page descriptor");
        assert!(pte & (1 << 10) != 0, "the access flag must be set");
        assert!(
            pte & (1 << 11) != 0,
            "a runtime TTBR0 page must be non-global (nG=1)"
        );
        assert_eq!(
            pte & (0b11 << 6),
            0b01 << 6,
            "a READ|WRITE mapping must be user read/write"
        );
        assert!(
            pte & (1 << 53) != 0 && pte & (1 << 54) != 0,
            "a mapping without the EXECUTE right must stay UXN|PXN"
        );
    }

    // The frame entry records the full mapping identity.
    {
        // SAFETY: keytable_addr names the live boot KeyTable.
        let boot_table = unsafe { &*KeyTable::at_carve(keytable_addr) };
        let f = boot_table
            .lookup(frame_key, BOOT_TABLE_GUARD)
            .unwrap_or_else(|_| panic!("frame entry missing"))
            .as_frame()
            .unwrap_or_else(|_| panic!("frame entry is not a Frame cap"));
        assert!(f.is_mapped());
        assert_eq!(f.mapping().unwrap().vaddr, 0x1000_0000);
    }

    // CopyDerive of a mapped frame yields an unmapped derived capability:
    // Copy is capability-only derivation with no mapping association.
    let derived_frame_key = self_table
        .copy_derive(
            frame_key,
            &self_table,
            KeySlot(13).0,
            Rights(Rights::READ | Rights::WRITE),
        )
        .unwrap_or_else(|error| panic!("frame CopyDerive failed: {:?}", error.code()));
    assert!(matches!(
        FrameKey::from_key(derived_frame_key).unmap(),
        Err(CapError::NotMapped)
    ));

    // Alias policy: the derived capability names the same physical frame
    // the original maps at 0x1000_0000. Mapping it at a second virtual
    // address in the same Domain is rejected whatever capability carries
    // it; the error names the conflicting live mapping's physical base.
    assert!(matches!(
        FrameKey::from_key(derived_frame_key).map(
            boot_as_key,
            0x1000_1000,
            Rights(Rights::READ | Rights::WRITE),
            libobject::FrameAttribute::Default
        ),
        Err(CapError::PhysicalAlias { paddr }) if paddr == frame_paddr
    ));

    // A 2 MiB frame installs a block descriptor at level 2 in the same
    // chain (a distinct slot, read-only).
    let large_frame = FrameKey::from_key(large_frame_key);
    large_frame
        .map(
            boot_as_key,
            0x1020_0000,
            Rights(Rights::READ),
            libobject::FrameAttribute::Default,
        )
        .unwrap_or_else(|error| panic!("2 MiB Frame.Map failed: {:?}", error.code()));
    {
        const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;
        // SAFETY: see above.
        let read_entry = |paddr: u64, slot: usize| unsafe {
            *(PhysAddr::new(paddr).user_to_kernel().as_ptr::<u64>()).add(slot)
        };
        let l0e = read_entry(root_paddr, 0);
        let l1e = read_entry(l0e & ADDR_MASK, 0);
        let block = read_entry(l1e & ADDR_MASK, 129);
        assert_eq!(block & ADDR_MASK, large_frame_paddr);
        assert!(block & 0b1 != 0, "the block descriptor must be valid");
        assert!(
            block & 0b10 == 0,
            "a level-2 entry must be a block descriptor"
        );
        assert!(
            block & (1 << 11) != 0,
            "a runtime TTBR0 block must be non-global (nG=1)"
        );
        assert_eq!(
            block & (0b11 << 6),
            0b11 << 6,
            "a READ-only mapping must be user read-only"
        );
    }

    // ─────────────────────────────────────────────────────────────────
    // Domain activation (2026-09-15): install the bound root into
    // TTBR0_EL1 with the bound ASID, making the carved tables the live
    // hardware translation context for the low half. The kernel executes
    // through the TTBR1 high map, so kernel code keeps running unchanged
    // while the Domain's context is installed.
    // ─────────────────────────────────────────────────────────────────

    // Distinct marker contents: the original frame carries one magic
    // word, the second carved frame another, so a stale cached
    // translation is distinguishable from a freshly walked one.
    #[expect(clippy::items_after_statements)]
    const MAGIC_ORIGINAL: u64 = 0x1111_2222_3333_4444;
    #[expect(clippy::items_after_statements)]
    const MAGIC_SECOND: u64 = 0x5555_6666_7777_8888;
    // SAFETY: both frames lie in the boot Untyped's committed range; the
    // direct map is live.
    unsafe {
        PhysAddr::new(frame_paddr)
            .user_to_kernel()
            .as_mut_ptr::<u64>()
            .write_volatile(MAGIC_ORIGINAL);
        PhysAddr::new(second_frame_paddr)
            .user_to_kernel()
            .as_mut_ptr::<u64>()
            .write_volatile(MAGIC_SECOND);
    }

    // Keep the already provisioned source context installed throughout
    // these live remaps. Activation below is idempotent, not a late image
    // mapping step; the ASID-0 bootstrap context is never restored.
    let boot_ttbr0 = TTBR0_EL1.get();
    let boot_ttbr1 = TTBR1_EL1.get();

    // Read only the selected path through retained private table backing;
    // no references survive a SVC. Bootstrap identity-block nG and invariant
    // high vector/direct-map global checks ran before early activation.
    // create_identity_mapping installs only 2 MiB blocks: no live bootstrap
    // TTBR0 page exists to test map_page's TTBR0 branch.
    let read_leaf = translation::read_leaf;

    // A real Domain's address space contains its own image and stack by
    // construction, and the bootstrap caller is no exception. Its stack
    // sits below the image base at 0x80000; the linked image extends beyond
    // the 2 MiB boundary. Both were mapped before the first handoff using
    // accounted retained pages, not fabricated two-block authority over
    // unrelated occupied bytes. EXECUTE-requested image leaves have nG and
    // UXN|PXN clear with trusted writable EL1 AP=00; stack leaves remain XN.
    translation::verify_retained(&retained_init, root_paddr, bounce_root);

    // Activate through the real SVC path: the tables become hardware-live.
    boot_as
        .activate()
        .unwrap_or_else(|error| panic!("AddressSpace.Activate failed: {:?}", error.code()));
    let active_ttbr0 = root_paddr | (u64::from(bound_asid) << 48);
    assert_eq!(
        TTBR0_EL1.get(),
        active_ttbr0,
        "activation must install the carved root with its bound ASID"
    );
    assert_eq!(active_ttbr0, boot_ttbr0, "later Activate is idempotent");
    assert_eq!(
        TCR_EL1.get() & (1 << 22),
        0,
        "TCR.A1 must select TTBR0's ASID for the warmed remap"
    );
    assert_eq!(
        TTBR1_EL1.get(),
        boot_ttbr1,
        "the kernel map stays unchanged"
    );

    // A load from the mapped virtual address now walks the AddressSpace's
    // tables: the marker written through the direct map must come back
    // through the level-3 page descriptor.
    // SAFETY: the activated translation context maps this virtual address
    // to the original frame; the boot test runs at EL1 with PAN inactive.
    let observed = unsafe { (0x1000_0000_u64 as *const u64).read_volatile() };
    assert_eq!(
        observed, MAGIC_ORIGINAL,
        "the activated context must serve the real mapping"
    );

    // Volatile accesses force real loads/stores rather than compiler
    // reuse of a marker. Keep this root/ASID installed until both remaps
    // have been observed: no reactivation or test-side TLBI can hide a
    // missing or misencoded VA-scoped invalidation in Frame.Unmap.
    // Hardware may evict a warmed translation; this is not a proof that
    // unrelated VAs/ASIDs remain cached instead of a broader flush.

    // Unmapping a non-empty table is rejected: L3 still holds the page.
    assert!(matches!(l3_pt.unmap(), Err(CapError::InvalidOperation)));
    // L2 still holds the L3 table descriptor.
    assert!(matches!(l2_pt.unmap(), Err(CapError::InvalidOperation)));

    // Frame.Unmap clears the descriptor and the record, and withdraws the
    // cached translation under the bound ASID (tlbi vae1is + dsb/isb) —
    // executing the real maintenance sequence here proves it is safe on
    // the live kernel context.
    // SAFETY: the original page is still mapped; warm it again immediately
    // before Unmap, after the rejection-path SVCs above.
    let rewarmed = unsafe { (0x1000_0000_u64 as *const u64).read_volatile() };
    assert_eq!(rewarmed, MAGIC_ORIGINAL);
    frame
        .unmap()
        .unwrap_or_else(|error| panic!("Frame.Unmap failed: {:?}", error.code()));
    assert!(matches!(frame.unmap(), Err(CapError::NotMapped)));
    {
        const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;
        // SAFETY: see above.
        let read_entry = |paddr: u64, slot: usize| unsafe {
            *(PhysAddr::new(paddr).user_to_kernel().as_ptr::<u64>()).add(slot)
        };
        let l0e = read_entry(root_paddr, 0);
        let l1e = read_entry(l0e & ADDR_MASK, 0);
        let l2e = read_entry(l1e & ADDR_MASK, 128);
        assert_eq!(read_entry(l2e & ADDR_MASK, 0), 0, "the PTE must be cleared");
    }

    // The unmap above cleared the descriptor and invalidated the cached
    // translation under the bound ASID on the live context. Prove the
    // invalidation: map the second frame (distinct physical backing and
    // contents) at the same virtual address and read through it — a
    // stale cached entry would still serve the original frame's marker.
    FrameKey::from_key(second_frame_key)
        .map(
            boot_as_key,
            0x1000_0000,
            Rights(Rights::READ | Rights::WRITE),
            libobject::FrameAttribute::Default,
        )
        .unwrap_or_else(|error| panic!("second frame Frame.Map failed: {:?}", error.code()));
    assert_eq!(
        TTBR0_EL1.get(),
        active_ttbr0,
        "remap must not switch contexts"
    );
    {
        let (level, pte) = read_leaf(active_ttbr0, 0x1000_0000);
        assert_eq!(level, 3, "the replacement must be a page leaf");
        assert_eq!(pte & 0x0000_FFFF_FFFF_F000, second_frame_paddr);
        assert!(
            pte & (1 << 11) != 0,
            "the replacement page must be non-global"
        );
    }
    // SAFETY: the activated context now maps this virtual address to the
    // second frame; PAN is inactive at EL1.
    let observed = unsafe { (0x1000_0000_u64 as *const u64).read_volatile() };
    assert_eq!(
        observed, MAGIC_SECOND,
        "the unmap's TLB invalidation must withdraw the stale translation"
    );
    FrameKey::from_key(second_frame_key)
        .unmap()
        .unwrap_or_else(|error| panic!("second frame Frame.Unmap failed: {:?}", error.code()));
    frame_cache_maintenance(
        &self_table,
        second_frame_key,
        second_frame_paddr,
        MAGIC_SECOND,
    );
    device_frame_reads_pl011(&self_table, boot_as_key, keytable_addr, active_ttbr0);
    frame_attribute_ceilings(
        &self_table,
        &untyped,
        boot_as_key,
        keytable_addr,
        active_ttbr0,
    );
    // Repeat in the reverse direction while the second translation is
    // warm: neither physical marker changes, only the same VA's backing.
    frame
        .map(
            boot_as_key,
            0x1000_0000,
            Rights(Rights::READ | Rights::WRITE),
            libobject::FrameAttribute::Default,
        )
        .unwrap_or_else(|error| panic!("original frame remap failed: {:?}", error.code()));
    assert_eq!(
        TTBR0_EL1.get(),
        active_ttbr0,
        "reverse remap keeps the same ASID/root"
    );
    {
        let (level, pte) = read_leaf(active_ttbr0, 0x1000_0000);
        assert_eq!(level, 3);
        assert_eq!(pte & 0x0000_FFFF_FFFF_F000, frame_paddr);
        assert!(pte & (1 << 11) != 0, "the restored page must be non-global");
    }
    // SAFETY: the still-active context maps the original frame again;
    // the volatile load must not reuse the second frame's warmed result.
    let observed = unsafe { (0x1000_0000_u64 as *const u64).read_volatile() };
    assert_eq!(
        observed, MAGIC_ORIGINAL,
        "the second unmap must withdraw the warmed replacement translation"
    );
    frame
        .unmap()
        .unwrap_or_else(|error| panic!("original frame final Unmap failed: {:?}", error.code()));
    assert_eq!(TTBR0_EL1.get(), active_ttbr0);
    assert_eq!(TTBR1_EL1.get(), boot_ttbr1);

    // Keep the bound source root live. Never restore the raw ASID-0
    // bootstrap TTBR while Thread metadata still names the ASID-1 source,
    // and never withdraw its executing image or low SP_EL0 stack. Only
    // disposable test mappings and their now-empty L3 are torn down.

    // With the original unmapped, the physical extent is free in this
    // Domain again: the derived capability now maps at the second address,
    // proving the policy tracks live physical mappings, not capability
    // identity. The 2 MiB block remains mapped and disjoint, so the overlap
    // walk must not reject the unrelated extent.
    FrameKey::from_key(derived_frame_key)
        .map(
            boot_as_key,
            0x1000_1000,
            Rights(Rights::READ | Rights::WRITE),
            libobject::FrameAttribute::Default,
        )
        .unwrap_or_else(|error| panic!("post-unmap derived Frame.Map failed: {:?}", error.code()));
    {
        const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;
        // SAFETY: see above.
        let read_entry = |paddr: u64, slot: usize| unsafe {
            *(PhysAddr::new(paddr).user_to_kernel().as_ptr::<u64>()).add(slot)
        };
        let l0e = read_entry(root_paddr, 0);
        let l1e = read_entry(l0e & ADDR_MASK, 0);
        let l2e = read_entry(l1e & ADDR_MASK, 128);
        let pte = read_entry(l2e & ADDR_MASK, 1);
        assert_eq!(
            pte & ADDR_MASK,
            frame_paddr,
            "the derived mapping's PTE must name the same frame"
        );
        assert!(pte & 0b1 != 0, "the derived PTE must be valid");
    }
    FrameKey::from_key(derived_frame_key)
        .unmap()
        .unwrap_or_else(|error| panic!("derived Frame.Unmap failed: {:?}", error.code()));
    // The 2 MiB block clears too.
    large_frame
        .unmap()
        .unwrap_or_else(|error| panic!("2 MiB Frame.Unmap failed: {:?}", error.code()));

    // Now the empty tables unmap cleanly, innermost first.
    l3_pt
        .unmap()
        .unwrap_or_else(|error| panic!("L3 PageTable.Unmap failed: {:?}", error.code()));
    // Shared ancestors still carry the active image/stack/probe. Their
    // empty-table guards must reject teardown; root withdrawal and whole-
    // ASID invalidation are covered on the disposable retirement root above.
    assert!(matches!(l2_pt.unmap(), Err(CapError::InvalidOperation)));
    assert!(matches!(l1_pt.unmap(), Err(CapError::InvalidOperation)));
    assert!(matches!(root_pt.unmap(), Err(CapError::InvalidOperation)));
    assert_eq!(TTBR0_EL1.get(), active_ttbr0);
    {
        let address_space = nucleus
            .address_space(boot_as_id)
            .unwrap_or_else(|| panic!("boot AddressSpace missing"));
        assert_eq!(address_space.translation_root, Some(root_paddr));
        assert_eq!(address_space.asid, Some(bound_asid));
    }

    // Page-table metadata is charged to the invoking Untyped: a 16 KiB
    // Untyped would hold four bare 4 KiB tables, but each table also carves
    // its metadata record after the batch's tables, so a batch of four does
    // not fit and leaves the Untyped unchanged, while a batch of three does.
    let charge_key = untyped
        .retype(
            ObjectType::UNTYPED,
            14,
            0,
            1,
            &self_table,
            KeySlot(24).0,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("16 KiB Untyped split failed: {:?}", error.code()));
    let charge = UntypedKey::from_key(charge_key);
    assert!(matches!(
        charge.retype(
            ObjectType::PAGE_TABLE,
            12,
            0,
            4,
            &self_table,
            KeySlot(25).0,
            Rights::all(),
        ),
        Err(CapError::InsufficientMemory)
    ));
    let charged = charge
        .retype(
            ObjectType::PAGE_TABLE,
            12,
            0,
            3,
            &self_table,
            KeySlot(25).0,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("charged PageTable Retype failed: {:?}", error.code()));
    {
        // SAFETY: keytable_addr names the live boot KeyTable; no SVC runs
        // while this shared borrow is live.
        let boot_table = unsafe { &*KeyTable::at_carve(keytable_addr) };
        let base = boot_table
            .lookup(charge_key, BOOT_TABLE_GUARD)
            .and_then(KeyEntry::as_untyped)
            .unwrap_or_else(|error| panic!("16 KiB Untyped missing: {:?}", error.code()))
            .paddr;
        let ids: [_; 3] = core::array::from_fn(|index| {
            let key = boot_key(25 + u32::try_from(index).unwrap(), charged.incarnation());
            boot_table
                .lookup(key, BOOT_TABLE_GUARD)
                .and_then(KeyEntry::page_table_id)
                .unwrap_or_else(|error| panic!("charged PageTable missing: {:?}", error.code()))
        });
        // The failed batch of four reserved nothing: the tables start at the
        // Untyped's base, and the records follow the last table.
        let stride = ids[1].address - ids[0].address;
        assert!(stride > 0);
        for (index, id) in (0_u64..).zip(ids) {
            let table = nucleus
                .page_table(id)
                .unwrap_or_else(|| panic!("charged PageTable metadata missing"));
            assert_eq!(table.paddr, base + index * 4096);
            assert_eq!(
                id.address,
                PhysAddr::new(base + 3 * 4096).user_to_kernel().as_u64() + index * stride
            );
        }
    }
    // Nothing more fits: the remaining bytes hold no table and record.
    assert!(matches!(
        charge.retype(
            ObjectType::PAGE_TABLE,
            12,
            0,
            1,
            &self_table,
            KeySlot(28).0,
            Rights::all(),
        ),
        Err(CapError::InsufficientMemory)
    ));
    assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);
    translation::verify_retained(&retained_init, source_root, bounce_root);

    address_space_bind(&builder, nucleus, root_scheduler_key, &boot_asid_pool);

    semi::println!("Memory suite passed");
}

/// The PL011's register page (QEMU's raspi3b and raspi4b device trees).
#[cfg(board_rpi3)]
const PL011_PAGE: u64 = 0x3f20_1000;
#[cfg(board_rpi4)]
const PL011_PAGE: u64 = 0xfe20_1000;
/// Where the device Frame is mapped: a free page under the suite's L3 table.
const DEVICE_VA: u64 = 0x1000_3000;
/// First boot-table slot for the device Frames this check retypes.
const DEVICE_FRAME_SLOT: u32 = 220;

/// Device memory end to end: the device Untypeds Kickstart handed out avoid
/// the interrupt controller, one of them covers the PL011, a Frame retyped
/// from it maps only as non-executable device memory, and reading through
/// the mapping returns the PL011's `PrimeCell` peripheral ID.
fn device_frame_reads_pl011(
    self_table: &KeyTableKey,
    boot_as_key: RawKey,
    keytable_addr: u64,
    active_ttbr0: u64,
) {
    devices::assert_device_untypeds_avoid_the_kernel(keytable_addr);
    let (untyped_key, base, _) = devices::device_untyped_extents(keytable_addr)
        .find(|&(_, start, end)| (start..end).contains(&PL011_PAGE))
        .expect("no device Untyped covers the PL011");

    // Frames are carved from the start of the Untyped: retype up to the
    // PL011's page and use the last one.
    let count = u32::try_from((PL011_PAGE - base) / 0x1000 + 1).unwrap();
    let first = UntypedKey::from_key(untyped_key)
        .retype(
            ObjectType::FRAME,
            12,
            0,
            count,
            self_table,
            DEVICE_FRAME_SLOT,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("device Frame Retype failed: {:?}", error.code()));
    let frame = FrameKey::from_key(boot_key(DEVICE_FRAME_SLOT + count - 1, first.incarnation()));
    assert!(matches!(
        frame.map(
            boot_as_key,
            DEVICE_VA,
            Rights(Rights::READ | Rights::EXECUTE),
            libobject::FrameAttribute::Default
        ),
        Err(CapError::InvalidOperation)
    ));
    frame
        .map(
            boot_as_key,
            DEVICE_VA,
            Rights(Rights::READ | Rights::WRITE),
            libobject::FrameAttribute::Default,
        )
        .unwrap_or_else(|error| panic!("device Frame.Map failed: {:?}", error.code()));
    let (level, pte) = translation::read_leaf(active_ttbr0, DEVICE_VA);
    assert_eq!(level, 3);
    assert_eq!(pte & 0x0000_FFFF_FFFF_F000, PL011_PAGE);
    assert_eq!(
        (pte >> 2) & 0b111,
        3,
        "a device Frame maps Device-nGnRnE (AttrIndx 3) by default"
    );
    assert_eq!(pte & ((1 << 53) | (1 << 54)), (1 << 53) | (1 << 54));
    // SAFETY: the active context maps DEVICE_VA to the PL011's registers as
    // device memory; UARTPeriphID0 at +0xFE0 is a read-only ID register.
    let periph_id0 = unsafe { ((DEVICE_VA + 0xFE0) as *const u32).read_volatile() };
    // Device memory is never cached: cache maintenance is refused.
    assert!(matches!(
        frame.clean_data(0, 0x1000),
        Err(CapError::InvalidOperation)
    ));
    assert_eq!(periph_id0 & 0xff, 0x11, "PL011 UARTPeriphID0");
    frame
        .unmap()
        .unwrap_or_else(|error| panic!("device Frame.Unmap failed: {:?}", error.code()));
    semi::println!("Device Frame mapped the PL011: UARTPeriphID0 = {periph_id0:#x}");
}

/// First boot-table slot of the Frames and Untypeds the attribute check
/// carves; each carve takes the next one.
const ATTRIBUTE_SLOT: u32 = 330;
/// Where the attribute check maps its Frames: free pages under the suite's L3
/// table, apart from `DEVICE_VA`.
const ATTRIBUTE_VA: u64 = 0x1000_8000;

/// Frame memory attributes (contract: Frame memory attributes): the `MAIR`
/// index each attribute writes, the ceilings Kickstart gives the register
/// Untypeds (strict) and the video memory Untypeds (write-combining), lowering
/// a ceiling at Retype, inheritance through `CopyDerive` and Untyped splits,
/// and RAM Frames mapping write-back only.
fn frame_attribute_ceilings(
    self_table: &KeyTableKey,
    ram: &UntypedKey,
    boot_as_key: RawKey,
    keytable_addr: u64,
    active_ttbr0: u64,
) {
    use libobject::FrameAttribute::{self, Cacheable, DevicePosted, DeviceStrict, WriteCombining};
    let mut next_slot = ATTRIBUTE_SLOT;
    let mut take = || {
        let slot = next_slot;
        next_slot += 1;
        slot
    };
    // All rights: the execute checks must reach the device rule, not fail the
    // rights ceiling first.
    let rights = Rights::all();
    let invalid = |result: Result<RawKey, CapError>| {
        assert!(matches!(result, Err(CapError::InvalidOperation)));
    };
    // Map `frame` with `attribute`, check the descriptor's `MAIR` index and
    // execute-never bits, and unmap it again.
    let mapped_index = |frame: &FrameKey, attribute: FrameAttribute| -> u64 {
        frame
            .map(
                boot_as_key,
                ATTRIBUTE_VA,
                Rights(Rights::READ | Rights::WRITE),
                attribute,
            )
            .unwrap_or_else(|error| panic!("{attribute:?} Frame.Map failed: {:?}", error.code()));
        let (level, pte) = translation::read_leaf(active_ttbr0, ATTRIBUTE_VA);
        assert_eq!(level, 3);
        frame
            .unmap()
            .unwrap_or_else(|error| panic!("Frame.Unmap failed: {:?}", error.code()));
        (pte >> 2) & 0b111
    };
    let refuses = |frame: &FrameKey, attribute: FrameAttribute| {
        assert!(
            matches!(
                frame.map(
                    boot_as_key,
                    ATTRIBUTE_VA,
                    Rights(Rights::READ | Rights::WRITE),
                    attribute
                ),
                Err(CapError::InvalidOperation)
            ),
            "{attribute:?} must be refused"
        );
    };
    let retype_frame = |from: &UntypedKey, ceiling: FrameAttribute, slot: u32| -> FrameKey {
        let key = from
            .retype_with_ceiling(ObjectType::FRAME, 12, ceiling, 1, self_table, slot, rights)
            .unwrap_or_else(|error| panic!("{ceiling:?} Frame Retype failed: {:?}", error.code()));
        FrameKey::from_key(key)
    };

    // Register Untypeds: the strictest ceiling. Use the first with room left
    // (the PL011's has none after the check above carved up to its page); its
    // one Frame is the strict one.
    let strict_slot = take();
    let (register, strict) = devices::device_untyped_extents(keytable_addr)
        .filter(|&(key, _, _)| !devices::is_video_untyped(key))
        .find_map(|(key, _, _)| {
            let untyped = UntypedKey::from_key(key);
            untyped
                .retype_with_ceiling(
                    ObjectType::FRAME,
                    12,
                    FrameAttribute::Default,
                    1,
                    self_table,
                    strict_slot,
                    rights,
                )
                .ok()
                .map(|frame| (untyped, FrameKey::from_key(frame)))
        })
        .expect("a register Untyped with room for a Frame");
    refuses(&strict, WriteCombining);
    refuses(&strict, DevicePosted);
    refuses(&strict, Cacheable);
    assert_eq!(mapped_index(&strict, DeviceStrict), 3);
    assert_eq!(mapped_index(&strict, FrameAttribute::Default), 3);
    // A ceiling never rises: looser than the source's, `Cacheable` and an
    // undefined code are refused, and nothing is carved.
    for ceiling in [DevicePosted, WriteCombining, Cacheable] {
        invalid(register.retype_with_ceiling(
            ObjectType::FRAME,
            12,
            ceiling,
            1,
            self_table,
            take(),
            rights,
        ));
    }
    invalid(register.retype(ObjectType::FRAME, 12, 9, 1, self_table, take(), rights));

    // Video memory: write-combining ceiling, when the board has any.
    if let Some(video_key) = devices::first_video_untyped() {
        let video = UntypedKey::from_key(video_key);
        let combining = retype_frame(&video, FrameAttribute::Default, take());
        assert_eq!(mapped_index(&combining, WriteCombining), 1);
        assert_eq!(mapped_index(&combining, DevicePosted), 2);
        assert_eq!(mapped_index(&combining, DeviceStrict), 3);
        assert_eq!(mapped_index(&combining, FrameAttribute::Default), 3);
        refuses(&combining, Cacheable);
        // Write-combining mappings are never executable.
        assert!(matches!(
            combining.map(
                boot_as_key,
                ATTRIBUTE_VA,
                Rights(Rights::READ | Rights::EXECUTE),
                WriteCombining
            ),
            Err(CapError::InvalidOperation)
        ));
        frame_cache_maintenance_is_refused(&combining);
        // The mapping's descriptor is non-executable.
        combining
            .map(
                boot_as_key,
                ATTRIBUTE_VA,
                Rights(Rights::READ | Rights::WRITE),
                WriteCombining,
            )
            .unwrap_or_else(|error| panic!("write-combining Frame.Map: {:?}", error.code()));
        let (_, pte) = translation::read_leaf(active_ttbr0, ATTRIBUTE_VA);
        assert_eq!(pte & ((1 << 53) | (1 << 54)), (1 << 53) | (1 << 54));
        combining
            .unmap()
            .unwrap_or_else(|error| panic!("Frame.Unmap failed: {:?}", error.code()));

        // CopyDerive keeps the ceiling.
        let copy_slot = take();
        let copy = self_table
            .copy_derive(combining.raw(), self_table, copy_slot, rights)
            .unwrap_or_else(|error| panic!("Frame CopyDerive failed: {:?}", error.code()));
        assert_eq!(mapped_index(&FrameKey::from_key(copy), WriteCombining), 1);

        // Lowering at Retype: posted writes at most.
        let posted = retype_frame(&video, DevicePosted, take());
        refuses(&posted, WriteCombining);
        assert_eq!(mapped_index(&posted, DevicePosted), 2);
        assert_eq!(mapped_index(&posted, DeviceStrict), 3);

        // A split child with a lowered ceiling passes it to its Frames.
        let child_slot = take();
        let child = UntypedKey::from_key(
            video
                .retype_with_ceiling(
                    ObjectType::UNTYPED,
                    16,
                    DeviceStrict,
                    1,
                    self_table,
                    child_slot,
                    Rights::all(),
                )
                .unwrap_or_else(|error| panic!("child Untyped Retype failed: {:?}", error.code())),
        );
        let from_child = retype_frame(&child, FrameAttribute::Default, take());
        refuses(&from_child, DevicePosted);
        assert_eq!(mapped_index(&from_child, DeviceStrict), 3);
        // A split without a ceiling inherits the source's.
        let plain_child = UntypedKey::from_key(
            video
                .retype(
                    ObjectType::UNTYPED,
                    16,
                    0,
                    1,
                    self_table,
                    take(),
                    Rights::all(),
                )
                .unwrap_or_else(|error| panic!("child Untyped Retype failed: {:?}", error.code())),
        );
        let inherited = retype_frame(&plain_child, FrameAttribute::Default, take());
        assert_eq!(mapped_index(&inherited, WriteCombining), 1);
    }

    // RAM has no ceiling: a ceiling on a RAM source is refused, and a RAM Frame
    // maps write-back only.
    invalid(ram.retype_with_ceiling(
        ObjectType::FRAME,
        12,
        DeviceStrict,
        1,
        self_table,
        take(),
        rights,
    ));
    let ram_frame = retype_frame(ram, FrameAttribute::Default, take());
    for attribute in [WriteCombining, DevicePosted, DeviceStrict] {
        refuses(&ram_frame, attribute);
    }
    assert_eq!(mapped_index(&ram_frame, Cacheable), 0);
    assert_eq!(mapped_index(&ram_frame, FrameAttribute::Default), 0);
    semi::println!("Frame attribute ceilings checked");
}

/// A device Frame refuses cache maintenance whatever its attribute: a
/// non-cacheable mapping has nothing to clean or invalidate.
fn frame_cache_maintenance_is_refused(frame: &FrameKey) {
    assert!(matches!(
        frame.clean_data(0, 0x1000),
        Err(CapError::InvalidOperation)
    ));
}

/// Boot-table slot for the read-only Frame copy the cache-maintenance check
/// derives.
const READ_ONLY_FRAME_SLOT: u32 = 240;

/// One `FrameKey` cache-maintenance wrapper.
type CacheOperation = fn(&FrameKey, u64, u64) -> Result<(), CapError>;

/// Frame cache maintenance on an unmapped RAM frame whose first word is
/// `first_word`: every operation succeeds over the whole frame without a
/// mapping, a partial-line invalidate keeps the bytes outside its range, the
/// range must be nonempty and inside the frame, and invalidating needs
/// `WRITE` where cleaning needs only `READ`.
fn frame_cache_maintenance(
    self_table: &KeyTableKey,
    frame_key: RawKey,
    frame_paddr: u64,
    first_word: u64,
) {
    let frame = FrameKey::from_key(frame_key);
    let operations: [(&str, CacheOperation); 4] = [
        ("CleanData", FrameKey::clean_data),
        ("InvalidateData", FrameKey::invalidate_data),
        ("CleanInvalidateData", FrameKey::clean_invalidate_data),
        ("UnifyInstruction", FrameKey::unify_instruction),
    ];
    for (name, operation) in operations {
        operation(&frame, 0, 0x1000)
            .unwrap_or_else(|error| panic!("Frame.{name} failed: {:?}", error.code()));
        for (start, end) in [
            (0, 0),
            (0x40, 0x40),
            (0x80, 0x40),
            (0, 0x1001),
            (0x1000, 0x1001),
        ] {
            assert!(
                matches!(
                    operation(&frame, start, end),
                    Err(CapError::InvalidOperation)
                ),
                "Frame.{name} accepted {start:#x}..{end:#x}"
            );
        }
    }
    // The first word lies in a line only partly inside 8..0x48: it is
    // cleaned, not discarded.
    frame
        .invalidate_data(8, 0x48)
        .unwrap_or_else(|error| panic!("partial Frame.InvalidateData failed: {:?}", error.code()));
    // SAFETY: the kernel direct map covers the frame's RAM; nothing maps or
    // writes it meanwhile.
    let observed = unsafe {
        (PhysAddr::new(frame_paddr).user_to_kernel().as_u64() as *const u64).read_volatile()
    };
    assert_eq!(
        observed, first_word,
        "bytes outside the invalidated range survive"
    );

    let read_only = FrameKey::from_key(
        self_table
            .copy_derive(
                frame_key,
                self_table,
                READ_ONLY_FRAME_SLOT,
                Rights(Rights::READ),
            )
            .unwrap_or_else(|error| {
                panic!("read-only Frame CopyDerive failed: {:?}", error.code())
            }),
    );
    read_only
        .clean_data(0, 0x1000)
        .unwrap_or_else(|error| panic!("read-only Frame.CleanData failed: {:?}", error.code()));
    assert!(matches!(
        read_only.invalidate_data(0, 0x1000),
        Err(CapError::InsufficientRights)
    ));
    semi::println!("Frame cache maintenance: four operations, ranges and rights checked");
}

/// First boot-table slot of the `AddressSpace.Bind` checks.
const BIND_SLOT: u32 = 140;
/// The guard of the tables the Bind checks carve (16 entries, so it must fit
/// 28 bits).
const BIND_TABLE_GUARD: u32 = 0x0AB_CDE;

/// `AddressSpace` creation end to end: Retype creates inert, unbound
/// `AddressSpace`s, which refuse activation, Invocation construction and
/// Threads with `NotBound` but accept translation setup; `AddressSpace.Bind`
/// checks its authority and the table's self capability, binds once, and lets
/// two `AddressSpace`s share one table.
fn address_space_bind(
    builder: &Builder<'_>,
    nucleus: &Nucleus<ArchObjectsImpl>,
    root_scheduler_key: RawKey,
    boot_asid_pool: &ASIDPoolKey,
) {
    let (untyped, self_table, keytable_addr) =
        (builder.untyped, builder.self_table, builder.boot_table_addr);
    let slot = |offset: u32| KeySlot(BIND_SLOT + offset).0;
    let retype = |kind: ObjectType, size_bits: u8, guard: u32, count: u32, offset: u32| {
        untyped
            .retype(
                kind,
                size_bits,
                guard,
                count,
                self_table,
                slot(offset),
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("Bind fixture Retype failed: {:?}", error.code()))
    };
    let boot_install = |offset: u32, entry: KeyEntry| {
        // SAFETY: keytable_addr names the live boot KeyTable; each borrow
        // ends before the next SVC.
        unsafe { &mut *KeyTable::at_carve(keytable_addr) }
            .insert(KeySlot(slot(offset)), entry, BOOT_TABLE_GUARD)
            .unwrap_or_else(|failure| {
                panic!("Bind fixture grant failed: {:?}", failure.error.code())
            })
    };

    // Two inert AddressSpaces in one batch, a table that names itself in
    // `SELF_KEYTABLE` and a table that does not.
    let first_key = retype(ObjectType::ADDRESS_SPACE, 0, 0, 2, 0);
    let second_key = boot_key(slot(1), first_key.incarnation());
    let (first, second) = (
        AddressSpaceKey::from_key(first_key),
        AddressSpaceKey::from_key(second_key),
    );
    let first_id = builder.address_space_id(first_key);
    let second_id = builder.address_space_id(second_key);
    assert_ne!(first_id, second_id);
    for id in [first_id, second_id] {
        let address_space = nucleus
            .address_space(id)
            .expect("Retyped AddressSpace missing");
        assert!(address_space.keytable().is_none());
        assert!(address_space.translation_root.is_none());
        assert!(address_space.asid.is_none());
    }
    let table_key = retype(ObjectType::KEY_TABLE, 4, BIND_TABLE_GUARD, 1, 2);
    let table_addr = builder.table_address(table_key);
    // SAFETY: Retype initialized the private carve.
    let table_id = unsafe { KeyTable::carve_id(table_addr) };
    // SAFETY: Retype initialized the private carve; no SVC runs during this
    // borrow.
    unsafe { &mut *KeyTable::at_carve(table_addr) }
        .insert(
            KeySlot::SELF_KEYTABLE,
            KeyEntry::new_keytable(table_id, BIND_TABLE_GUARD, 4, Rights::all(), 0),
            BIND_TABLE_GUARD,
        )
        .unwrap_or_else(|failure| {
            panic!("Bind table self grant failed: {:?}", failure.error.code())
        });
    let table = KeyTableKey::from_key(table_key);
    let selfless_key = retype(ObjectType::KEY_TABLE, 4, BIND_TABLE_GUARD, 1, 3);

    // Unbound: no activation, no Invocation construction, no Threads.
    assert!(matches!(first.activate(), Err(CapError::NotBound)));
    assert!(matches!(
        first.create_invocation(0x1000, self_table, KeySlot(slot(4)), 0, 16, 16),
        Err(CapError::NotBound)
    ));
    let thread_key = retype(ObjectType::THREAD, 0, 0, 1, 5);
    let time_key = retype(ObjectType::TIME, 0, 0, 1, 6);
    let scheduler = SchedulerKey::from_key(root_scheduler_key);
    let start = ThreadStart {
        entry: 0x1000,
        stack_pointer: 0x2000,
        argument: 0,
    };
    assert!(matches!(
        scheduler.configure_thread(thread_key, first_key, start, time_key),
        Err(CapError::NotBound)
    ));

    // Translation setup works before binding: a root and an ASID.
    let root_key = retype(ObjectType::PAGE_TABLE, 12, 0, 1, 7);
    let root = PageTableKey::from_key(root_key);
    root.map(second_key, 0)
        .unwrap_or_else(|error| panic!("unbound root PageTable.Map failed: {:?}", error.code()));
    boot_asid_pool
        .assign(second_key)
        .unwrap_or_else(|error| panic!("unbound ASIDPool.Assign failed: {:?}", error.code()));
    root.unmap()
        .unwrap_or_else(|error| panic!("unbound root PageTable.Unmap failed: {:?}", error.code()));

    // Authority: `MAP` on the AddressSpace, `INSTALL` on the table.
    let no_map = boot_install(
        8,
        KeyEntry::new_carved(first_id, Rights(Rights::all().0 & !Rights::MAP), 0),
    );
    assert!(matches!(
        AddressSpaceKey::from_key(no_map).bind(&table),
        Err(CapError::InsufficientRights)
    ));
    let no_install = boot_install(
        9,
        KeyEntry::new_keytable(
            table_id,
            BIND_TABLE_GUARD,
            4,
            Rights(Rights::all().0 & !Rights::INSTALL),
            0,
        ),
    );
    assert!(matches!(
        first.bind(&KeyTableKey::from_key(no_install)),
        Err(CapError::InsufficientRights)
    ));
    // The table operand must be a KeyTable that names itself.
    assert!(matches!(
        first.bind(&KeyTableKey::from_key(second_key)),
        Err(CapError::TypeMismatch { expected, .. }) if expected == ObjectType::KEY_TABLE
    ));
    assert!(matches!(
        first.bind(&KeyTableKey::from_key(selfless_key)),
        Err(CapError::InvalidSelfReference {
            reference: InvalidSelfReference::KeyTableKey(KeySlot::SELF_KEYTABLE),
            key,
        }) if key == selfless_key
    ));
    assert!(
        nucleus
            .address_space(first_id)
            .expect("AddressSpace missing")
            .keytable()
            .is_none()
    );

    // Bind once; the table gets its Return sentinel.
    first
        .bind(&table)
        .unwrap_or_else(|error| panic!("AddressSpace.Bind failed: {:?}", error.code()));
    let binding = nucleus
        .address_space(first_id)
        .expect("AddressSpace missing")
        .keytable()
        .expect("Bind recorded no table");
    assert_eq!(binding.address(), table_addr);
    assert_eq!(binding.size_bits(), 4);
    assert!(
        // SAFETY: Retype initialized the table'''s private carve; no SVC runs
        // during this borrow.
        unsafe { &*KeyTable::at_carve(table_addr) }
            .lookup(
                table_key_for(BIND_TABLE_GUARD, KeySlot::THREAD_RETURN),
                BIND_TABLE_GUARD
            )
            .is_ok_and(KeyEntry::is_thread_return_key)
    );
    assert!(matches!(first.bind(&table), Err(CapError::AlreadyMapped)));
    // Bound: the AddressSpace is past the binding check (it is not the
    // caller's own, so Activate still refuses), takes Invocations, and a
    // Thread now fails on its missing root instead.
    assert!(matches!(first.activate(), Err(CapError::InvalidOperation)));
    first
        .create_invocation(0x1000, self_table, KeySlot(slot(4)), 0, 16, 16)
        .unwrap_or_else(|error| panic!("bound CreateInvocation failed: {:?}", error.code()));
    assert!(matches!(
        scheduler.configure_thread(thread_key, first_key, start, time_key),
        Err(CapError::NotMapped)
    ));

    // One table may serve several AddressSpaces.
    second
        .bind(&table)
        .unwrap_or_else(|error| panic!("second AddressSpace.Bind failed: {:?}", error.code()));
    assert_eq!(
        nucleus
            .address_space(second_id)
            .and_then(AddressSpaceObject::keytable),
        Some(binding)
    );

    for address_space in [first, second] {
        address_space
            .retire()
            .unwrap_or_else(|error| panic!("Bind fixture Retire failed: {:?}", error.code()));
    }
}

/// The key of `slot` in a table guarded by `guard`, at the sentinel's fixed
/// incarnation.
fn table_key_for(guard: u32, slot: KeySlot) -> RawKey {
    RawKey::from_parts(guard, 4, slot.0, KeySlot::THREAD_RETURN_INCARNATION)
}
