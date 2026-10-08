//! The PPC suite: `AddressSpace.CreateInvocation` construction and its full
//! rejection matrix, then same-Thread `Invocation.Call` / `Thread.Return`
//! round trips from the boot Thread into the Bounce fixture's
//! `AddressSpace` — raw and library wrappers, a compiled `ppc_export!` entry
//! with its Return-fault handler, rejected Returns inside a migrated call, and
//! live values across deliberate target clobbers.

use {
    crate::ppc,
    aarch64_cpu::registers::{Readable, TCR_EL1},
    core::arch::asm,
    kickstart::bootstrap::{
        BOOT_TABLE_GUARD, BOOT_TABLE_SIZE_BITS, BootState, PoolCapacities, bootstrap_nucleus,
        retained_init_memory,
    },
    libkicktest::{
        bounce::{
            self, TEST_TABLE_GUARD, assert_source_selected, record_fixture_stacks, test_slot,
        },
        builder::Builder,
        keys::{boot_key, boot_slot},
        paging::image_table_count,
        translation,
    },
    libobject::{
        CapError, InconsistencyReason, InvalidKeyReason, InvalidStackReason, KeySlot, KeyTableKey,
        ObjectType, RawKey, Rights, UntypedKey, address_space::AddressSpaceKey,
        thread::ThreadReturnKey,
    },
    libqemu::semihosting as semi,
    nucleus::{
        api::key_entry::KeyEntry,
        objects::{
            ArchObjects, ArchObjectsImpl, KeyTable, Nucleus, Thread, access::ObjectId,
            arch_objects::AddressSpaceObject,
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

fn assert_invalid_stack(
    result: Result<RawKey, CapError>,
    value: u64,
    reason: InvalidStackReason,
    reason_id: u64,
) {
    let error = match result {
        Err(error) => error,
        Ok(key) => panic!("malformed stack installed key {:#x}", key.to_wire()),
    };
    assert!(
        matches!(
            &error,
            CapError::InvalidStack { value: actual, reason: actual_reason }
                if *actual == value && *actual_reason == reason
        ),
        "wrong stack diagnostic: {:?}",
        error.code()
    );
    assert_eq!(error.code(), (32, value, reason_id));
}

/// Snapshot only initialized semantic fields, never union padding. Checked
/// lookup checks the occupied slot's incarnation; `NeverIssued` plus `check_insert`
/// checks the vacant slot's zero counter and absence of an installed payload.
#[derive(Debug, PartialEq, Eq)]
struct InvocationDestinationState {
    count: usize,
    target: ObjectId,
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
    let table = unsafe { &*(table_addr as *const KeyTable) };
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
    // The boot Thread only; the boot and Bounce AddressSpaces; the source and
    // Bounce root prefixes, both image closures and both probe tables.
    let boot = bootstrap_nucleus(&PoolCapacities {
        threads: 1,
        address_spaces: 2,
        notifications: 0,
        event_counts: 0,
        page_tables: 8 + 2 * image_table_count(&retained_init),
        asid_pools: 1,
    });
    let BootState {
        nucleus,
        keytable_addr,
        boot_as_id,
        self_table_key,
        boot_untyped_key,
        debug_console_key,
    } = boot;
    let boot_table_binding = nucleus
        .pools
        .arch
        .address_spaces
        .get_live(usize::from(boot_as_id.index))
        .expect("boot AddressSpace missing")
        .keytable();
    let untyped = UntypedKey::from_key(boot_untyped_key);
    let self_table = KeyTableKey::from_key(self_table_key);
    let builder = Builder {
        untyped: &untyped,
        self_table: &self_table,
        boot_table_addr: keytable_addr,
        retained: &retained_init,
    };
    // A second table for exports into a KeyTable other than the caller's.
    let new_table_key = untyped
        .retype(
            ObjectType::KEY_TABLE,
            8,
            TEST_TABLE_GUARD,
            1,
            &self_table,
            KeySlot(6).0,
            Rights::all(),
        )
        .unwrap_or_else(|error| panic!("export KeyTable Retype failed: {:?}", error.code()));
    let source = bounce::provision_source(&builder);
    let boot_as_key = source.boot_as_key;
    let boot_as = AddressSpaceKey::from_key(boot_as_key);
    let bound_asid = source.asid;

    // ─────────────────────────────────────────────────────────────────
    // AddressSpace.CreateInvocation installs a CALL-only capability into
    // the selected KeyTable without validating the supplied entry address.
    // These are numeric low/user-range contracts, not mapped stacks. The
    // existing high direct-map Bounce SP is not a valid PPC stack fixture.
    assert_eq!(TCR_EL1.get() & 0x3F, 16, "the fixture uses 48-bit TTBR0");
    let user_end_exclusive = 1_u64 << 48;
    let invocation_stack = [0x1000, 0x2000, 0x40];
    let invocation_key = boot_as
        .create_invocation(
            0x1234,
            &self_table,
            KeySlot(62),
            invocation_stack[0],
            invocation_stack[1],
            invocation_stack[2],
        )
        .unwrap_or_else(|error| panic!("AddressSpace.CreateInvocation failed: {:?}", error.code()));
    assert_eq!(invocation_key.slot(), boot_slot(62));
    assert_ne!(invocation_key.incarnation(), 0);
    {
        // SAFETY: keytable_addr names the live carved boot KeyTable.
        let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
        let Ok(invocation) = boot_table.lookup(invocation_key, BOOT_TABLE_GUARD) else {
            panic!("CreateInvocation did not install its destination cap");
        };
        assert_eq!(invocation.object_type(), ObjectType::INVOCATION);
        assert_eq!(invocation.rights(), Rights(Rights::CALL));
        let Ok((target, function_address)) = invocation.invocation_target() else {
            panic!("installed entry is not an Invocation");
        };
        assert_eq!(target, boot_as_id);
        assert_eq!(function_address.get(), 0x1234);
        let extent = invocation
            .invocation_stack_extent()
            .unwrap_or_else(|error| {
                panic!("installed Invocation extent missing: {:?}", error.code())
            });
        assert_eq!(
            [extent.base(), extent.end(), extent.minimum_headroom()],
            invocation_stack
        );
    }
    let before_rejections =
        invocation_destination_state(keytable_addr, invocation_key, KeySlot(63));

    // Invocation always has a nonzero entry. A zero function address
    // cannot construct an Invocation (or current-relative Thread authority)
    // from userspace. Once authority/live identity are established, zero
    // wins over malformed extents and destination readiness.
    let table_len_before_zero = before_rejections.count;
    for slot in [KeySlot(62), KeySlot(63), KeySlot(300)] {
        for stack in [invocation_stack, [0x1001, 0x1001, 0]] {
            assert!(matches!(
                boot_as.create_invocation(0, &self_table, slot, stack[0], stack[1], stack[2]),
                Err(CapError::InvalidPointer)
            ));
            assert_eq!(
                invocation_destination_state(keytable_addr, invocation_key, KeySlot(63)),
                before_rejections
            );
        }
    }
    assert_eq!(
        create_invocation_raw(
            boot_as_key,
            [0, self_table_key.to_wire(), u64::MAX, 0x1001, 0x1001, 0]
        ),
        CapError::InvalidPointer.code()
    );
    {
        // SAFETY: keytable_addr names the live carved boot KeyTable.
        let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
        assert_eq!(boot_table.len(), table_len_before_zero);
        let existing = boot_table
            .lookup(invocation_key, BOOT_TABLE_GUARD)
            .unwrap_or_else(|error| panic!("zero-address rejection lost cap: {:?}", error.code()));
        let target = existing
            .invocation_target()
            .unwrap_or_else(|error| panic!("Invocation payload changed: {:?}", error.code()));
        assert_eq!(target.0, boot_as_id);
        assert_eq!(target.1.get(), 0x1234);
        assert_eq!(existing.rights(), Rights(Rights::CALL));
        assert!(matches!(
            boot_table.lookup(boot_key(63, 1), BOOT_TABLE_GUARD),
            Err(CapError::InvalidKey {
                reason: InvalidKeyReason::NeverIssued,
                ..
            })
        ));
    }

    // All construction reasons, literal wire IDs, offending submitted
    // values, and first-error precedence go through the real SVC path.
    // Repeat against occupied, never-issued and out-of-bounds destinations;
    // a full-width unrepresentable slot must likewise lose to InvalidStack.
    let malformed_stacks = [
        (
            [0x1000, 0x1000, 0x10],
            InvalidStackReason::ExtentEmpty,
            1,
            0x1000,
        ),
        (
            [0x1001, 0x1001, 0],
            InvalidStackReason::ExtentEmpty,
            1,
            0x1001,
        ),
        (
            [0x2001, 0x1001, 0],
            InvalidStackReason::ExtentInverted,
            2,
            0x1001,
        ),
        (
            [user_end_exclusive + 1, user_end_exclusive + 0x101, 0],
            InvalidStackReason::BaseOutsideUserRange,
            3,
            user_end_exclusive + 1,
        ),
        (
            [
                libaddress::PHYSICAL_KERNEL_WINDOW,
                libaddress::PHYSICAL_KERNEL_WINDOW + 0x1000,
                0x10,
            ],
            InvalidStackReason::BaseOutsideUserRange,
            3,
            libaddress::PHYSICAL_KERNEL_WINDOW,
        ),
        (
            [0x1001, user_end_exclusive + 1, 0],
            InvalidStackReason::EndOutsideUserRange,
            4,
            user_end_exclusive + 1,
        ),
        (
            [0x1001, 0x2001, 0],
            InvalidStackReason::BaseMisaligned,
            5,
            0x1001,
        ),
        (
            [0x1000, 0x2001, 0],
            InvalidStackReason::EndMisaligned,
            6,
            0x2001,
        ),
        (
            [0x1000, 0x2000, 0],
            InvalidStackReason::MinimumHeadroomZero,
            7,
            0,
        ),
        (
            [0x1000, 0x1030, 0x41],
            InvalidStackReason::MinimumHeadroomMisaligned,
            8,
            0x41,
        ),
        (
            [0x1000, 0x1030, 0x40],
            InvalidStackReason::MinimumHeadroomTooLarge,
            9,
            0x40,
        ),
    ];
    for (stack, reason, reason_id, value) in malformed_stacks {
        for slot in [KeySlot(62), KeySlot(63), KeySlot(300)] {
            assert_invalid_stack(
                boot_as.create_invocation(0x5678, &self_table, slot, stack[0], stack[1], stack[2]),
                value,
                reason,
                reason_id,
            );
            assert_eq!(
                invocation_destination_state(keytable_addr, invocation_key, KeySlot(63)),
                before_rejections
            );
        }
        assert_eq!(
            create_invocation_raw(
                boot_as_key,
                [
                    0x5678,
                    self_table_key.to_wire(),
                    u64::MAX,
                    stack[0],
                    stack[1],
                    stack[2]
                ]
            ),
            (32, value, reason_id)
        );
        assert_eq!(
            invocation_destination_state(keytable_addr, invocation_key, KeySlot(63)),
            before_rejections
        );
    }

    // Export into a separate KeyTable as well; the result key must carry
    // that destination table's guard rather than the caller table's guard.
    // Implementation status: the destination is only capability storage;
    // it does not replace the target AddressSpace's provisioned keytable.
    let export_table_addr = {
        // SAFETY: keytable_addr names the live carved boot KeyTable.
        let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
        let Ok(export_table_cap) = boot_table.lookup(new_table_key, BOOT_TABLE_GUARD) else {
            panic!("export KeyTable capability is missing");
        };
        let Ok(address) = export_table_cap.keytable_address() else {
            panic!("export destination is not a KeyTable");
        };
        address
    };
    let export_stack = [0x3010, 0x30B0, 0x30];
    let cross_table_invocation = boot_as
        .create_invocation(
            0x5678,
            &KeyTableKey::from_key(new_table_key),
            KeySlot(3),
            export_stack[0],
            export_stack[1],
            export_stack[2],
        )
        .unwrap_or_else(|error| panic!("cross-table CreateInvocation failed: {:?}", error.code()));
    assert_eq!(cross_table_invocation.slot(), test_slot(3));
    {
        // SAFETY: export_table_addr came from its live kernel-issued cap.
        let export_table = unsafe { &*(export_table_addr as *const KeyTable) };
        let Ok(invocation) = export_table.lookup(cross_table_invocation, TEST_TABLE_GUARD) else {
            panic!("cross-table Invocation key did not resolve in its target table");
        };
        let Ok((target, function_address)) = invocation.invocation_target() else {
            panic!("cross-table export is not an Invocation");
        };
        assert_eq!(target, boot_as_id);
        assert_eq!(function_address.get(), 0x5678);
        let extent = invocation
            .invocation_stack_extent()
            .unwrap_or_else(|error| {
                panic!("cross-table Invocation extent missing: {:?}", error.code())
            });
        assert_eq!(
            [extent.base(), extent.end(), extent.minimum_headroom()],
            export_stack
        );
        assert_eq!(invocation.rights(), Rights(Rights::CALL));
        assert_ne!(export_table_addr, boot_table_binding.address());
        let target_table = nucleus
            .pools
            .arch
            .address_spaces
            .get_live(usize::from(target.index))
            .expect("Invocation target AddressSpace missing")
            .keytable();
        assert_eq!(target_table.address(), boot_table_binding.address());
        assert_eq!(target_table.size_bits(), boot_table_binding.size_bits());
    }
    // Equality, sub-page/non-page-aligned boundaries, non-power-of-two
    // extent/minimum, zero base, and the exclusive user ceiling are valid.
    // No mapping walk or executable-entry validation is implied by success.
    for (slot, stack) in [
        (KeySlot(4), [0x4010, 0x40A0, 0x90]),
        (
            KeySlot(5),
            [user_end_exclusive - 0x30, user_end_exclusive, 0x30],
        ),
        (KeySlot(6), [0, 0x30, 0x10]),
    ] {
        let response = create_invocation_raw(
            boot_as_key,
            [
                0x6789,
                new_table_key.to_wire(),
                u64::from(slot.0),
                stack[0],
                stack[1],
                stack[2],
            ],
        );
        assert_eq!(response.0, 0, "valid stack rejected: {response:?}");
        assert_eq!(response.2, 0, "construction success has one key result");
        let key = RawKey::from_wire(response.1);
        assert_eq!(key.slot(), test_slot(slot.0));
        assert_eq!(key.incarnation(), 1);
        // SAFETY: retained initialized export table, borrowed only after
        // SVC completion; no SVC/mutation occurs while this borrow is live.
        let table = unsafe { &*(export_table_addr as *const KeyTable) };
        let entry = table
            .lookup(key, TEST_TABLE_GUARD)
            .unwrap_or_else(|error| panic!("valid stack cap missing: {:?}", error.code()));
        let extent = entry
            .invocation_stack_extent()
            .unwrap_or_else(|error| panic!("valid stack payload missing: {:?}", error.code()));
        assert_eq!(
            [extent.base(), extent.end(), extent.minimum_headroom()],
            stack
        );
        assert!(matches!(
            entry.invocation_target(),
            Ok((target, function)) if target == boot_as_id && function.get() == 0x6789
        ));
        assert_eq!(entry.rights(), Rights(Rights::CALL));
    }
    assert!(matches!(
        boot_as.create_invocation(0x5678, &self_table, KeySlot(62), 0x5010, 0x50A0, 0x30),
        Err(CapError::SlotOccupied(KeySlot(62)))
    ));
    assert!(matches!(
        boot_as.create_invocation(0x5678, &self_table, KeySlot(300), 0x5010, 0x50A0, 0x30),
        Err(CapError::InvalidSlot(KeySlot(300)))
    ));
    assert_eq!(
        create_invocation_raw(
            boot_as_key,
            [
                0x5678,
                self_table_key.to_wire(),
                u64::MAX,
                0x5010,
                0x50A0,
                0x30
            ]
        ),
        CapError::InvalidKey {
            key: self_table_key,
            reason: InvalidKeyReason::SlotOutOfRange,
            operand: 4,
        }
        .code()
    );
    assert!(matches!(
        boot_as.create_invocation(
            0x5678,
            &KeyTableKey::from_key(boot_untyped_key),
            KeySlot(63),
            0x1000,
            0x2000,
            0x40,
        ),
        Err(CapError::TypeMismatch { .. })
    ));

    assert!(matches!(
        boot_as.create_invocation(
            0, &KeyTableKey::from_key(boot_untyped_key), KeySlot(63), 0x1001, 0x1001, 0,
        ),
        Err(CapError::TypeMismatch { expected, found })
            if expected == ObjectType::KEY_TABLE && found == ObjectType::UNTYPED
    ));
    assert_eq!(
        invocation_destination_state(keytable_addr, invocation_key, KeySlot(63)),
        before_rejections
    );

    // GRANT on the source AddressSpace and INSTALL on the destination
    // KeyTable are independently enforced by the kernel.
    // SAFETY: keytable_addr names the live carved boot KeyTable.
    let no_grant_address_space = unsafe { &mut *(keytable_addr as *mut KeyTable) }
        .insert(
            KeySlot(63),
            KeyEntry::new::<<ArchObjectsImpl as ArchObjects>::AddressSpace>(
                boot_as_id,
                Rights(Rights::MAP),
                0,
            ),
            BOOT_TABLE_GUARD,
        )
        .unwrap_or_else(|failure| {
            panic!(
                "restricted AddressSpace fixture failed: {:?}",
                failure.error.code()
            )
        });
    assert_eq!(no_grant_address_space.incarnation(), 1);
    assert!(matches!(
        AddressSpaceKey::from_key(no_grant_address_space).create_invocation(
            0x5678,
            &self_table,
            KeySlot(64),
            0x1000,
            0x2000,
            0x40,
        ),
        Err(CapError::InsufficientRights)
    ));
    // SAFETY: keytable_addr names the live carved boot KeyTable.
    let no_install_table = unsafe { &mut *(keytable_addr as *mut KeyTable) }
        .insert(
            KeySlot(64),
            KeyEntry::new_keytable(
                keytable_addr,
                BOOT_TABLE_GUARD,
                BOOT_TABLE_SIZE_BITS,
                Rights(Rights::DERIVE),
                0,
            ),
            BOOT_TABLE_GUARD,
        )
        .unwrap_or_else(|failure| {
            panic!(
                "restricted KeyTable fixture failed: {:?}",
                failure.error.code()
            )
        });
    assert_eq!(no_install_table.incarnation(), 1);
    let before_authority_rejections =
        invocation_destination_state(keytable_addr, invocation_key, KeySlot(65));
    assert!(matches!(
        boot_as.create_invocation(
            0x5678,
            &KeyTableKey::from_key(no_install_table),
            KeySlot(65),
            0x1000,
            0x2000,
            0x40,
        ),
        Err(CapError::InsufficientRights)
    ));
    {
        // SAFETY: keytable_addr names the live carved boot KeyTable.
        let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
        assert!(
            boot_table.check_insert(KeySlot(65)).is_ok(),
            "failed INSTALL check modified the destination table"
        );
    }

    // Both independent authority failures precede zero function, invalid
    // extent, occupied destination and full-width slot diagnostics.
    for function in [0, 0x5678] {
        for slot in [KeySlot(62), KeySlot(65)] {
            assert!(matches!(
                AddressSpaceKey::from_key(no_grant_address_space).create_invocation(
                    function,
                    &self_table,
                    slot,
                    0x1001,
                    0x1001,
                    0,
                ),
                Err(CapError::InsufficientRights)
            ));
            assert!(matches!(
                boot_as.create_invocation(
                    function,
                    &KeyTableKey::from_key(no_install_table),
                    slot,
                    0x1001,
                    0x1001,
                    0,
                ),
                Err(CapError::InsufficientRights)
            ));
            assert_eq!(
                invocation_destination_state(keytable_addr, invocation_key, KeySlot(65)),
                before_authority_rejections
            );
        }
        for (source, destination) in [
            (no_grant_address_space, self_table_key),
            (boot_as_key, no_install_table),
        ] {
            assert_eq!(
                create_invocation_raw(
                    source,
                    [function, destination.to_wire(), u64::MAX, 0x1001, 0x1001, 0]
                ),
                CapError::InsufficientRights.code()
            );
            assert_eq!(
                invocation_destination_state(keytable_addr, invocation_key, KeySlot(65)),
                before_authority_rejections
            );
        }
    }
    {
        // SAFETY: retained boot table; all rejected SVCs have returned and
        // these shared fixture borrows end before the next construction.
        let table = unsafe { &*(keytable_addr as *const KeyTable) };
        let address_space_entry = table
            .lookup(no_grant_address_space, BOOT_TABLE_GUARD)
            .unwrap_or_else(|error| {
                panic!("restricted AddressSpace cap changed: {:?}", error.code())
            });
        assert!(matches!(address_space_entry.object_id(), Ok(id) if id == boot_as_id));
        assert_eq!(address_space_entry.rights(), Rights(Rights::MAP));
        assert_eq!(address_space_entry.badge(), 0);
        let table_entry = table
            .lookup(no_install_table, BOOT_TABLE_GUARD)
            .unwrap_or_else(|error| panic!("restricted KeyTable cap changed: {:?}", error.code()));
        assert_eq!(table_entry.keytable_address().ok(), Some(keytable_addr));
        assert_eq!(
            table_entry.keytable_guard_and_size().ok(),
            Some((BOOT_TABLE_GUARD, BOOT_TABLE_SIZE_BITS))
        );
        assert_eq!(table_entry.rights(), Rights(Rights::DERIVE));
        assert_eq!(table_entry.badge(), 0);
    }
    let after_rejections = boot_as
        .create_invocation(0x789A, &self_table, KeySlot(65), 0x6010, 0x60A0, 0x30)
        .unwrap_or_else(|error| panic!("post-rejection install failed: {:?}", error.code()));
    assert_eq!(
        after_rejections.incarnation(),
        1,
        "failed construction burned an incarnation"
    );
    {
        // SAFETY: retained boot table, borrowed after construction commits.
        let table = unsafe { &*(keytable_addr as *const KeyTable) };
        let entry = table
            .lookup(after_rejections, BOOT_TABLE_GUARD)
            .unwrap_or_else(|error| panic!("post-rejection cap missing: {:?}", error.code()));
        let extent = entry
            .invocation_stack_extent()
            .unwrap_or_else(|error| panic!("post-rejection extent missing: {:?}", error.code()));
        assert_eq!(
            [extent.base(), extent.end(), extent.minimum_headroom()],
            [0x6010, 0x60A0, 0x30]
        );
        assert!(matches!(entry.invocation_target(), Ok((target, function))
                if target == boot_as_id && function.get() == 0x789A));
    }

    // ── The Bounce AddressSpace: the PPC target ───────────────────────────
    let bounce = bounce::provision(
        &builder,
        nucleus,
        &source,
        boot_table_binding,
        boot_as_id,
        debug_console_key,
        shared_trap_sp,
    );
    let bounce_as_key = bounce.as_key;
    let bounce_table_addr = bounce.table_addr;
    let bounce_debug_console_key = bounce.debug_console_key;
    let source_root = bounce.source_root;

    // Provisioning (`bind_address_space`) installed Bounce's Slot(1)
    // sentinel before its AddressSpace existed. As Bounce's builder, this
    // fixture knows the table's guard and size, so the Return key is
    // deterministic; it hands the key to the component's init, which
    // records it for the export adapter.
    let ppc_return_key = ThreadReturnKey::provisioned(TEST_TABLE_GUARD, BOOT_TABLE_SIZE_BITS).raw();
    assert_eq!(ppc_return_key.slot(), test_slot(KeySlot::THREAD_RETURN.0));
    {
        // SAFETY: Bounce's retained initialized table; this read-only
        // check overlaps no capability invocation.
        let bounce_table = unsafe { &*(bounce_table_addr as *const KeyTable) };
        let entry = bounce_table
            .lookup(ppc_return_key, TEST_TABLE_GUARD)
            .unwrap_or_else(|error| {
                panic!(
                    "provisioned Return key does not resolve: {:?}",
                    error.code()
                )
            });
        assert!(entry.is_thread_return_key());
    }
    ppc::component_init(ppc_return_key, bounce_debug_console_key);
    let ppc_key = AddressSpaceKey::from_key(bounce_as_key)
        .create_invocation(
            ppc::target_entry as *const () as u64,
            &self_table,
            KeySlot(200),
            ppc::STACK_BASE,
            ppc::STACK_END,
            ppc::MINIMUM_HEADROOM,
        )
        .unwrap_or_else(|error| panic!("PPC CreateInvocation failed: {:?}", error.code()));
    let boot_thread_index = usize::try_from(
        nucleus
            .current_thread
            .unwrap_or_else(|| panic!("no current boot Thread")),
    )
    .unwrap_or_else(|_| panic!("boot Thread index out of range"));
    let assert_boot_thread_home = |nucleus: &Nucleus<ArchObjectsImpl>| {
        let boot_thread = nucleus
            .pools
            .threads
            .get_live(boot_thread_index)
            .unwrap_or_else(|| panic!("boot Thread missing"));
        assert!(boot_thread.invocation_stack.is_empty());
        assert_eq!(boot_thread.address_space, boot_as_id);
    };

    // Recoverable rejections through real dispatch: no push, no switch,
    // no scrubbing, source registers intact.
    for (op, sp, expected) in [
        (
            0,
            ppc::STACK_END - 8,
            CapError::InvalidStack {
                value: ppc::STACK_END - 8,
                reason: InvalidStackReason::SpMisaligned,
            },
        ),
        (
            0,
            ppc::STACK_BASE + 16,
            CapError::InvalidStack {
                value: ppc::STACK_BASE + 16,
                reason: InvalidStackReason::SpInsufficientHeadroom,
            },
        ),
        (1, ppc::STACK_END, CapError::InvalidOperation),
    ] {
        ppc::assert_call_rejected(ppc_key, op, sp, expected);
        assert_boot_thread_home(nucleus);
        assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);
    }

    // The real round trip, twice: the same Thread migrates into Bounce's
    // root/ASID and table, Returns through Bounce's sentinel, and resumes
    // here with SUCCESS/r0/r1 and its own x19-x30/SP/NZCV/root.
    // The first pass uses the instrumented raw pair, the second the
    // libobject `InvocationKey::call` / `ThreadReturnKey` wrappers.
    for via_library in [false, true] {
        let (probe_word, target_ttbr) = ppc::round_trip(ppc_key, via_library);
        assert_eq!(target_ttbr, translation::bounce_ttbr());
        assert_ne!(probe_word, 0);
        assert_boot_thread_home(nucleus);
        assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);
    }

    // A compiled `ppc_export!` entry: wrapper → linked body → result
    // spill → Return through the init-recorded key. The second pass makes
    // init record a stale key, so the adapter's Return is rejected and the
    // image's `vesper_thread_return_fault` gets the exact diagnostics and
    // original words, then repairs by retrying Return with the right key.
    let export_key = AddressSpaceKey::from_key(bounce_as_key)
        .create_invocation(
            ppc::export_entry as *const () as u64,
            &self_table,
            KeySlot(201),
            ppc::STACK_BASE,
            ppc::STACK_END,
            ppc::MINIMUM_HEADROOM,
        )
        .unwrap_or_else(|error| panic!("export CreateInvocation failed: {:?}", error.code()));
    for stale_init_key in [false, true] {
        ppc::export_round_trip(
            export_key,
            ppc_return_key,
            bounce_debug_console_key,
            stale_init_key,
        );
        assert_boot_thread_home(nucleus);
        assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);
    }

    // Rejected Returns from inside a migrated call: every bad key, through
    // the library helper and through raw SVC with junk x4..x7, yields the
    // ordinary lookup/operation error and leaves the target running in
    // Bounce on its own stack with the continuation intact; a valid Return
    // then completes the Call. Return on a named Thread needs a named
    // entry in Bounce's table: Thread is not on the CopyDerive allowlist,
    // so the bootstrap grants it kernel-privately, like the N1/N2 grants.
    let bounce_named_thread_key = {
        // SAFETY: both initialized private tables have retained accounted
        // carves; neither borrow survives a capability invocation.
        let boot_thread_id = unsafe { &*(keytable_addr as *const KeyTable) }
            .lookup(boot_key(50, 1), BOOT_TABLE_GUARD)
            .and_then(KeyEntry::object_id)
            .unwrap_or_else(|_| panic!("boot Thread entry missing"));
        // SAFETY: as above; exclusively borrowed for this bootstrap grant.
        unsafe { &mut *(bounce_table_addr as *mut KeyTable) }
            .insert(
                KeySlot(9),
                KeyEntry::new::<Thread>(boot_thread_id, Rights::all(), 0),
                TEST_TABLE_GUARD,
            )
            .unwrap_or_else(|failure| {
                panic!(
                    "Bounce named Thread grant failed: {:?}",
                    failure.error.code()
                )
            })
    };
    let rejection_key = AddressSpaceKey::from_key(bounce_as_key)
        .create_invocation(
            ppc::rejection_entry as *const () as u64,
            &self_table,
            KeySlot(202),
            ppc::STACK_BASE,
            ppc::STACK_END,
            ppc::MINIMUM_HEADROOM,
        )
        .unwrap_or_else(|error| panic!("rejection CreateInvocation failed: {:?}", error.code()));
    let zero_incarnation = RawKey::new(ppc_return_key.slot(), 0);
    let never_issued = RawKey::new(test_slot(10), 1);
    let wrong_guard = RawKey::new(boot_slot(KeySlot::THREAD_RETURN.0), 1);
    let stale = RawKey::new(ppc_return_key.slot(), ppc_return_key.incarnation() + 1);
    ppc::return_rejection_trip(
        rejection_key,
        [
            ppc::RejectedReturn {
                key: zero_incarnation,
                expected: CapError::InvalidKey {
                    key: zero_incarnation,
                    reason: InvalidKeyReason::ZeroIncarnation,
                    operand: 0,
                },
            },
            ppc::RejectedReturn {
                key: never_issued,
                expected: CapError::InvalidKey {
                    key: never_issued,
                    reason: InvalidKeyReason::NeverIssued,
                    operand: 0,
                },
            },
            ppc::RejectedReturn {
                key: wrong_guard,
                expected: CapError::InvalidKey {
                    key: wrong_guard,
                    reason: InvalidKeyReason::GuardMismatch,
                    operand: 0,
                },
            },
            ppc::RejectedReturn {
                key: stale,
                expected: CapError::InconsistentKey {
                    key: stale,
                    reason: InconsistencyReason::SlotIncarnationMismatch,
                    operand: 0,
                },
            },
            ppc::RejectedReturn {
                key: bounce_named_thread_key,
                expected: CapError::InvalidOperation,
            },
        ],
    );
    assert_boot_thread_home(nucleus);
    assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);

    // Wrapper declarations under deliberate target clobbers: the target
    // fills x4..x30 and NZCV with garbage and writes plain memory; the
    // source's compiler-allocated live values must all survive the
    // `InvocationKey::call` wrapper, and the memory write must be seen.
    let clobber_key = AddressSpaceKey::from_key(bounce_as_key)
        .create_invocation(
            ppc::clobber_entry as *const () as u64,
            &self_table,
            KeySlot(203),
            ppc::STACK_BASE,
            ppc::STACK_END,
            ppc::MINIMUM_HEADROOM,
        )
        .unwrap_or_else(|error| panic!("clobber CreateInvocation failed: {:?}", error.code()));
    ppc::live_values_trip(clobber_key);
    assert_boot_thread_home(nucleus);
    assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);
    semi::println!("PPC Call/Return round trips through Bounce passed");
}
