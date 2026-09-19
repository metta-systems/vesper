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
// The binary also allows unused items while these APIs are being brought up.
#[allow(unused)]
#[path = "../src/api/mod.rs"]
mod api;
#[allow(unused)]
#[path = "../src/objects/mod.rs"]
mod objects;

use {
    api::{KeyEntry, debug_console::invoke},
    core::mem::{MaybeUninit, size_of},
    libaddress::PhysAddr,
    libobject::{
        CapError, InconsistencyReason, InvalidKeyReason, KeySlot, ObjectType, RawKey, Rights,
        decode_syscall_result,
        domain::{DcbPage, DomainId},
    },
    objects::{
        ArchObjectsImpl, Domain, KeyTable, Nucleus, ObjectPool,
        access::{ObjectId, PoolTag},
        arch::ArchPools,
        domain::DcbPages,
        nucleus::NucleusPools,
    },
};

// The debug console is a stateless singleton, not a pool object; its entry
// carries a null identity and is validated by type only (see the debug-only
// exception in doc/nucleus_capabilities.md).
fn console_entry(rights: Rights, badge: u16) -> KeyEntry {
    KeyEntry::from_id(
        ObjectType::DEBUG_CONSOLE,
        ObjectId {
            pool: PoolTag::Region,
            index: 0,
            generation: 0,
        },
        rights,
        badge,
    )
}

/// Fixed RAM address for test-carved `KeyTable`s (QEMU rpi3: 1 GiB RAM at 0).
///
/// The test binary loads at `0x80000` and the DTB sits at `0x8000000`; 512 MiB
/// is clear of both. Carving from a fixed address keeps the large `KeyTable`
/// storage out of the test's stack frame.
const TEST_BACKING: u64 = 0x2000_0000;

/// Carve a `KeyTable` into the fixed test backing at `index`, returning its
/// kernel address.
fn carve(index: usize) -> u64 {
    let obj = (TEST_BACKING + (index as u64) * (size_of::<KeyTable>() as u64)) as *mut KeyTable;
    // SAFETY: TEST_BACKING is RAM, aligned for KeyTable, and exclusively owned
    // by the test fixture for its lifetime.
    unsafe {
        obj.write(KeyTable::new(DomainId(0)));
    }
    obj as u64
}

fn with_nucleus(test: impl FnOnce(&mut Nucleus<ArchObjectsImpl>, u64, u64)) {
    let mut dom_backing = MaybeUninit::<[Domain; 2]>::uninit();
    type Pt = objects::arch::AArch64PageTable;
    let mut pt_backing = MaybeUninit::<[Pt; 2]>::uninit();
    // SAFETY: The backings are aligned for their arrays and remain exclusively
    // owned here until after the nucleus and its pools are dropped. The
    // callback cannot return a borrowed nucleus/object reference. Only the
    // pools access the backing while they are live.
    let domains = unsafe {
        ObjectPool::new(
            dom_backing.as_mut_ptr().cast::<u8>(),
            size_of::<[Domain; 2]>(),
        )
    };
    let page_tables =
        unsafe { ObjectPool::new(pt_backing.as_mut_ptr().cast::<u8>(), size_of::<[Pt; 2]>()) };
    // Carve two KeyTable regions from the fixed test backing (mirrors the boot
    // carve / runtime Retype: the table's storage is the carved region).
    let table_addr = carve(0);
    let second_table_addr = carve(1);
    let mut nucleus = Nucleus {
        pools: NucleusPools {
            domains,
            // SAFETY: zero-capacity backing: this test never allocates or
            // invokes Notification objects.
            notifications: unsafe { ObjectPool::new(pt_backing.as_mut_ptr().cast::<u8>(), 0) },
            // SAFETY: zero-capacity backing: this test never allocates or
            // invokes EventCount objects.
            event_counts: unsafe { ObjectPool::new(pt_backing.as_mut_ptr().cast::<u8>(), 0) },
            // SAFETY: the page-table and ASID-pool backings are exclusively
            // owned by this fixture; this test does not allocate or invoke
            // arch objects (the ASID pool has zero capacity for the same
            // reason).
            arch: unsafe {
                ArchPools::new(
                    page_tables,
                    ObjectPool::new(pt_backing.as_mut_ptr().cast::<u8>(), 0),
                )
            },
        },
        current_domain: None,
        dcb_pages: DcbPages::new(),
        pending: crate::objects::PendingPool::new(),
        scheduler: crate::objects::Scheduler::new(),
    };
    test(&mut nucleus, table_addr, second_table_addr);
    for index in 0..2_u16 {
        let id = ObjectId {
            pool: PoolTag::Domain,
            index,
            generation: 1,
        };
        if nucleus.pools.domains.get_live(usize::from(index)).is_some() {
            nucleus
                .pools
                .domains
                .deallocate(id)
                .unwrap_or_else(|_| panic!("domain cleanup failed"));
        }
    }
}

#[test_case]
fn missing_caller_cannot_invoke_bootstrap_console() {
    with_nucleus(|nucleus, table_addr, _second| {
        let key = nucleus
            .create_domain(table_addr)
            .expect("bootstrap console key missing");
        assert_eq!(key.slot(), KeySlot::DEBUG_CONSOLE);
        assert_ne!(key.incarnation(), 0);
        assert_eq!(nucleus.current_domain, None);
        let args = [u64::MAX; 6];

        assert!(nucleus.current_domain_mut().is_none());
        let error = match api::handle_cap_invoke(nucleus, key, 1, &args) {
            Err(error) => error,
            Ok(_) => panic!("invocation without a caller succeeded"),
        };
        assert!(matches!(error, CapError::InvalidDomain));
        let response = error.code();
        assert_eq!(response, (3, 0, 0));
        assert!(matches!(
            decode_syscall_result(response),
            Err(CapError::InvalidDomain)
        ));

        // Explicitly selecting the existing boot fixture preserves its debug
        // path. Invalid op 1 proves dispatch without dereferencing write args.
        nucleus.current_domain = Some(0);
        assert!(matches!(
            api::handle_cap_invoke(nucleus, key, 1, &args),
            Err(CapError::InvalidOperation)
        ));
        nucleus.current_domain = None;
        assert!(matches!(
            api::handle_cap_invoke(nucleus, key, 1, &args),
            Err(CapError::InvalidDomain)
        ));
        // Caller validation also precedes malformed-key validation.
        assert!(matches!(
            api::handle_cap_invoke(nucleus, RawKey::from_wire(0), 0, &args),
            Err(CapError::InvalidDomain)
        ));
    });
}

#[test_case]
fn dispatch_uses_only_the_explicit_allocated_caller_table() {
    with_nucleus(|nucleus, table_addr, second_table_addr| {
        let key = nucleus
            .create_domain(table_addr)
            .expect("bootstrap console key missing");
        let args = [u64::MAX; 6];
        for caller in [1, 2, u32::MAX] {
            nucleus.current_domain = Some(caller);
            assert!(nucleus.current_domain_mut().is_none());
            assert!(matches!(
                api::handle_cap_invoke(nucleus, key, 1, &args),
                Err(CapError::InvalidDomain)
            ));
        }

        nucleus
            .pools
            .domains
            .allocate(Domain {
                keytable_addr: second_table_addr,
                translation_root: None,
                asid: None,
                context: crate::objects::ExecutionContext::Running,
            })
            .expect("second domain allocation failed");
        nucleus.current_domain = Some(1);
        assert!(matches!(
            api::handle_cap_invoke(nucleus, key, 1, &args),
            Err(CapError::InvalidKey {
                key: submitted,
                reason: InvalidKeyReason::NeverIssued,
                operand: 0,
            }) if submitted == key
        ));
        assert_eq!(nucleus.current_domain_table_mut().unwrap().len(), 0);
        let id = ObjectId {
            pool: PoolTag::Domain,
            index: 1,
            generation: 1,
        };
        assert!(nucleus.pools.domains.deallocate(id).is_ok());
        assert!(matches!(
            api::handle_cap_invoke(nucleus, key, 1, &args),
            Err(CapError::InvalidDomain)
        ));
    });
}

#[test_case]
fn missing_caller_cannot_select_an_existing_dcb() {
    // This page is used only by this test, once in the serial QEMU harness.
    // Static backing satisfies DcbPages' retained-reference lifetime.
    static mut PAGE: DcbPage = DcbPage::new();
    with_nucleus(|nucleus, _table_addr, _second| {
        let page = &raw mut PAGE;
        // SAFETY: PAGE is initialized, aligned, static, and exclusively accessed
        // through this DcbPages instance. Tests run with identity-mapped RAM;
        // the recorded physical address is the page's actual address.
        unsafe { nucleus.dcb_pages.add_page(page, PhysAddr::new(page as u64)) }
            .expect("DCB page installation failed");
        let first = nucleus.dcb_pages.allocate_domain(DomainId(0)).unwrap();
        let second = nucleus.dcb_pages.allocate_domain(DomainId(0)).unwrap();
        assert_eq!(first, DomainId(0));
        assert_eq!(second, DomainId(1));
        assert!(nucleus.current_dcb_mut().is_none());
        for id in [first, second] {
            nucleus.current_domain = Some(id.0);
            assert_eq!(nucleus.current_dcb_mut().unwrap().id, id);
        }
        nucleus.current_domain = None;
        assert!(nucleus.current_dcb_mut().is_none());
        nucleus.current_domain = Some(u32::MAX);
        assert!(nucleus.current_dcb_mut().is_none());
    });
}

#[test_case]
fn rejects_wrong_capability_types_before_touching_write_arguments() {
    for cap in [
        KeyEntry::null(),
        KeyEntry::new_untyped(0, 12, false, Rights::all()),
        KeyEntry::new_frame(0, 12, false, Rights::all()),
    ] {
        for op in [0, u64::from(u32::MAX), u64::MAX] {
            assert!(matches!(
                invoke(&cap, op, u64::MAX, u64::MAX),
                Err(CapError::TypeMismatch { expected, found })
                    if expected == ObjectType::DEBUG_CONSOLE && found == cap.object_type()
            ));
        }
    }
}

#[test_case]
fn rejects_invalid_operations_through_shared_capability_borrows() {
    let cap = console_entry(Rights::all(), 42);
    let alias = &cap;

    // Invalid opcodes must fail before constructing an address or copying bytes.
    for op in [1, 127, 255, 256, 1 << 16, u64::from(u32::MAX), u64::MAX]
        .into_iter()
        .chain((0..64).map(|bit| 1_u64 << bit))
    {
        assert!(matches!(
            invoke(&cap, op, u64::MAX, u64::MAX),
            Err(CapError::InvalidOperation)
        ));
        assert!(matches!(
            invoke(alias, op, u64::MAX, u64::MAX),
            Err(CapError::InvalidOperation)
        ));
    }
    assert_eq!(cap.object_type(), ObjectType::DEBUG_CONSOLE);
    assert_eq!(cap.rights(), Rights::all());
    assert_eq!(cap.badge(), 42);
    assert_eq!(
        cap.object_id()
            .unwrap_or_else(|_| panic!("no identity"))
            .generation,
        0
    );
}

// Check every slot through the public API, including retained identity on deletion.
// Only console metadata is inspected; no object pointer or write buffer is accessed.
fn assert_console_table(table: &KeyTable, key: RawKey, live: Option<(Rights, u16)>) {
    assert_eq!(table.len(), usize::from(live.is_some()));
    match live {
        Some((rights, badge)) => {
            let cap = table
                .lookup(key)
                .unwrap_or_else(|_| panic!("console key changed"));
            assert_eq!(cap.object_type(), ObjectType::DEBUG_CONSOLE);
            assert_eq!(cap.rights(), rights);
            assert_eq!(cap.badge(), badge);
            assert_eq!(
                cap.object_id()
                    .unwrap_or_else(|_| panic!("no identity"))
                    .generation,
                0
            );
        }
        None => assert!(matches!(
            table.lookup(key),
            Err(CapError::InconsistentKey {
                key: submitted,
                reason: InconsistencyReason::CapabilityInvalidated,
                operand: 0,
            }) if submitted == key
        )),
    }
    for index in 0..KeyTable::NUM_SLOTS {
        let slot = KeySlot(u32::try_from(index).unwrap());
        if slot != key.slot() {
            let probe = RawKey::new(slot, 1);
            assert!(matches!(
                table.lookup(probe),
                Err(CapError::InvalidKey {
                    key: submitted,
                    reason: InvalidKeyReason::NeverIssued,
                    operand: 0,
                }) if submitted == probe
            ));
        }
    }
}

fn assert_dispatch_error(
    nucleus: &mut Nucleus<ArchObjectsImpl>,
    key: RawKey,
    op: u64,
    expected: (u64, u64, u64),
) {
    // An erroneous Write dispatch must not get as far as copying these arguments.
    let error = match api::handle_cap_invoke(nucleus, key, op, &[u64::MAX; 6]) {
        Err(error) => error,
        Ok(_) => panic!("rejected invocation succeeded"),
    };
    assert_eq!(error.code(), expected);
}

#[test_case]
fn malformed_dispatch_keys_encode_literal_error_words_without_changing_table() {
    with_nucleus(|nucleus, table_addr, _second| {
        let issued = nucleus
            .create_domain(table_addr)
            .expect("bootstrap console key missing");
        nucleus.current_domain = Some(0);
        // Literal wire words deliberately avoid deriving expectations from the
        // encoder under test. Zero incarnation wins even over out-of-range slots;
        // a never-issued slot wins over an arbitrary nonzero incarnation.
        for (wire, reason, details) in [
            (0x0000_0000_0000_0000, InvalidKeyReason::ZeroIncarnation, 1),
            (0x0000_0000_0000_007f, InvalidKeyReason::ZeroIncarnation, 1),
            (0x0000_0000_ffff_ffff, InvalidKeyReason::ZeroIncarnation, 1),
            (0x0000_0001_ffff_ffff, InvalidKeyReason::SlotOutOfRange, 2),
            (0x0000_0001_8000_007f, InvalidKeyReason::SlotOutOfRange, 2),
            (0xffff_ffff_ffff_ffff, InvalidKeyReason::SlotOutOfRange, 2),
            (0x0000_0001_0000_0000, InvalidKeyReason::NeverIssued, 3),
            (0xffff_ffff_0000_0000, InvalidKeyReason::NeverIssued, 3),
        ] {
            let key = RawKey::from_wire(wire);
            let words = (26, wire, details);
            for op in [0, u64::MAX] {
                assert_dispatch_error(nucleus, key, op, words);
                assert_console_table(
                    nucleus.current_domain_table_mut().unwrap(),
                    issued,
                    Some((Rights::all(), 0)),
                );
            }
            assert!(matches!(
                decode_syscall_result(words),
                Err(CapError::InvalidKey {
                    key: submitted,
                    reason: decoded,
                    operand: 0,
                }) if submitted == key && decoded == reason
            ));
        }
    });
}

#[test_case]
fn production_dispatch_rejects_every_operation_bit_without_changing_table() {
    with_nucleus(|nucleus, table_addr, _second| {
        let issued = nucleus
            .create_domain(table_addr)
            .expect("bootstrap console key missing");
        nucleus.current_domain = Some(0);
        // Write is zero: every individual set bit is invalid, including all
        // aliases that narrowing to u8/u16/u32 would turn back into Write.
        for op in (0..64).map(|bit| 1_u64 << bit).chain([u64::MAX]) {
            assert_dispatch_error(nucleus, issued, op, (8, 0, 0));
            assert_console_table(
                nucleus.current_domain_table_mut().unwrap(),
                issued,
                Some((Rights::all(), 0)),
            );
        }
        let table = nucleus.current_domain_table_mut().unwrap();
        let entry = table
            .remove(issued)
            .unwrap_or_else(|_| panic!("console removal failed"));
        let next = table
            .insert(issued.slot(), entry)
            .unwrap_or_else(|_| panic!("console reinstallation failed"));
        assert_eq!(next, RawKey::new(issued.slot(), issued.incarnation() + 1));
        assert_console_table(table, next, Some((Rights::all(), 0)));
    });
}

#[test_case]
fn production_dispatch_rejects_deleted_and_same_type_replaced_keys() {
    with_nucleus(|nucleus, table_addr, _second| {
        let old = nucleus
            .create_domain(table_addr)
            .expect("bootstrap console key missing");
        nucleus.current_domain = Some(0);
        assert_dispatch_error(nucleus, old, 1, (8, 0, 0));
        assert_console_table(
            nucleus.current_domain_table_mut().unwrap(),
            old,
            Some((Rights::all(), 0)),
        );
        nucleus
            .current_domain_table_mut()
            .unwrap()
            .remove(old)
            .unwrap_or_else(|_| panic!("console removal failed"));
        let future = RawKey::new(old.slot(), old.incarnation() + 1);
        for op in [0, u64::MAX] {
            assert_dispatch_error(nucleus, old, op, (27, old.to_wire(), 2));
            assert_console_table(nucleus.current_domain_table_mut().unwrap(), old, None);
            // Mismatch precedes invalidation even while the slot is vacant.
            assert_dispatch_error(nucleus, future, op, (27, future.to_wire(), 1));
            assert_console_table(nucleus.current_domain_table_mut().unwrap(), old, None);
        }
        let rights = Rights(Rights::READ);
        let replacement = nucleus
            .current_domain_table_mut()
            .unwrap()
            .insert(old.slot(), console_entry(rights, 0x2222))
            .unwrap_or_else(|_| panic!("replacement installation failed"));
        assert_eq!(replacement, future);
        for op in [0, u64::MAX] {
            assert_dispatch_error(nucleus, old, op, (27, old.to_wire(), 1));
            assert_console_table(
                nucleus.current_domain_table_mut().unwrap(),
                replacement,
                Some((rights, 0x2222)),
            );
        }
        assert_dispatch_error(nucleus, replacement, 1, (8, 0, 0));
        assert_console_table(
            nucleus.current_domain_table_mut().unwrap(),
            replacement,
            Some((rights, 0x2222)),
        );
        nucleus
            .current_domain_table_mut()
            .unwrap()
            .remove(replacement)
            .unwrap_or_else(|_| panic!("replacement removal failed"));
        assert_dispatch_error(nucleus, old, 0, (27, old.to_wire(), 1));
        assert_console_table(
            nucleus.current_domain_table_mut().unwrap(),
            replacement,
            None,
        );
        assert_dispatch_error(nucleus, replacement, 0, (27, replacement.to_wire(), 2));
        assert_console_table(
            nucleus.current_domain_table_mut().unwrap(),
            replacement,
            None,
        );
    });
}

#[test_case]
fn table_lookup_still_requires_an_installed_capability() {
    let mut table = KeyTable::new(DomainId(0));
    let slot = KeySlot::DEBUG_CONSOLE;
    let never_issued = RawKey::new(slot, 1);
    assert!(matches!(
        table.lookup(never_issued),
        Err(CapError::InvalidKey {
            key,
            reason: InvalidKeyReason::NeverIssued,
            operand: 0,
        }) if key == never_issued
    ));
    let invalid = RawKey::new(KeySlot(u32::MAX), 1);
    assert!(matches!(
        table.lookup(invalid),
        Err(CapError::InvalidKey {
            key,
            reason: InvalidKeyReason::SlotOutOfRange,
            operand: 0,
        }) if key == invalid
    ));

    let key = table
        .insert(slot, console_entry(Rights::all(), 0))
        .unwrap_or_else(|_| panic!("console insertion failed"));
    assert_eq!(key, RawKey::new(slot, 1));
    assert_eq!(table.len(), 1);
    let cap = table
        .lookup(key)
        .unwrap_or_else(|_| panic!("installed console not found"));
    assert!(matches!(
        invoke(cap, 1, u64::MAX, u64::MAX),
        Err(CapError::InvalidOperation)
    ));
}
