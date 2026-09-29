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

#[path = "support/resume.rs"]
mod resume_tests;

use {
    api::{KeyEntry, debug_console::invoke},
    core::mem::{MaybeUninit, align_of, size_of},
    libaddress::PhysAddr,
    libobject::{
        CapError, InconsistencyReason, InvalidKeyReason, KeySlot, ObjectType, RawKey, Rights,
        decode_syscall_result,
        domain::{DcbPage, DomainId},
    },
    objects::{
        ArchObjects, ArchObjectsImpl, KeyTable, Nucleus, ObjectPool, Thread,
        access::{Access, ObjectId, PoolTag},
        arch::ArchPools,
        arch_objects::AddressSpaceObject,
        domain::DcbPages,
        key_table::KeyTableBinding,
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

/// Fixture-table capacity exponent: the historical 256-slot table.
const SIZE_BITS: u8 = 8;

/// The fixture tables' guard (guarded key-space package, selected 2026-09-23):
/// every key minted into a fixture table carries it above the slot index. It
/// fits the 24 guard bits of a 256-entry table's table-relative address.
const FIXTURE_GUARD: u32 = 0xD1A_5EE;

/// The bare slot index of a fixture key (its low `SIZE_BITS` address bits).
fn bare_index(key: RawKey) -> KeySlot {
    KeySlot(key.slot().0 & ((1_u32 << u32::from(SIZE_BITS)) - 1))
}

/// Carve a `KeyTable` into the fixed test backing at `index`, returning its
/// kernel address. A self-table capability with the fixture guard is installed
/// at the well-known slot, anchoring invocations through this table.
///
/// Implementation status: returns the stable carved-table binding used when
/// provisioning an AddressSpace, rather than a per-Thread table address.
fn carve(index: usize) -> KeyTableBinding {
    let stride = KeyTable::carve_size(SIZE_BITS);
    let obj = (TEST_BACKING + (index as u64) * (stride as u64)) as *mut u8;
    // SAFETY: TEST_BACKING is RAM, 32-byte aligned (a 512 MiB boundary), and
    // exclusively owned by the test fixture for its lifetime; the stride
    // covers the full variable-size carve.
    unsafe {
        KeyTable::initialize(obj, DomainId(0), SIZE_BITS);
        let table = &mut *(obj as *mut KeyTable);
        table
            .insert(
                KeySlot::SELF_KEYTABLE,
                KeyEntry::new_keytable(obj as u64, FIXTURE_GUARD, SIZE_BITS, Rights::all(), 0),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|_| panic!("fixture self-table installation failed"));
        // SAFETY: the full carve is initialized, private to this serial QEMU
        // fixture, and stays at its fixed address without relocation or
        // reclamation while any binding is live. No binding escapes the
        // fixture; subsequent tests reinitialize it only after this nucleus
        // and all its AddressSpaces have been dropped.
        table.binding()
    }
}

impl Nucleus<ArchObjectsImpl> {
    /// One-time console provisioning for a fixture AddressSpace, then Thread creation.
    /// Additional Threads in that AddressSpace use production `create_thread`.
    ///
    /// Implementation status: console authority is test-local; production Thread
    /// creation validates the AddressSpace and allocates without changing its table
    /// or selecting a current caller.
    fn create_console_thread(&mut self, address_space: ObjectId) -> Option<RawKey> {
        let binding = {
            // SAFETY: the serial QEMU fixture provides exclusive kernel access;
            // no other Access context overlaps this short validation scope.
            let access = unsafe { Access::new() };
            access
                .resolve(&self.pools.arch.address_spaces, address_space)
                .ok()?
                .keytable()
        };
        let key = {
            // SAFETY: this live AddressSpace binding names a fixed, initialized
            // carve owned by the serial fixture. No table reference or other
            // Access context overlaps this scope, and the guard is dropped
            // before production Thread creation or subsequent dispatch.
            let access = unsafe { Access::new() };
            let mut table = access
                .resolve_carved_mut::<KeyTable>(binding.address())
                .ok()?;
            // Discover the guard through SELF, the same source used by syscall
            // entry, so the fixture installs a resolvable guarded console key.
            let (_self_addr, guard, _size_bits) = table
                .self_table_capability()
                .expect("fixture table has no self-table capability");
            table
                .insert(
                    KeySlot::DEBUG_CONSOLE,
                    console_entry(Rights::all(), 0),
                    guard,
                )
                .unwrap_or_else(|failure| {
                    panic!(
                        "bootstrap console installation failed: {:?}",
                        failure.error.code()
                    )
                })
        };
        self.create_thread(address_space)?;
        Some(key)
    }
}

fn with_nucleus(test: impl FnOnce(&mut Nucleus<ArchObjectsImpl>, u64, u64, ObjectId, ObjectId)) {
    // 16-byte-aligned carve backings (the minimum pool carve alignment), each
    // large enough for its pool's carve (asserted below).
    const THREAD_CAPACITY: usize = 4;
    const THREAD_WORDS: usize =
        ObjectPool::<Thread>::carve_size(THREAD_CAPACITY).div_ceil(size_of::<u128>());
    type Pt = objects::arch::AArch64PageTable;
    const PT_WORDS: usize = ObjectPool::<Pt>::carve_size(2).div_ceil(size_of::<u128>());
    type As = objects::arch::AArch64AddressSpace;
    const AS_WORDS: usize = ObjectPool::<As>::carve_size(2).div_ceil(size_of::<u128>());
    let mut thread_backing = MaybeUninit::<[u128; THREAD_WORDS]>::uninit();
    let mut pt_backing = MaybeUninit::<[u128; PT_WORDS]>::uninit();
    let mut as_backing = MaybeUninit::<[u128; AS_WORDS]>::uninit();
    const _: () = {
        assert!(ObjectPool::<Thread>::carve_size(THREAD_CAPACITY) <= 4096);
        assert!(align_of::<[u128; THREAD_WORDS]>() >= ObjectPool::<Thread>::ALIGN);
        assert!(align_of::<[u128; PT_WORDS]>() >= ObjectPool::<Pt>::ALIGN);
        assert!(align_of::<[u128; AS_WORDS]>() >= ObjectPool::<As>::ALIGN);
    };
    assert!(ObjectPool::<Thread>::carve_size(THREAD_CAPACITY) <= size_of::<[u128; THREAD_WORDS]>());
    assert!(ObjectPool::<Pt>::carve_size(2) <= size_of::<[u128; PT_WORDS]>());
    assert!(ObjectPool::<As>::carve_size(2) <= size_of::<[u128; AS_WORDS]>());
    // SAFETY: The backings are carve-aligned and remain exclusively owned
    // here until after the nucleus and its pools are dropped. The callback
    // cannot return a borrowed nucleus/object reference. Only the pools
    // access the backing while they are live.
    let threads = unsafe {
        ObjectPool::initialize(thread_backing.as_mut_ptr().cast::<u8>(), THREAD_CAPACITY)
    };
    let page_tables = unsafe { ObjectPool::initialize(pt_backing.as_mut_ptr().cast::<u8>(), 2) };
    let address_spaces = unsafe { ObjectPool::initialize(as_backing.as_mut_ptr().cast::<u8>(), 2) };
    // Carve two KeyTable regions from the fixed test backing (mirrors the boot
    // carve / runtime Retype: the table's storage is the carved region).
    let table_binding = carve(0);
    let second_table_binding = carve(1);
    let mut nucleus = Nucleus {
        pools: NucleusPools {
            threads,
            // SAFETY: zero-capacity pool: this test never allocates or
            // invokes Notification objects, so the carve pointers are never
            // dereferenced.
            notifications: unsafe {
                ObjectPool::initialize(pt_backing.as_mut_ptr().cast::<u8>(), 0)
            },
            // SAFETY: zero-capacity pool: this test never allocates or
            // invokes EventCount objects.
            event_counts: unsafe {
                ObjectPool::initialize(pt_backing.as_mut_ptr().cast::<u8>(), 0)
            },
            // SAFETY: the page-table, address-space, and ASID-pool backings are
            // exclusively owned by this fixture; this test does not invoke
            // arch objects (the ASID pool has zero capacity for the same
            // reason).
            arch: unsafe {
                ArchPools::new(
                    page_tables,
                    address_spaces,
                    ObjectPool::initialize(pt_backing.as_mut_ptr().cast::<u8>(), 0),
                )
            },
        },
        current_thread: None,
        dcb_pages: DcbPages::new(),
        pending: crate::objects::PendingPool::new(),
        scheduler: crate::objects::Scheduler::new(),
    };
    // One fixture AddressSpace shared by the fixture Threads: these tests
    // never resolve translation state.
    // Implementation status: table selection now resolves AddressSpace
    // identity. Each distinct fixture table is bound to its own AddressSpace
    // at provisioning; Threads in the same AddressSpace share its one table.
    // No translation root or hardware context is installed by this fixture.
    let fixture_as = nucleus
        .pools
        .arch
        .address_spaces
        .allocate(ArchObjectsImpl::new_address_space(table_binding))
        .expect("no fixture AddressSpace slot")
        .0;
    let second_as = nucleus
        .pools
        .arch
        .address_spaces
        .allocate(ArchObjectsImpl::new_address_space(second_table_binding))
        .expect("no second fixture AddressSpace slot")
        .0;
    assert_ne!(fixture_as, second_as);
    assert_ne!(table_binding.address(), second_table_binding.address());
    assert_eq!(table_binding.size_bits(), SIZE_BITS);
    assert_eq!(second_table_binding.size_bits(), SIZE_BITS);
    test(
        &mut nucleus,
        table_binding.address(),
        second_table_binding.address(),
        fixture_as,
        second_as,
    );
    for index in 0..THREAD_CAPACITY {
        if let Some(generation) = nucleus.pools.threads.generation_of(index) {
            let id = ObjectId {
                pool: PoolTag::Thread,
                index: u16::try_from(index).unwrap(),
                generation,
            };
            nucleus
                .pools
                .threads
                .deallocate(id)
                .unwrap_or_else(|_| panic!("thread cleanup failed"));
        }
    }
}

#[test_case]
fn missing_caller_cannot_invoke_bootstrap_console() {
    with_nucleus(|nucleus, _table_addr, _second, fixture_as, _second_as| {
        let key = nucleus
            .create_console_thread(fixture_as)
            .expect("bootstrap console key missing");
        assert_eq!(bare_index(key), KeySlot::DEBUG_CONSOLE);
        assert_ne!(key.incarnation(), 0);
        assert_eq!(nucleus.current_thread, None);
        let args = [u64::MAX; 6];

        assert!(nucleus.current_thread_mut().is_none());
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
        nucleus.current_thread = Some(0);
        assert!(matches!(
            api::handle_cap_invoke(nucleus, key, 1, &args),
            Err(CapError::InvalidOperation)
        ));
        nucleus.current_thread = None;
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
    with_nucleus(
        |nucleus, _table_addr, _second_table_addr, fixture_as, second_as| {
            let key = nucleus
                .create_console_thread(fixture_as)
                .expect("bootstrap console key missing");
            let args = [u64::MAX; 6];
            for caller in [1, 2, u32::MAX] {
                nucleus.current_thread = Some(caller);
                assert!(nucleus.current_thread_mut().is_none());
                assert!(matches!(
                    api::handle_cap_invoke(nucleus, key, 1, &args),
                    Err(CapError::InvalidDomain)
                ));
            }

            nucleus
                .pools
                .threads
                .allocate(Thread {
                    address_space: second_as,
                    context: crate::objects::ExecutionContext::Running,
                })
                .expect("second thread allocation failed");
            nucleus.current_thread = Some(1);
            let diag = api::handle_cap_invoke(nucleus, key, 1, &args);
            let words = match diag {
                Ok(_) => panic!("second-table invocation succeeded"),
                Err(error) => error.code(),
            };
            // NeverIssued in wire form: the console key's slot was never issued in
            // the second table.
            assert_eq!(words, (26, key.to_wire(), 3));
            // The second table holds only its self-table capability: the console
            // key (same guard, same slot number) was never issued there.
            assert_eq!(nucleus.current_thread_table_mut().unwrap().len(), 1);
            let id = ObjectId {
                pool: PoolTag::Thread,
                index: 1,
                generation: 1,
            };
            assert!(nucleus.pools.threads.deallocate(id).is_ok());
            assert!(matches!(
                api::handle_cap_invoke(nucleus, key, 1, &args),
                Err(CapError::InvalidDomain)
            ));
        },
    );
}

#[test_case]
fn threads_in_the_same_address_space_share_one_dispatch_table() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let issued = nucleus
            .create_console_thread(fixture_as)
            .expect("bootstrap console key missing");
        nucleus.current_thread = Some(0);
        let self_key = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, KeySlot::SELF_KEYTABLE.0, 1);
        let original_console_id = {
            let table = nucleus.current_thread_table_mut().unwrap();
            assert_console_table(table, issued, Some((Rights::all(), 0)));
            table
                .lookup(issued, FIXTURE_GUARD)
                .unwrap_or_else(|_| panic!("console key missing before Thread creation"))
                .object_id()
                .unwrap_or_else(|_| panic!("console identity missing"))
        };
        // Implementation status: production creation is table-neutral even when
        // the AddressSpace already has a console grant. It must not reinstall
        // that grant, advance any slot counter, or replace either live identity.
        let second_thread = nucleus
            .create_thread(fixture_as)
            .expect("second thread allocation failed");
        assert_eq!(second_thread.pool, PoolTag::Thread);
        assert_eq!(second_thread.index, 1);
        assert_eq!(second_thread.generation, 1);
        assert_eq!(nucleus.pools.threads.len(), 2);
        assert_eq!(nucleus.current_thread, Some(0));
        for caller in [0, u32::from(second_thread.index)] {
            nucleus.current_thread = Some(caller);
            assert_eq!(
                nucleus.current_thread_mut().unwrap().address_space,
                fixture_as
            );
            assert_eq!(nucleus.current_thread_table_addr(), Some(table_addr));
            assert_dispatch_error(nucleus, issued, 1, (8, 0, 0));
            let table = nucleus.current_thread_table_mut().unwrap();
            assert_console_table(table, issued, Some((Rights::all(), 0)));
            assert_eq!(
                table.self_table_capability(),
                Some((table_addr, FIXTURE_GUARD, SIZE_BITS))
            );
            let self_cap = table
                .lookup(self_key, FIXTURE_GUARD)
                .unwrap_or_else(|_| panic!("Thread creation changed SELF identity"));
            assert_eq!(self_cap.object_type(), ObjectType::KEY_TABLE);
            assert_eq!(self_cap.rights(), Rights::all());
            assert_eq!(self_cap.badge(), 0);
            assert_eq!(
                table
                    .lookup(issued, FIXTURE_GUARD)
                    .unwrap_or_else(|_| panic!("Thread creation changed console key"))
                    .object_id()
                    .unwrap_or_else(|_| panic!("Thread creation changed console identity")),
                original_console_id
            );
            // Exact live keys still resolve, their next incarnations mismatch,
            // and assert_console_table checks every other slot is NeverIssued:
            // production creation changed neither occupancy nor slot counters.
            for key in [self_key, issued] {
                let future = RawKey::from_parts(
                    FIXTURE_GUARD,
                    SIZE_BITS,
                    bare_index(key).0,
                    key.incarnation() + 1,
                );
                assert!(matches!(
                    table.lookup(future, FIXTURE_GUARD),
                    Err(CapError::InconsistentKey {
                        key: submitted,
                        reason: InconsistencyReason::SlotIncarnationMismatch,
                        operand: 0,
                    }) if submitted == future
                ));
            }
        }

        nucleus.current_thread = Some(0);
        let rights = Rights(Rights::READ);
        let replacement = {
            let table = nucleus.current_thread_table_mut().unwrap();
            table
                .remove(issued, FIXTURE_GUARD)
                .unwrap_or_else(|_| panic!("shared console removal failed"));
            table
                .insert(bare_index(issued), console_entry(rights, 91), FIXTURE_GUARD)
                .unwrap_or_else(|_| panic!("shared console replacement failed"))
        };
        assert_eq!(replacement.incarnation(), issued.incarnation() + 1);
        for caller in [0, u32::from(second_thread.index)] {
            nucleus.current_thread = Some(caller);
            assert_eq!(nucleus.current_thread_table_addr(), Some(table_addr));
            assert_dispatch_error(nucleus, issued, 0, (27, issued.to_wire(), 1));
            assert_dispatch_error(nucleus, replacement, 1, (8, 0, 0));
            assert_console_table(
                nucleus.current_thread_table_mut().unwrap(),
                replacement,
                Some((rights, 91)),
            );
        }
    });
}

#[test_case]
fn changing_thread_address_space_selects_its_table_on_the_next_dispatch() {
    with_nucleus(
        |nucleus, table_addr, second_table_addr, fixture_as, second_as| {
            let issued = nucleus
                .create_console_thread(fixture_as)
                .expect("bootstrap console key missing");
            nucleus.current_thread = Some(0);
            assert_dispatch_error(nucleus, issued, 1, (8, 0, 0));
            assert_eq!(nucleus.current_thread_table_addr(), Some(table_addr));

            // This is a private-state fixture transition, not PPC or a hardware
            // migration: no translation context or saved registers are switched.
            nucleus.current_thread_mut().unwrap().address_space = second_as;
            assert_eq!(nucleus.current_thread_table_addr(), Some(second_table_addr));
            assert_dispatch_error(nucleus, issued, 0, (26, issued.to_wire(), 3));
            assert_eq!(nucleus.current_thread_table_mut().unwrap().len(), 1);
            let rights = Rights(Rights::READ);
            let second_key = nucleus
                .current_thread_table_mut()
                .unwrap()
                .insert(
                    KeySlot::DEBUG_CONSOLE,
                    console_entry(rights, 92),
                    FIXTURE_GUARD,
                )
                .unwrap_or_else(|_| panic!("second-table console installation failed"));
            // These fixture tables deliberately use the same guard: differing
            // table contents, not differing key bits, prove the table selection.
            assert_eq!(second_key, issued);
            assert_dispatch_error(nucleus, second_key, 1, (8, 0, 0));
            assert_console_table(
                nucleus.current_thread_table_mut().unwrap(),
                second_key,
                Some((rights, 92)),
            );

            nucleus.current_thread_mut().unwrap().address_space = fixture_as;
            assert_eq!(nucleus.current_thread_table_addr(), Some(table_addr));
            assert_dispatch_error(nucleus, issued, 1, (8, 0, 0));
            assert_console_table(
                nucleus.current_thread_table_mut().unwrap(),
                issued,
                Some((Rights::all(), 0)),
            );
            for (id, address) in [(fixture_as, table_addr), (second_as, second_table_addr)] {
                assert!(nucleus.pools.arch.address_spaces.validate(id).is_ok());
                let binding = nucleus
                    .pools
                    .arch
                    .address_spaces
                    .get_live(usize::from(id.index))
                    .unwrap()
                    .keytable();
                assert_eq!(binding.address(), address);
                assert_eq!(binding.size_bits(), SIZE_BITS);
            }
        },
    );
}

#[test_case]
fn stale_address_space_is_rejected_before_self_table_or_key_lookup() {
    with_nucleus(
        |nucleus, _table_addr, second_table_addr, fixture_as, second_as| {
            let issued = nucleus
                .create_console_thread(fixture_as)
                .expect("bootstrap console key missing");
            nucleus.current_thread = Some(0);
            let self_key =
                RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, KeySlot::SELF_KEYTABLE.0, 1);
            nucleus
                .current_thread_table_mut()
                .unwrap()
                .remove(self_key, FIXTURE_GUARD)
                .unwrap_or_else(|_| panic!("self-table removal failed"));
            // A live AddressSpace reaches the missing SELF rejection, even for a
            // malformed key; stale AddressSpace validation must win instead.
            // Implementation status: stale callers report InvalidDomain (status
            // 3), before SELF or key lookup, not an object-retirement status.
            let malformed = RawKey::from_wire(0);
            assert_dispatch_error(nucleus, malformed, 0, (27, 0, 2));
            assert!(
                nucleus
                    .pools
                    .arch
                    .address_spaces
                    .deallocate(fixture_as)
                    .is_ok()
            );
            assert_eq!(
                nucleus.current_thread_mut().unwrap().address_space,
                fixture_as
            );
            assert!(nucleus.current_thread_table_addr().is_none());
            assert!(nucleus.current_thread_table_mut().is_none());
            for key in [malformed, issued] {
                assert_dispatch_error(nucleus, key, 0, (3, 0, 0));
            }

            let replacement_binding = nucleus
                .pools
                .arch
                .address_spaces
                .get_live(usize::from(second_as.index))
                .unwrap()
                .keytable();
            assert!(
                nucleus
                    .pools
                    .arch
                    .address_spaces
                    .deallocate(second_as)
                    .is_ok()
            );
            let replacement_as = nucleus
                .pools
                .arch
                .address_spaces
                .allocate(ArchObjectsImpl::new_address_space(replacement_binding))
                .expect("replacement AddressSpace allocation failed")
                .0;
            assert_eq!(replacement_as.index, fixture_as.index);
            assert_eq!(replacement_as.generation, fixture_as.generation + 1);
            assert!(
                nucleus
                    .pools
                    .arch
                    .address_spaces
                    .validate(replacement_as)
                    .is_ok()
            );
            assert!(
                nucleus
                    .pools
                    .arch
                    .address_spaces
                    .validate(fixture_as)
                    .is_err()
            );
            assert!(nucleus.current_thread_table_addr().is_none());
            assert!(nucleus.current_thread_table_mut().is_none());
            for key in [malformed, issued] {
                assert_dispatch_error(nucleus, key, 0, (3, 0, 0));
            }
            assert_eq!(nucleus.pools.threads.len(), 1);

            // Only the fresh identity admits access to the replacement's live
            // table. Neither reuse nor the stale Thread silently refreshes it.
            nucleus.current_thread_mut().unwrap().address_space = replacement_as;
            assert_eq!(nucleus.current_thread_table_addr(), Some(second_table_addr));
            assert_dispatch_error(nucleus, malformed, 0, (26, 0, 1));
            assert_dispatch_error(nucleus, issued, 0, (26, issued.to_wire(), 3));
            assert_eq!(nucleus.current_thread_table_mut().unwrap().len(), 1);
        },
    );
}

#[test_case]
fn fixture_thread_creation_rejects_stale_address_space_without_allocating() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let binding = nucleus
            .pools
            .arch
            .address_spaces
            .get_live(usize::from(fixture_as.index))
            .unwrap()
            .keytable();
        assert!(nucleus.pools.threads.is_empty());
        assert!(
            nucleus
                .pools
                .arch
                .address_spaces
                .deallocate(fixture_as)
                .is_ok()
        );
        // These rejection checks exercise production creation, not the console
        // provisioning helper: a stale AddressSpace must consume no Thread slot.
        assert!(nucleus.create_thread(fixture_as).is_none());
        assert!(nucleus.pools.threads.is_empty());
        assert_eq!(nucleus.current_thread, None);

        let replacement_as = nucleus
            .pools
            .arch
            .address_spaces
            .allocate(ArchObjectsImpl::new_address_space(binding))
            .expect("replacement AddressSpace allocation failed")
            .0;
        assert_eq!(replacement_as.index, fixture_as.index);
        assert_eq!(replacement_as.generation, fixture_as.generation + 1);
        assert!(nucleus.create_thread(fixture_as).is_none());
        assert!(nucleus.pools.threads.is_empty());
        assert_eq!(nucleus.current_thread, None);

        let issued = nucleus
            .create_console_thread(replacement_as)
            .expect("live replacement AddressSpace rejected");
        assert_eq!(nucleus.pools.threads.len(), 1);
        // The first slot and first generation are intact: rejected creation
        // did not allocate a Thread even temporarily, or consume its identity.
        assert_eq!(nucleus.pools.threads.generation_of(0), Some(1));
        assert_eq!(issued.incarnation(), 1);
        nucleus.current_thread = Some(0);
        assert_eq!(
            nucleus.current_thread_mut().unwrap().address_space,
            replacement_as
        );
        assert_eq!(nucleus.current_thread_table_addr(), Some(table_addr));
        assert_dispatch_error(nucleus, issued, 1, (8, 0, 0));
        assert_console_table(
            nucleus.current_thread_table_mut().unwrap(),
            issued,
            Some((Rights::all(), 0)),
        );
    });
}

#[test_case]
fn mismatched_self_table_address_or_size_rejects_dispatch_without_changes() {
    with_nucleus(
        |nucleus, table_addr, second_table_addr, fixture_as, _second_as| {
            let issued = nucleus
                .create_console_thread(fixture_as)
                .expect("bootstrap console key missing");
            nucleus.current_thread = Some(0);
            let mut self_key =
                RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, KeySlot::SELF_KEYTABLE.0, 1);
            for (address, size_bits) in [
                (second_table_addr, SIZE_BITS),
                (table_addr, SIZE_BITS - 1),
                (table_addr, SIZE_BITS + 1),
            ] {
                let original = nucleus
                    .current_thread_table_mut()
                    .unwrap()
                    .remove(self_key, FIXTURE_GUARD)
                    .unwrap_or_else(|_| panic!("self-table removal failed"));
                self_key = nucleus
                    .current_thread_table_mut()
                    .unwrap()
                    .insert(
                        KeySlot::SELF_KEYTABLE,
                        KeyEntry::new_keytable(
                            address,
                            FIXTURE_GUARD,
                            size_bits,
                            Rights::all(),
                            93,
                        ),
                        FIXTURE_GUARD,
                    )
                    .unwrap_or_else(|_| panic!("mismatched self-table installation failed"));
                for key in [issued, RawKey::from_wire(0), RawKey::from_wire(u64::MAX)] {
                    for op in [0, u64::MAX] {
                        assert_dispatch_error(nucleus, key, op, (27, key.to_wire(), 2));
                        let table = nucleus.current_thread_table_mut().unwrap();
                        assert_console_table(table, issued, Some((Rights::all(), 0)));
                        assert_eq!(table.size_bits(), SIZE_BITS);
                        assert_eq!(
                            table.self_table_capability(),
                            Some((address, FIXTURE_GUARD, size_bits))
                        );
                        let cap = table
                            .lookup(self_key, FIXTURE_GUARD)
                            .unwrap_or_else(|_| panic!("rejected dispatch changed SELF identity"));
                        assert_eq!(
                            cap.keytable_address()
                                .unwrap_or_else(|_| panic!("not a table")),
                            address
                        );
                        assert_eq!(
                            cap.keytable_guard_and_size()
                                .unwrap_or_else(|_| panic!("not a table")),
                            (FIXTURE_GUARD, size_bits)
                        );
                        assert_eq!(cap.rights(), Rights::all());
                        assert_eq!(cap.badge(), 93);
                        let future = RawKey::from_parts(
                            FIXTURE_GUARD,
                            SIZE_BITS,
                            KeySlot::SELF_KEYTABLE.0,
                            self_key.incarnation() + 1,
                        );
                        let error = match table.lookup(future, FIXTURE_GUARD) {
                            Err(error) => error,
                            Ok(_) => panic!("rejected dispatch changed SELF incarnation"),
                        };
                        assert_eq!(error.code(), (27, future.to_wire(), 1));
                    }
                }
                nucleus
                    .current_thread_table_mut()
                    .unwrap()
                    .remove(self_key, FIXTURE_GUARD)
                    .unwrap_or_else(|_| panic!("mismatched self-table removal failed"));
                self_key = nucleus
                    .current_thread_table_mut()
                    .unwrap()
                    .insert(KeySlot::SELF_KEYTABLE, original, FIXTURE_GUARD)
                    .unwrap_or_else(|_| panic!("self-table restoration failed"));
                assert_dispatch_error(nucleus, issued, 1, (8, 0, 0));
            }
        },
    );
}

#[test_case]
fn self_table_capability_remains_the_source_of_the_caller_guard() {
    with_nucleus(|nucleus, table_addr, _second, fixture_as, _second_as| {
        let issued = nucleus
            .create_console_thread(fixture_as)
            .expect("bootstrap console key missing");
        nucleus.current_thread = Some(0);
        let self_key = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, KeySlot::SELF_KEYTABLE.0, 1);
        let changed_guard = FIXTURE_GUARD ^ 1;
        // Deliberately replace private fixture metadata to test the lookup
        // source; this is not a public guard-mutation or table-rebinding API.
        let table = nucleus.current_thread_table_mut().unwrap();
        table
            .remove(self_key, FIXTURE_GUARD)
            .unwrap_or_else(|_| panic!("self-table removal failed"));
        table
            .insert(
                KeySlot::SELF_KEYTABLE,
                KeyEntry::new_keytable(table_addr, changed_guard, SIZE_BITS, Rights::all(), 0),
                changed_guard,
            )
            .unwrap_or_else(|_| panic!("changed-guard self-table installation failed"));
        let rebound_key = RawKey::from_parts(
            changed_guard,
            SIZE_BITS,
            KeySlot::DEBUG_CONSOLE.0,
            issued.incarnation(),
        );
        assert_dispatch_error(nucleus, issued, 0, (26, issued.to_wire(), 4));
        assert_dispatch_error(nucleus, rebound_key, 1, (8, 0, 0));
        let table = nucleus.current_thread_table_mut().unwrap();
        assert_eq!(table.len(), 2);
        assert_eq!(
            table.self_table_capability(),
            Some((table_addr, changed_guard, SIZE_BITS))
        );
        let cap = table
            .lookup(rebound_key, changed_guard)
            .unwrap_or_else(|_| panic!("console metadata changed"));
        assert_eq!(cap.object_type(), ObjectType::DEBUG_CONSOLE);
        assert_eq!(cap.rights(), Rights::all());
        assert_eq!(cap.badge(), 0);
    });
}

#[test_case]
fn missing_caller_cannot_select_an_existing_dcb() {
    // This page is used only by this test, once in the serial QEMU harness.
    // Static backing satisfies DcbPages' retained-reference lifetime.
    static mut PAGE: DcbPage = DcbPage::new();
    with_nucleus(|nucleus, _table_addr, _second, _fixture_as, _second_as| {
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
            nucleus.current_thread = Some(id.0);
            assert_eq!(nucleus.current_dcb_mut().unwrap().id, id);
        }
        nucleus.current_thread = None;
        assert!(nucleus.current_dcb_mut().is_none());
        nucleus.current_thread = Some(u32::MAX);
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
    // The fixture's self-table capability is always present alongside the
    // console entry under test.
    assert_eq!(table.len(), 1 + usize::from(live.is_some()));
    match live {
        Some((rights, badge)) => {
            let cap = table
                .lookup(key, FIXTURE_GUARD)
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
            table.lookup(key, FIXTURE_GUARD),
            Err(CapError::InconsistentKey {
                key: submitted,
                reason: InconsistencyReason::CapabilityInvalidated,
                operand: 0,
            }) if submitted == key
        )),
    }
    let console_index = bare_index(key).0;
    let self_index = KeySlot::SELF_KEYTABLE.0;
    for index in 0..KeyTable::capacity_for(SIZE_BITS) {
        // Skip the console slot and the fixture's self-table slot (both
        // issued); every other correctly guarded probe is never issued.
        if u32::try_from(index).unwrap() != console_index
            && u32::try_from(index).unwrap() != self_index
        {
            let probe = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, index as u32, 1);
            assert!(matches!(
                table.lookup(probe, FIXTURE_GUARD),
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
    with_nucleus(|nucleus, _table_addr, _second, fixture_as, _second_as| {
        let issued = nucleus
            .create_console_thread(fixture_as)
            .expect("bootstrap console key missing");
        nucleus.current_thread = Some(0);
        // Literal wire words deliberately avoid deriving expectations from the
        // encoder under test. Zero incarnation wins even over a wrong guard;
        // with a nonzero table guard, every address whose guard bits do not
        // match rejects as GuardMismatch before indexing (selected 2026-09-23).
        for (wire, reason, details) in [
            (0x0000_0000_0000_0000, InvalidKeyReason::ZeroIncarnation, 1),
            (0x0000_0000_0000_007f, InvalidKeyReason::ZeroIncarnation, 1),
            (0x0000_0000_ffff_ffff, InvalidKeyReason::ZeroIncarnation, 1),
            (0x0000_0001_ffff_ffff, InvalidKeyReason::GuardMismatch, 4),
            (0x0000_0001_8000_007f, InvalidKeyReason::GuardMismatch, 4),
            (0xffff_ffff_ffff_ffff, InvalidKeyReason::GuardMismatch, 4),
            (0x0000_0001_0000_0000, InvalidKeyReason::GuardMismatch, 4),
            (0xffff_ffff_0000_0000, InvalidKeyReason::GuardMismatch, 4),
        ] {
            let key = RawKey::from_wire(wire);
            let words = (26, wire, details);
            for op in [0, u64::MAX] {
                assert_dispatch_error(nucleus, key, op, words);
                assert_console_table(
                    nucleus.current_thread_table_mut().unwrap(),
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
        // A correctly guarded key at a never-issued slot keeps its own
        // diagnostic: the guard matches and the slot was never issued.
        let never = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, 200, 1);
        let words = (26, never.to_wire(), 3);
        for op in [0, u64::MAX] {
            assert_dispatch_error(nucleus, never, op, words);
        }
        assert!(matches!(
            decode_syscall_result(words),
            Err(CapError::InvalidKey {
                key: submitted,
                reason: InvalidKeyReason::NeverIssued,
                operand: 0,
            }) if submitted == never
        ));
    });
}

#[test_case]
fn production_dispatch_rejects_every_operation_bit_without_changing_table() {
    with_nucleus(|nucleus, _table_addr, _second, fixture_as, _second_as| {
        let issued = nucleus
            .create_console_thread(fixture_as)
            .expect("bootstrap console key missing");
        nucleus.current_thread = Some(0);
        // Write is zero: every individual set bit is invalid, including all
        // aliases that narrowing to u8/u16/u32 would turn back into Write.
        for op in (0..64).map(|bit| 1_u64 << bit).chain([u64::MAX]) {
            assert_dispatch_error(nucleus, issued, op, (8, 0, 0));
            assert_console_table(
                nucleus.current_thread_table_mut().unwrap(),
                issued,
                Some((Rights::all(), 0)),
            );
        }
        let table = nucleus.current_thread_table_mut().unwrap();
        let entry = table
            .remove(issued, FIXTURE_GUARD)
            .unwrap_or_else(|_| panic!("console removal failed"));
        let next = table
            .insert(bare_index(issued), entry, FIXTURE_GUARD)
            .unwrap_or_else(|_| panic!("console reinstallation failed"));
        assert_eq!(
            next,
            RawKey::from_parts(
                FIXTURE_GUARD,
                SIZE_BITS,
                bare_index(issued).0,
                issued.incarnation() + 1
            )
        );
        assert_console_table(table, next, Some((Rights::all(), 0)));
    });
}

#[test_case]
fn production_dispatch_rejects_deleted_and_same_type_replaced_keys() {
    with_nucleus(|nucleus, _table_addr, _second, fixture_as, _second_as| {
        let old = nucleus
            .create_console_thread(fixture_as)
            .expect("bootstrap console key missing");
        nucleus.current_thread = Some(0);
        assert_dispatch_error(nucleus, old, 1, (8, 0, 0));
        assert_console_table(
            nucleus.current_thread_table_mut().unwrap(),
            old,
            Some((Rights::all(), 0)),
        );
        nucleus
            .current_thread_table_mut()
            .unwrap()
            .remove(old, FIXTURE_GUARD)
            .unwrap_or_else(|_| panic!("console removal failed"));
        let future = RawKey::from_parts(
            FIXTURE_GUARD,
            SIZE_BITS,
            bare_index(old).0,
            old.incarnation() + 1,
        );
        for op in [0, u64::MAX] {
            assert_dispatch_error(nucleus, old, op, (27, old.to_wire(), 2));
            assert_console_table(nucleus.current_thread_table_mut().unwrap(), old, None);
            // Mismatch precedes invalidation even while the slot is vacant.
            assert_dispatch_error(nucleus, future, op, (27, future.to_wire(), 1));
            assert_console_table(nucleus.current_thread_table_mut().unwrap(), old, None);
        }
        let rights = Rights(Rights::READ);
        let replacement = nucleus
            .current_thread_table_mut()
            .unwrap()
            .insert(
                bare_index(old),
                console_entry(rights, 0x2222),
                FIXTURE_GUARD,
            )
            .unwrap_or_else(|_| panic!("replacement installation failed"));
        assert_eq!(replacement, future);
        for op in [0, u64::MAX] {
            assert_dispatch_error(nucleus, old, op, (27, old.to_wire(), 1));
            assert_console_table(
                nucleus.current_thread_table_mut().unwrap(),
                replacement,
                Some((rights, 0x2222)),
            );
        }
        assert_dispatch_error(nucleus, replacement, 1, (8, 0, 0));
        assert_console_table(
            nucleus.current_thread_table_mut().unwrap(),
            replacement,
            Some((rights, 0x2222)),
        );
        nucleus
            .current_thread_table_mut()
            .unwrap()
            .remove(replacement, FIXTURE_GUARD)
            .unwrap_or_else(|_| panic!("replacement removal failed"));
        assert_dispatch_error(nucleus, old, 0, (27, old.to_wire(), 1));
        assert_console_table(
            nucleus.current_thread_table_mut().unwrap(),
            replacement,
            None,
        );
        assert_dispatch_error(nucleus, replacement, 0, (27, replacement.to_wire(), 2));
        assert_console_table(
            nucleus.current_thread_table_mut().unwrap(),
            replacement,
            None,
        );
    });
}

/// 32-byte-aligned backing for the standalone table test (the carve needs
/// `KeyEntry` alignment, which a plain byte array does not provide).
#[repr(align(32))]
struct Backing<const N: usize>([u8; N]);

#[test_case]
fn table_lookup_still_requires_an_installed_capability() {
    const TABLE_BACKING_SIZE: usize = KeyTable::carve_size(SIZE_BITS);
    static mut TABLE_BACKING: Backing<TABLE_BACKING_SIZE> = Backing([0; TABLE_BACKING_SIZE]);
    // SAFETY: the backing is exclusively owned by this sequential test and
    // covers the full variable-size carve at the table's alignment.
    let table = unsafe {
        let ptr = (&raw mut TABLE_BACKING.0).cast::<u8>();
        KeyTable::initialize(ptr, DomainId(0), SIZE_BITS);
        &mut *(ptr as *mut KeyTable)
    };
    let slot = KeySlot::DEBUG_CONSOLE;
    let never_issued = RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, slot.0, 1);
    assert!(matches!(
        table.lookup(never_issued, FIXTURE_GUARD),
        Err(CapError::InvalidKey {
            key,
            reason: InvalidKeyReason::NeverIssued,
            operand: 0,
        }) if key == never_issued
    ));
    // An address whose guard bits do not match rejects before indexing.
    let invalid = RawKey::new(KeySlot(u32::MAX), 1);
    assert!(matches!(
        table.lookup(invalid, FIXTURE_GUARD),
        Err(CapError::InvalidKey {
            key,
            reason: InvalidKeyReason::GuardMismatch,
            operand: 0,
        }) if key == invalid
    ));

    let key = table
        .insert(slot, console_entry(Rights::all(), 0), FIXTURE_GUARD)
        .unwrap_or_else(|_| panic!("console insertion failed"));
    assert_eq!(key, RawKey::from_parts(FIXTURE_GUARD, SIZE_BITS, slot.0, 1));
    assert_eq!(table.len(), 1);
    let cap = table
        .lookup(key, FIXTURE_GUARD)
        .unwrap_or_else(|_| panic!("installed console not found"));
    assert!(matches!(
        invoke(cap, 1, u64::MAX, u64::MAX),
        Err(CapError::InvalidOperation)
    ));
}
