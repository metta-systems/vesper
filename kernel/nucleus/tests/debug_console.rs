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
        CapError, KeySlot, ObjectType, Rights, decode_syscall_result,
        domain::{DcbPage, DomainId},
    },
    objects::{
        ArchObjectsImpl, DebugConsole, Domain, KeyTable, Nucleus, ObjectPool, arch::ArchPools,
        domain::DcbPages, nucleus::NucleusPools,
    },
};

fn with_nucleus(test: impl FnOnce(&mut Nucleus<ArchObjectsImpl>)) {
    let mut backing = MaybeUninit::<[Domain; 2]>::uninit();
    // SAFETY: The backing is aligned for two Domains and remains exclusively
    // owned here until after the nucleus and its pool are dropped. The callback
    // cannot return a borrowed nucleus/object reference. Only the pool accesses
    // the backing while it is live.
    let domains =
        unsafe { ObjectPool::new(backing.as_mut_ptr().cast::<u8>(), size_of::<[Domain; 2]>()) };
    let mut nucleus = Nucleus {
        pools: NucleusPools {
            domains,
            // SAFETY: ArchPools currently contains only PhantomData and owns no
            // regions. This fixture does not allocate or invoke arch objects.
            arch: unsafe { ArchPools::new() },
        },
        current_domain: None,
        dcb_pages: DcbPages::new(),
    };
    test(&mut nucleus);
    for index in 0..2 {
        nucleus.pools.domains.deallocate(index);
    }
}

#[test_case]
fn missing_caller_cannot_invoke_bootstrap_console() {
    with_nucleus(|nucleus| {
        nucleus.create_domain();
        assert_eq!(nucleus.current_domain, None);
        let slot = KeySlot::DEBUG_CONSOLE.0;
        let args = [u64::MAX; 6];

        assert!(nucleus.current_domain_mut().is_none());
        let error = match api::handle_cap_invoke(nucleus, slot, 1, &args) {
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
            api::handle_cap_invoke(nucleus, slot, 1, &args),
            Err(CapError::InvalidOperation)
        ));
        nucleus.current_domain = None;
        assert!(matches!(
            api::handle_cap_invoke(nucleus, slot, 1, &args),
            Err(CapError::InvalidDomain)
        ));
    });
}

#[test_case]
fn dispatch_uses_only_the_explicit_allocated_caller_table() {
    with_nucleus(|nucleus| {
        nucleus.create_domain();
        let slot = KeySlot::DEBUG_CONSOLE.0;
        let args = [u64::MAX; 6];
        for caller in [1, 2, u32::MAX] {
            nucleus.current_domain = Some(caller);
            assert!(nucleus.current_domain_mut().is_none());
            assert!(matches!(
                api::handle_cap_invoke(nucleus, slot, 1, &args),
                Err(CapError::InvalidDomain)
            ));
        }

        nucleus
            .pools
            .domains
            .allocate(Domain {
                keytable: KeyTable::new(DomainId(1)),
            })
            .expect("second domain allocation failed");
        nucleus.current_domain = Some(1);
        assert!(matches!(
            api::handle_cap_invoke(nucleus, slot, 1, &args),
            Err(CapError::EmptySlot(s)) if s.0 == slot
        ));
        assert!(nucleus.pools.domains.deallocate(1));
        assert!(matches!(
            api::handle_cap_invoke(nucleus, slot, 1, &args),
            Err(CapError::InvalidDomain)
        ));
    });
}

#[test_case]
fn missing_caller_cannot_select_an_existing_dcb() {
    // This page is used only by this test, once in the serial QEMU harness.
    // Static backing satisfies DcbPages' retained-reference lifetime.
    static mut PAGE: DcbPage = DcbPage::new();
    with_nucleus(|nucleus| {
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
        for op in [0, u32::MAX] {
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
    let console = DebugConsole;
    let cap = KeyEntry::new(&console, Rights::all(), 42);
    let alias = &cap;

    // Invalid opcodes must fail before constructing an address or copying bytes.
    for op in [1, 127, 255, 256, 1 << 16, u32::MAX] {
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
    assert_eq!(cap.generation(), 0);
}

#[test_case]
fn table_lookup_still_requires_an_installed_capability() {
    let mut table = KeyTable::new(DomainId(0));
    let slot = KeySlot::DEBUG_CONSOLE;
    assert!(matches!(table.lookup(slot), Err(CapError::EmptySlot(s)) if s == slot));
    let invalid = KeySlot(u32::MAX);
    assert!(matches!(table.lookup(invalid), Err(CapError::InvalidSlot(s)) if s == invalid));

    let console = DebugConsole;
    table
        .insert(slot, KeyEntry::new(&console, Rights::all(), 0))
        .unwrap_or_else(|_| panic!("console insertion failed"));
    let cap = table
        .lookup(slot)
        .unwrap_or_else(|_| panic!("installed console not found"));
    assert!(matches!(
        invoke(cap, 1, u64::MAX, u64::MAX),
        Err(CapError::InvalidOperation)
    ));
}
