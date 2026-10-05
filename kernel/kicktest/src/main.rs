#![no_std]
#![no_main]
#![allow(unused)]
#![feature(format_args_nl)]

//! kicktest: the Vesper end-to-end runtime/bootup test kernel.
//!
//! It reuses the real kickstart boot path (early EL2 init, device-tree
//! parsing, nucleus image loading, the EL1 transition, and the initial
//! kernel-state bootstrap) and then runs the capability e2e suite — actual
//! issued-key handoff, Retype/copy/map/unmap, notifications, `EventCount`s,
//! blocking waits through the Bounce fixture, and Thread/AddressSpace
//! retirement — through the real SVC path, with in-guest assertions and the
//! QEMU semihosting exit status as the pass/fail signal
//! (`just test-capability-boot`).
//!
//! The real boot kernel is kickstart; nothing here runs in production
//! images.

use {
    aarch64_cpu::registers::{Readable, TCR_EL1, TTBR0_EL1, TTBR1_EL1},
    cfg_if::cfg_if,
    core::{
        arch::asm,
        mem::size_of,
        panic::PanicInfo,
        slice,
        sync::atomic::{AtomicU64, Ordering},
    },
    kickstart::{
        bootstrap::{
            BOOT_TABLE_GUARD, BOOT_TABLE_SIZE_BITS, BootState, PoolCapacities, bootstrap_nucleus,
            retained_init_memory,
        },
        kickstart_init_el2, print_my_sp,
    },
    libaddress::{PhysAddr, VirtAddr},
    libboot as boot,
    libcpu::endless_sleep,
    libexception::arch::aarch64::{ExceptionOrigin, SavedContext},
    libobject::{
        ASIDPoolKey, CapError, EventCountKey, FrameKey, InvalidKeyReason, KeySlot, KeyTableKey,
        NotificationKey, ObjectType, PageTableKey, RawKey, Rights, UntypedKey,
        address_space::AddressSpaceKey,
        domain::DomainId,
        thread::{ThreadKey, ThreadOp},
    },
    libqemu::semihosting as semi,
    nucleus::{
        api::key_entry::{KeyEntry, ThreadSelector},
        objects::{
            ArchObjects, ArchObjectsImpl, ExecutionContext, KeyTable, Nucleus, ObjectPool, Thread,
            access::{ObjectId, PoolTag},
            arch_objects::AddressSpaceObject,
            completion::PendingState,
        },
    },
};

#[cfg(feature = "debug_kernel")]
use libobject::DebugConsoleKey;

#[cfg(feature = "debug_kernel")]
mod translation;

boot::entry!(boot_main);

/// EL2 entry: run the shared kickstart boot through the EL1 transition, then
/// continue in [`kicktest_run`].
fn boot_main(dtb: u32) -> ! {
    kickstart_init_el2(dtb, kicktest_run as *const u8 as u64)
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    semi::println!("PANICKED: {info}");
    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_failure()
        } else {
            endless_sleep()
        }
    }
}

/// The guard Kickstart picks for the tables the boot test carves at runtime
/// (guarded key-space package, selected 2026-09-23): distinct from the boot
/// table's guard so cross-table key confusion is exercised. Fits the 24 guard
/// bits of a 256-entry table's table-relative address.
const TEST_TABLE_GUARD: u32 = 0xFEE_D42;

/// Compose a boot-table key from a bare slot index and incarnation: the boot
/// guard packed above the index.
fn boot_key(slot: u32, incarnation: u32) -> RawKey {
    RawKey::from_parts(BOOT_TABLE_GUARD, BOOT_TABLE_SIZE_BITS, slot, incarnation)
}

/// The boot-table slot half for a bare index (the guard packed above it).
fn boot_slot(index: u32) -> KeySlot {
    KeySlot((BOOT_TABLE_GUARD << u32::from(BOOT_TABLE_SIZE_BITS)) | index)
}

/// The slot half of a key in one of the boot test's runtime-carved tables.
fn test_slot(index: u32) -> KeySlot {
    KeySlot((TEST_TABLE_GUARD << u32::from(BOOT_TABLE_SIZE_BITS)) | index)
}

/// The trusted boot/Bounce fixture must share one high trap stack, even while
/// their execution stacks differ. This is test-local state, not a syscall ABI.
#[cfg(feature = "debug_kernel")]
static FIXTURE_TRAP_SP: AtomicU64 = AtomicU64::new(0);

#[cfg(feature = "debug_kernel")]
static BOUNCE_N1_KEY: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "debug_kernel")]
static BOUNCE_N2_KEY: AtomicU64 = AtomicU64::new(0);
#[cfg(feature = "debug_kernel")]
static BOUNCE_EC_KEY: AtomicU64 = AtomicU64::new(0);

/// Observe the execution SP on `SP_EL0` and the idle shared `SP_EL1` at the same
/// call depth. Reading `SP_EL1` directly at EL1 is not permitted, so briefly
/// select it without touching memory. This trusted fixture runs with DAIF
/// masked; the assertion precedes the temporary stack-selection change.
#[cfg(feature = "debug_kernel")]
#[inline(never)]
fn fixture_stack_state() -> (u64, u64) {
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

#[cfg(feature = "debug_kernel")]
fn assert_bounce_parked_context(thread: &Thread, stack_bottom: u64, stack_top: u64) {
    let ExecutionContext::Parked { saved, .. } = thread.context else {
        panic!("Bounce has no Thread-resident parked context")
    };
    assert_eq!(saved.origin, ExceptionOrigin::CurrentSp0);
    assert_eq!(saved.spsr_el1 & 0xf, 4, "Bounce must resume as EL1t");
    assert_eq!(
        saved.spsr_el1 & 0x3c0,
        0x3c0,
        "Bounce must retain its DAIF masks"
    );
    assert_eq!(saved.sp & 15, 0);
    assert!(saved.sp >= stack_bottom && saved.sp < stack_top);
    assert_ne!(saved.sp, FIXTURE_TRAP_SP.load(Ordering::Acquire));
    assert_ne!(saved.elr_el1, 0);
    assert_ne!(saved.lr, 0);
    translation::assert_parked_registers(
        &saved,
        RawKey::from_wire(BOUNCE_N2_KEY.load(Ordering::Acquire)),
    );
}

#[cfg(feature = "debug_kernel")]
fn assert_source_selected(
    nucleus: &Nucleus<ArchObjectsImpl>,
    source: ObjectId,
    root: u64,
    asid: u16,
) {
    assert_eq!(nucleus.current_thread, Some(0));
    let thread = nucleus
        .pools
        .threads
        .get_live(0)
        .expect("source Thread missing");
    assert_eq!(thread.address_space, source);
    assert_eq!(thread.context, ExecutionContext::Running);
    nucleus
        .pools
        .arch
        .address_spaces
        .validate(source)
        .unwrap_or_else(|_| panic!("source AddressSpace identity invalid"));
    let address_space = nucleus
        .pools
        .arch
        .address_spaces
        .get_live(usize::from(source.index))
        .expect("source AddressSpace missing");
    assert_eq!(address_space.translation_root, Some(root));
    assert_eq!(address_space.asid, Some(asid));
    assert_eq!(TTBR0_EL1.get(), root | (u64::from(asid) << 48));
}

// DTB should be available to this code through BOOT_INFO records.
pub fn kicktest_run() -> ! {
    semi::println!("kicktest_run: enabled MMU and dropped to EL1");
    print_my_sp();
    #[cfg(feature = "debug_kernel")]
    let (boot_execution_sp, shared_trap_sp) = {
        let stack_state = fixture_stack_state();
        FIXTURE_TRAP_SP.store(stack_state.1, Ordering::Release);
        semi::println!(
            "Thread layout: saved={} context={} thread={} pool(2)={} pool(4)={}",
            size_of::<SavedContext>(),
            size_of::<ExecutionContext>(),
            size_of::<Thread>(),
            ObjectPool::<Thread>::carve_size(2),
            ObjectPool::<Thread>::carve_size(4),
        );
        stack_state
    };

    // ─────────────────────────────────────────────────────────────────────
    // Build the initial kernel state in carved memory (inert nucleus), with
    // the e2e suite's fixture extents: the boot Thread + the Bounce fixture
    // Thread, the boot + two fixture AddressSpaces, the Notification and
    // EventCount pools the suite Retypes from, and the mapping-chain
    // page-table pool (16 slots plus the two fixture roots of the
    // AddressSpace.Retire test).
    // Implementation status: Bounce now has a distinct AddressSpace bound to
    // its own table, so the pool holds boot + Bounce + two retirement fixtures.
    // Translation provisioning also charges source/Bounce image/probe tables;
    // the precise capacity below leaves twelve slots for the refill test.
    // ─────────────────────────────────────────────────────────────────────
    #[cfg(feature = "debug_kernel")]
    let retained_init = retained_init_memory();
    #[cfg(feature = "debug_kernel")]
    let page_table_capacity = translation::page_table_capacity(&retained_init);
    #[cfg(not(feature = "debug_kernel"))]
    let page_table_capacity = 18;
    #[cfg(feature = "debug_kernel")]
    translation::observe_bootstrap();
    let boot = bootstrap_nucleus(&PoolCapacities {
        threads: 2,
        address_spaces: 4,
        notifications: 4,
        event_counts: 4,
        page_tables: page_table_capacity,
        asid_pools: 1,
    });

    #[cfg(feature = "debug_kernel")]
    {
        let BootState {
            nucleus,
            keytable_addr,
            boot_as_id,
            self_table_key,
            boot_untyped_key,
            debug_console_key,
        } = boot;

        assert_eq!(boot_as_id.index, 0);
        assert_eq!(nucleus.pools.threads.capacity(), 2);
        assert!(ObjectPool::<Thread>::carve_size(2) <= 4096);
        assert_eq!(nucleus.pools.arch.address_spaces.capacity(), 4);
        assert_eq!(
            nucleus.pools.arch.page_tables.capacity(),
            page_table_capacity
        );
        let boot_table_binding = nucleus
            .pools
            .arch
            .address_spaces
            .get_live(usize::from(boot_as_id.index))
            .expect("boot AddressSpace missing")
            .keytable();
        assert_eq!(boot_table_binding.address(), keytable_addr);
        assert_eq!(boot_table_binding.size_bits(), BOOT_TABLE_SIZE_BITS);
        {
            // SAFETY: bootstrap retained the initialized, private boot carve;
            // no SVC or mutable table access occurs while this borrow is live.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            assert_eq!(
                boot_table.capacity(),
                1_usize << boot_table_binding.size_bits()
            );
            let self_entry = boot_table
                .lookup(self_table_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("boot self-table capability missing"));
            assert_eq!(
                self_entry.keytable_address().ok(),
                Some(boot_table_binding.address())
            );
            assert_eq!(
                self_entry.keytable_guard_and_size().ok(),
                Some((BOOT_TABLE_GUARD, boot_table_binding.size_bits()))
            );
        }

        // We have domain caps here, can use:
        // Prototype status: use only the key actually issued to this boot Domain.
        let dbg = DebugConsoleKey::from_key(debug_console_key);
        dbg.write(
            "DEBCON| Debug output via capability invocation on domain's debug console capability\n",
        )
        .unwrap_or_else(|error| {
            panic!(
                "Issued debug console key invocation failed: {:?}",
                error.code()
            );
        });

        // Deliberately malformed key for rejection testing, never a slot-only fallback.
        let invalid_key = RawKey::new(debug_console_key.slot(), 0);
        let err = DebugConsoleKey::from_key(invalid_key);
        assert!(matches!(
            err.write("DEBCON| Invalid capability invocation - no output"),
            Err(CapError::InvalidKey {
                key,
                reason: InvalidKeyReason::ZeroIncarnation,
                operand: 0,
            }) if key == invalid_key
        ));

        // Retype a KeyTable from the boot Untyped into the boot table through
        // the real SVC path (the direct map is live, so the carve lands in the
        // boot Untyped's unused watermark range).
        let untyped = UntypedKey::from_key(boot_untyped_key);
        let self_table = KeyTableKey::from_key(self_table_key);
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
            .unwrap_or_else(|error| panic!("boot Retype failed: {:?}", error.code()));
        assert_eq!(new_table_key.slot(), boot_slot(6));
        assert_ne!(new_table_key.incarnation(), 0);

        // The new table is a distinct carved object: CopyDerive the self-table
        // capability into it through the real SVC path.
        let derived_key = self_table
            .copy_derive(
                self_table_key,
                &KeyTableKey::from_key(new_table_key),
                KeySlot(1).0,
                Rights(Rights::DERIVE),
            )
            .unwrap_or_else(|error| panic!("cross-table CopyDerive failed: {:?}", error.code()));
        assert_eq!(derived_key.slot(), test_slot(1));

        // Validation failures leave the Untyped and destination unchanged.
        assert!(matches!(
            untyped.retype(
                ObjectType::THREAD,
                0,
                0,
                1,
                &self_table,
                KeySlot(7).0,
                Rights::all(),
            ),
            Err(CapError::InvalidObjectType(ObjectType::THREAD))
        ));
        assert!(matches!(
            untyped.retype(
                ObjectType::KEY_TABLE,
                8,
                TEST_TABLE_GUARD,
                1,
                &self_table,
                KeySlot(6).0,
                Rights::all(),
            ),
            Err(CapError::SlotOccupied(KeySlot(6)))
        ));
        // A batch beyond the kernel's bound is malformed input, not a
        // resource limit: the transaction's defensive rollback records are
        // stack arrays sized by the bound (selected 2026-09-23).
        assert!(matches!(
            untyped.retype(
                ObjectType::KEY_TABLE,
                8,
                TEST_TABLE_GUARD,
                u32::MAX,
                &self_table,
                KeySlot(8).0,
                Rights::all(),
            ),
            Err(CapError::InvalidOperation)
        ));
        // A single table too large for the boot Untyped's remaining range
        // fails the reservation instead (2^20 entries ≈ 37 MiB of carve);
        // its guard must fit the 12 guard bits a 2^20-entry table leaves.
        assert!(matches!(
            untyped.retype(
                ObjectType::KEY_TABLE,
                20,
                0xFED,
                1,
                &self_table,
                KeySlot(8).0,
                Rights::all(),
            ),
            Err(CapError::InsufficientMemory)
        ));

        // A second Retype must not disturb the first carved table: its
        // storage and bookkeeping survive the next carve (non-overlap).
        let second_table_key = untyped
            .retype(
                ObjectType::KEY_TABLE,
                8,
                TEST_TABLE_GUARD,
                1,
                &self_table,
                KeySlot(7).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("second boot Retype failed: {:?}", error.code()));
        assert_eq!(second_table_key.slot(), boot_slot(7));

        // The first new table still accepts a cross-table derivation after
        // the second carve.
        let derived_again = self_table
            .copy_derive(
                self_table_key,
                &KeyTableKey::from_key(new_table_key),
                KeySlot(2).0,
                Rights(Rights::DERIVE),
            )
            .unwrap_or_else(|error| panic!("post-carve CopyDerive failed: {:?}", error.code()));
        assert_eq!(derived_again.slot(), test_slot(2));

        // Retype a Frame (4 KiB, the AArch64 small-granule baseline) from the
        // boot Untyped through the real SVC path. The kernel sanitizes the
        // carved contents (zeroes them) before installing the capability.
        let frame_key = untyped
            .retype(
                ObjectType::FRAME,
                12,
                0,
                1,
                &self_table,
                KeySlot(10).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("frame Retype failed: {:?}", error.code()));
        assert_eq!(frame_key.slot(), boot_slot(10));

        // Non-granular frame sizes are rejected with the architecture's own
        // error, leaving the table unchanged (slot 11 stays free).
        assert!(matches!(
            untyped.retype(
                ObjectType::FRAME,
                13,
                0,
                1,
                &self_table,
                KeySlot(11).0,
                Rights::all(),
            ),
            Err(CapError::InvalidFrameSize(13))
        ));

        // The frame entry records the aligned absolute carve and the granule.
        let frame_paddr = {
            // SAFETY: keytable_addr names the live boot KeyTable.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            let frame = boot_table
                .lookup(frame_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("frame entry missing"))
                .as_frame()
                .unwrap_or_else(|_| panic!("frame entry is not a Frame cap"));
            assert_eq!(frame.size_bits, 12);
            assert!(!frame.is_device());
            frame.paddr
        };
        assert_eq!(
            frame_paddr % 4096,
            0,
            "the frame carve must be frame-aligned"
        );

        // Sanitization: the carved frame contents were zeroed at retype.
        {
            // SAFETY: the frame lies in the boot Untyped's committed range;
            // the direct map is live.
            let words = unsafe {
                slice::from_raw_parts(
                    PhysAddr::new(frame_paddr).user_to_kernel().as_ptr::<u64>(),
                    4096 / 8,
                )
            };
            assert!(
                words.iter().all(|word| *word == 0),
                "frame contents must be zeroed at retype"
            );
        }

        // A subsequent same-size frame carve continues the watermark exactly:
        // no gap and no overlap between consecutive frame carves.
        let second_frame_key = untyped
            .retype(
                ObjectType::FRAME,
                12,
                0,
                1,
                &self_table,
                KeySlot(11).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("second frame Retype failed: {:?}", error.code()));
        assert_eq!(second_frame_key.slot(), boot_slot(11));
        let second_frame_paddr = {
            // SAFETY: see above.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            boot_table
                .lookup(second_frame_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("second frame entry missing"))
                .as_frame()
                .unwrap_or_else(|_| panic!("second frame entry is not a Frame cap"))
                .paddr
        };
        assert_eq!(
            second_frame_paddr,
            frame_paddr + 4096,
            "consecutive frame carves must neither gap nor overlap"
        );

        // A larger-granule frame (2 MiB) aligns its own carve and is zeroed
        // too (spot-checked at the first words).
        let large_frame_key = untyped
            .retype(
                ObjectType::FRAME,
                21,
                0,
                1,
                &self_table,
                KeySlot(12).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("large frame Retype failed: {:?}", error.code()));
        assert_eq!(large_frame_key.slot(), boot_slot(12));
        let large_frame_paddr = {
            // SAFETY: see above.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            boot_table
                .lookup(large_frame_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("large frame entry missing"))
                .as_frame()
                .unwrap_or_else(|_| panic!("large frame entry is not a Frame cap"))
                .paddr
        };
        assert_eq!(
            large_frame_paddr % (2 * 1024 * 1024),
            0,
            "the 2 MiB frame carve must be 2 MiB-aligned"
        );
        assert!(
            large_frame_paddr >= second_frame_paddr + 4096,
            "the large frame must not overlap the earlier carves"
        );
        {
            // SAFETY: see above.
            let words = unsafe {
                slice::from_raw_parts(
                    PhysAddr::new(large_frame_paddr)
                        .user_to_kernel()
                        .as_ptr::<u64>(),
                    8,
                )
            };
            assert!(
                words.iter().all(|word| *word == 0),
                "the large frame must be zeroed at retype"
            );
        }

        // ─────────────────────────────────────────────────────────────────
        // Untyped split: Retype an Untyped into smaller Untypeds through
        // the real SVC path, then carve from a child.
        // ─────────────────────────────────────────────────────────────────

        // The pre-split watermark locates the children: a 4 KiB child is
        // its own alignment, so the first child's absolute carve address is
        // the next 4 KiB boundary at or past the watermark.
        let (boot_paddr, pre_split_wm) = {
            // SAFETY: keytable_addr names the live boot KeyTable.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            let region = boot_table
                .lookup(boot_untyped_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("boot Untyped entry missing"))
                .as_untyped()
                .unwrap_or_else(|_| panic!("boot Untyped entry is not a region"));
            (
                region.paddr,
                u64::try_from(region.watermark_bytes()).unwrap(),
            )
        };
        let first_child_paddr = (boot_paddr + pre_split_wm + 4095) & !4095_u64;

        let split_children = untyped
            .retype(
                ObjectType::UNTYPED,
                12,
                0,
                2,
                &self_table,
                KeySlot(56).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("Untyped split failed: {:?}", error.code()));
        assert_eq!(split_children.slot(), boot_slot(56));

        // Both children are inline regions: consecutive 4 KiB ranges starting
        // at the aligned continuation of the boot Untyped's watermark, each
        // fully unused, and the parent's watermark advanced by exactly the
        // two child regions — no gap, no overlap.
        {
            // SAFETY: see above.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            let child0 = boot_table
                .lookup(split_children, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("first split child missing"))
                .as_untyped()
                .unwrap_or_else(|_| panic!("first split child is not a region"));
            assert_eq!(child0.paddr, first_child_paddr);
            assert_eq!(child0.size_bits, 12);
            assert!(!child0.is_device);
            assert_eq!(child0.watermark_bytes(), 0);
            let child1 = boot_table
                .lookup(boot_key(57, split_children.incarnation()), BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("second split child missing"))
                .as_untyped()
                .unwrap_or_else(|_| panic!("second split child is not a region"));
            assert_eq!(child1.paddr, first_child_paddr + 4096);
            assert_eq!(child1.size_bits, 12);
            assert_eq!(child1.watermark_bytes(), 0);
            let parent = boot_table
                .lookup(boot_untyped_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("boot Untyped entry missing"))
                .as_untyped()
                .unwrap_or_else(|_| panic!("boot Untyped entry is not a region"));
            assert_eq!(
                parent.watermark_bytes(),
                usize::try_from(first_child_paddr + 2 * 4096 - boot_paddr).unwrap(),
                "the split must advance the watermark by exactly the two children"
            );
        }

        // The second child is a working allocation source: a 4 KiB Frame
        // carved from it through the real SVC path lands exactly at the
        // child's base (its watermark was zero) and advances it.
        let child1_key = boot_key(57, split_children.incarnation());
        let child_frame_key = UntypedKey::from_key(child1_key)
            .retype(
                ObjectType::FRAME,
                12,
                0,
                1,
                &self_table,
                KeySlot(58).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("carve from split child failed: {:?}", error.code()));
        assert_eq!(child_frame_key.slot(), boot_slot(58));
        {
            // SAFETY: see above.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            let frame = boot_table
                .lookup(child_frame_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("child frame entry missing"))
                .as_frame()
                .unwrap_or_else(|_| panic!("child frame entry is not a Frame cap"));
            assert_eq!(frame.paddr, first_child_paddr + 4096);
            let child1 = boot_table
                .lookup(child1_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("second split child missing"))
                .as_untyped()
                .unwrap_or_else(|_| panic!("second split child is not a region"));
            assert_eq!(child1.watermark_bytes(), 4096);
        }

        // Rejections from the first child: a sub-granular split reports the
        // child's own `size_bits`, and an oversized split cannot fit; both
        // leave the child unchanged.
        let child0_key = split_children;
        assert!(matches!(
            UntypedKey::from_key(child0_key).retype(
                ObjectType::UNTYPED,
                3,
                0,
                1,
                &self_table,
                KeySlot(59).0,
                Rights::all(),
            ),
            Err(CapError::InvalidSize(3))
        ));
        assert!(matches!(
            UntypedKey::from_key(child0_key).retype(
                ObjectType::UNTYPED,
                13,
                0,
                1,
                &self_table,
                KeySlot(59).0,
                Rights::all(),
            ),
            Err(CapError::InsufficientMemory)
        ));
        {
            // SAFETY: see above.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            let child0 = boot_table
                .lookup(child0_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("first split child missing"))
                .as_untyped()
                .unwrap_or_else(|_| panic!("first split child is not a region"));
            assert_eq!(child0.watermark_bytes(), 0);
        }

        // A region whose base is not aligned still yields an aligned carve:
        // the absolute carve address (base + watermark) is aligned up, not
        // just the watermark. Fabricate a RAM region at a deliberately
        // misaligned free physical address (just past the boot Untyped's
        // committed watermark) and retype from it through the real SVC path.
        let (boot_paddr, boot_wm) = {
            // SAFETY: keytable_addr names the live boot KeyTable.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            let region = boot_table
                .lookup(boot_untyped_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("boot Untyped entry missing"))
                .as_untyped()
                .unwrap_or_else(|_| panic!("boot Untyped entry is not a region"));
            (
                region.paddr,
                u64::try_from(region.watermark_bytes()).unwrap(),
            )
        };
        // The committed watermark is object-aligned, so +24 keeps the address
        // inside the boot Untyped's free RAM while making the region base
        // misaligned for the KeyTable alignment (32 under the current layout;
        // 16 is the watermark encoding granularity — mirror the kernel's
        // max(align_of, MIN_ALIGN)).
        let align = u64::try_from(core::mem::align_of::<KeyTable>().max(16)).unwrap();
        let misaligned_base = boot_paddr + boot_wm + 24;
        assert_ne!(misaligned_base % align, 0, "fixture must be misaligned");
        let misaligned_untyped_key = {
            // SAFETY: see above.
            let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
            boot_table
                .insert(
                    KeySlot(8),
                    KeyEntry::new_untyped(misaligned_base, 14, false, Rights::all()),
                    BOOT_TABLE_GUARD,
                )
                .unwrap_or_else(|_| panic!("misaligned region install failed"))
        };
        let carved_key = UntypedKey::from_key(misaligned_untyped_key)
            .retype(
                ObjectType::KEY_TABLE,
                8,
                TEST_TABLE_GUARD,
                1,
                &self_table,
                KeySlot(9).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("misaligned-base Retype failed: {:?}", error.code()));
        assert_eq!(carved_key.slot(), boot_slot(9));

        // Read the carved address back: it must be the aligned base, not the
        // region's misaligned start. The capability stores the kernel-window
        // address of the carve; convert it back to physical to compare.
        let carved_paddr = {
            // SAFETY: see above.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            let window_addr = boot_table
                .lookup(carved_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("carved entry missing"))
                .keytable_address()
                .unwrap_or_else(|_| panic!("carved entry is not a KeyTable cap"));
            VirtAddr::new(window_addr).kernel_to_user().as_u64()
        };
        assert_eq!(
            carved_paddr,
            misaligned_base + (align - misaligned_base % align) % align,
            "the carve must start at the aligned absolute address"
        );

        // The aligned carve produced a live table: derive into it.
        let derived_misaligned = self_table
            .copy_derive(
                self_table_key,
                &KeyTableKey::from_key(carved_key),
                KeySlot(1).0,
                Rights(Rights::DERIVE),
            )
            .unwrap_or_else(|error| {
                panic!("misaligned-base CopyDerive failed: {:?}", error.code())
            });
        assert_eq!(derived_misaligned.slot(), test_slot(1));

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
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            let id = boot_table
                .lookup(root_pt_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("root PageTable entry missing"))
                .object_id()
                .unwrap_or_else(|_| panic!("root PageTable entry has no identity"));
            let paddr = nucleus
                .pools
                .arch
                .page_tables
                .get_live(usize::from(id.index))
                .unwrap_or_else(|| panic!("root PageTable metadata missing"))
                .paddr;
            assert_eq!(paddr % 4096, 0, "the table carve must be 4 KiB-aligned");
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
                .pools
                .arch
                .address_spaces
                .get_live(0)
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
                .pools
                .arch
                .address_spaces
                .get_live(0)
                .unwrap_or_else(|| panic!("boot AddressSpace missing"));
            assert_eq!(address_space.asid, Some(1));
        }
        // A second assignment to the same AddressSpace is rejected: one ASID
        // per translation context.
        assert!(matches!(
            boot_asid_pool.assign(boot_as_key),
            Err(CapError::AlreadyMapped)
        ));

        // Kickstart installs current-relative Thread.Return authority, not a
        // named Thread or an Invocation. Return execution still awaits PPC.
        {
            // SAFETY: keytable_addr names the live carved boot KeyTable.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            let return_key = boot_key(KeySlot::THREAD_RETURN.0, 1);
            let return_entry = boot_table
                .lookup(return_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("return key missing from its well-known slot"));
            assert_eq!(return_entry.object_type(), ObjectType::THREAD);
            assert!(return_entry.is_thread_return_key());
            assert!(matches!(
                return_entry.thread_selector(),
                Ok(ThreadSelector::CurrentReturnOnly)
            ));
            assert_eq!(return_entry.rights(), Rights::empty());
            assert!(matches!(
                return_entry.object_id(),
                Err(CapError::InvalidOperation)
            ));
            assert!(matches!(
                return_entry.invocation_target(),
                Err(CapError::TypeMismatch { expected, found })
                    if expected == ObjectType::INVOCATION && found == ObjectType::THREAD
            ));
        }
        for op in [
            ThreadOp::Return,
            ThreadOp::Grant,
            ThreadOp::Suspend,
            ThreadOp::Resume,
            ThreadOp::Retire,
        ] {
            let (status, word1, word2): (u64, u64, u64);
            // SAFETY: test-only ordinary rejected SVC, not a PPC Return
            // wrapper. No continuation or nonlocal completion is implemented.
            unsafe {
                asm!(
                    "svc #0",
                    inlateout("x0") boot_key(KeySlot::THREAD_RETURN.0, 1).to_wire() => status,
                    inlateout("x1") op as u64 => word1,
                    inlateout("x2") 0_u64 => word2,
                    in("x3") 0_u64,
                    in("x4") 0_u64,
                    in("x5") 0_u64,
                    in("x6") 0_u64,
                    in("x7") 0_u64,
                    options(nostack),
                );
            }
            assert_eq!((status, word1, word2), CapError::InvalidOperation.code());
        }

        // AddressSpace.CreateInvocation installs a CALL-only capability into
        // the selected KeyTable without validating the supplied entry address.
        let invocation_key = boot_as
            .create_invocation(0x1234, &self_table, KeySlot(62))
            .unwrap_or_else(|error| {
                panic!("AddressSpace.CreateInvocation failed: {:?}", error.code())
            });
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
        }

        // Invocation always has a nonzero entry. A zero function address
        // cannot construct an Invocation (or current-relative Thread authority)
        // from userspace, and is rejected before resolution or installation.
        let table_len_before_zero = {
            // SAFETY: keytable_addr names the live carved boot KeyTable.
            unsafe { &*(keytable_addr as *const KeyTable) }.len()
        };
        for slot in [KeySlot(62), KeySlot(63)] {
            assert!(matches!(
                boot_as.create_invocation(0, &self_table, slot),
                Err(CapError::InvalidPointer)
            ));
        }
        {
            // SAFETY: keytable_addr names the live carved boot KeyTable.
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
            assert_eq!(boot_table.len(), table_len_before_zero);
            let existing = boot_table
                .lookup(invocation_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|error| {
                    panic!("zero-address rejection lost cap: {:?}", error.code())
                });
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
        let cross_table_invocation = boot_as
            .create_invocation(0x5678, &KeyTableKey::from_key(new_table_key), KeySlot(3))
            .unwrap_or_else(|error| {
                panic!("cross-table CreateInvocation failed: {:?}", error.code())
            });
        assert_eq!(cross_table_invocation.slot(), test_slot(3));
        {
            // SAFETY: export_table_addr came from its live kernel-issued cap.
            let export_table = unsafe { &*(export_table_addr as *const KeyTable) };
            let Ok(invocation) = export_table.lookup(cross_table_invocation, TEST_TABLE_GUARD)
            else {
                panic!("cross-table Invocation key did not resolve in its target table");
            };
            let Ok((target, function_address)) = invocation.invocation_target() else {
                panic!("cross-table export is not an Invocation");
            };
            assert_eq!(target, boot_as_id);
            assert_eq!(function_address.get(), 0x5678);
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
        assert!(matches!(
            boot_as.create_invocation(0x5678, &self_table, KeySlot(62)),
            Err(CapError::SlotOccupied(KeySlot(62)))
        ));
        assert!(matches!(
            boot_as.create_invocation(0x5678, &self_table, KeySlot(300)),
            Err(CapError::InvalidSlot(KeySlot(300)))
        ));
        assert!(matches!(
            boot_as.create_invocation(
                0x5678,
                &KeyTableKey::from_key(boot_untyped_key),
                KeySlot(63)
            ),
            Err(CapError::TypeMismatch { .. })
        ));

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
        assert!(matches!(
            AddressSpaceKey::from_key(no_grant_address_space).create_invocation(
                0x5678,
                &self_table,
                KeySlot(64)
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
        assert!(matches!(
            boot_as.create_invocation(
                0x5678,
                &KeyTableKey::from_key(no_install_table),
                KeySlot(65),
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

        // ─────────────────────────────────────────────────────────────────
        // Notification: Retype-creatable synchronization state (2026-09-16)
        // ─────────────────────────────────────────────────────────────────

        // Retype two Notifications: pure kernel state allocated from the
        // bootstrap-carved pool — no Untyped bytes are carved.
        let notification_key = untyped
            .retype(
                ObjectType::NOTIFICATION,
                0,
                0,
                2,
                &self_table,
                KeySlot(16).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("notification Retype failed: {:?}", error.code()));
        assert_eq!(notification_key.slot(), boot_slot(16));

        // A nonzero size_bits is rejected before any pool slot is taken.
        assert!(matches!(
            untyped.retype(
                ObjectType::NOTIFICATION,
                12,
                0,
                1,
                &self_table,
                KeySlot(18).0,
                Rights::all(),
            ),
            Err(CapError::InvalidSize(12))
        ));

        let notification = NotificationKey::from_key(notification_key);

        // Signal coalesces distinct bits into the bitmap; Poll consumes all
        // the pending bits at once.
        notification
            .signal(0b0110)
            .unwrap_or_else(|error| panic!("Notification.Signal failed: {:?}", error.code()));
        notification.signal(0b0001).unwrap_or_else(|error| {
            panic!("second Notification.Signal failed: {:?}", error.code())
        });
        assert_eq!(
            notification
                .poll()
                .unwrap_or_else(|error| panic!("Notification.Poll failed: {:?}", error.code())),
            0b0111
        );
        // Nothing pending: Poll returns zero, never blocking.
        assert_eq!(
            notification.poll().unwrap_or_else(|error| panic!(
                "second Notification.Poll failed: {:?}",
                error.code()
            )),
            0
        );

        // Wait tests follow complete source/Bounce provisioning below, so even
        // the already-satisfied/rejected waits enter from the bound source root.

        // ─────────────────────────────────────────────────────────────────
        // Blocking Wait end-to-end: the Bounce fixture domain (N4-A, 2026-09-16)
        // ─────────────────────────────────────────────────────────────────

        // N1: the notification the boot domain blocks on. N2: the one
        // Bounce parks on between its rounds. EC: the event count whose
        // awaits the boot domain blocks on and Bounce advances. All carved
        // through the public Retype path.
        let n1_key = untyped
            .retype(
                ObjectType::NOTIFICATION,
                0,
                0,
                1,
                &self_table,
                KeySlot(18).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("N1 Retype failed: {:?}", error.code()));
        let n2_key = untyped
            .retype(
                ObjectType::NOTIFICATION,
                0,
                0,
                1,
                &self_table,
                KeySlot(19).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("N2 Retype failed: {:?}", error.code()));
        let ec_key = untyped
            .retype(
                ObjectType::EVENT_COUNT,
                0,
                0,
                1,
                &self_table,
                KeySlot(37).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("EC Retype failed: {:?}", error.code()));

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
        let bounce_stack_key = untyped
            .retype(
                ObjectType::FRAME,
                12,
                0,
                8,
                &self_table,
                KeySlot(41).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("Bounce stack Retype failed: {:?}", error.code()));
        let (bounce_stack_paddr, _bounce_stack_size) = FrameKey::from_key(bounce_stack_key)
            .get_extent()
            .unwrap_or_else(|error| panic!("Bounce stack GetExtent failed: {:?}", error.code()));
        let bounce_stack_bottom = PhysAddr::new(bounce_stack_paddr).user_to_kernel().as_u64();
        let bounce_stack_top = bounce_stack_bottom + 8 * 4096;
        assert_eq!(bounce_stack_top & 15, 0);
        assert_ne!(bounce_stack_top, shared_trap_sp);
        for index in 0..8_u32 {
            let frame = FrameKey::from_key(boot_key(41 + index, bounce_stack_key.incarnation()));
            assert_eq!(
                frame.get_extent().ok(),
                Some((bounce_stack_paddr + u64::from(index) * 4096, 4096)),
                "Bounce execution stack must be eight contiguous accounted Frames"
            );
        }

        // Bounce's capability table, carved through the public Retype path.
        // The fixture's slots (40–48) sit outside the later pool-refill
        // test's destination range (24–35), which requires those slots
        // vacant.
        let bounce_table_key = untyped
            .retype(
                ObjectType::KEY_TABLE,
                8,
                TEST_TABLE_GUARD,
                1,
                &self_table,
                KeySlot(40).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("Bounce KeyTable Retype failed: {:?}", error.code()));
        let bounce_table_addr = {
            // SAFETY: the boot table is the live carved boot KeyTable.
            let entry = unsafe { &*(keytable_addr as *const KeyTable) }
                .lookup(bounce_table_key, BOOT_TABLE_GUARD)
                .unwrap_or_else(|_| panic!("Bounce KeyTable entry missing"));
            entry
                .keytable_address()
                .unwrap_or_else(|_| panic!("Bounce KeyTable entry is not carved"))
        };

        // Bootstrap grant: install Bounce's notification keys (the boot
        // test's authority delegated kernel-privately by the bootstrap
        // builder, like the boot console grant).
        {
            // SAFETY: the boot table is the live carved boot KeyTable.
            let boot_table_ref = unsafe { &*(keytable_addr as *const KeyTable) };
            let n1_id = boot_table_ref
                .lookup(n1_key, BOOT_TABLE_GUARD)
                .and_then(KeyEntry::object_id)
                .unwrap_or_else(|_| panic!("N1 entry missing or not a pool identity"));
            let n2_id = boot_table_ref
                .lookup(n2_key, BOOT_TABLE_GUARD)
                .and_then(KeyEntry::object_id)
                .unwrap_or_else(|_| panic!("N2 entry missing or not a pool identity"));
            let ec_id = boot_table_ref
                .lookup(ec_key, BOOT_TABLE_GUARD)
                .and_then(KeyEntry::object_id)
                .unwrap_or_else(|_| panic!("EC entry missing or not a pool identity"));
            // SAFETY: Bounce's table is the freshly carved, live KeyTable.
            let bounce_table = unsafe { &mut *(bounce_table_addr as *mut KeyTable) };
            // The self-table capability anchors Bounce's invocations: the
            // syscall entry sources the caller's own-table guard from this
            // well-known slot (guarded key-space package, selected 2026-09-23).
            bounce_table
                .insert(
                    KeySlot::SELF_KEYTABLE,
                    KeyEntry::new_keytable(
                        bounce_table_addr,
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
            let bounce_n1 = bounce_table
                .insert(
                    KeySlot(5),
                    KeyEntry::new::<nucleus::objects::Notification>(n1_id, Rights::all(), 0),
                    TEST_TABLE_GUARD,
                )
                .unwrap_or_else(|failure| {
                    panic!("Bounce N1 grant failed: {:?}", failure.error.code())
                });
            let bounce_n2 = bounce_table
                .insert(
                    KeySlot(6),
                    KeyEntry::new::<nucleus::objects::Notification>(n2_id, Rights::all(), 0),
                    TEST_TABLE_GUARD,
                )
                .unwrap_or_else(|failure| {
                    panic!("Bounce N2 grant failed: {:?}", failure.error.code())
                });
            // The EventCount sits at slot 7: slots 1–4 are the well-known
            // layout (return key, self AddressSpace, parent Thread, self
            // KeyTable) and must not be disturbed by fixture grants.
            let bounce_ec = bounce_table
                .insert(
                    KeySlot(7),
                    KeyEntry::new::<nucleus::objects::EventCount>(ec_id, Rights::all(), 0),
                    TEST_TABLE_GUARD,
                )
                .unwrap_or_else(|failure| {
                    panic!("Bounce EC grant failed: {:?}", failure.error.code())
                });
            // Hand over the actual recipient-local keys, including returned
            // incarnations; slot conventions alone cannot mint authority.
            BOUNCE_N1_KEY.store(bounce_n1.to_wire(), Ordering::Release);
            BOUNCE_N2_KEY.store(bounce_n2.to_wire(), Ordering::Release);
            BOUNCE_EC_KEY.store(bounce_ec.to_wire(), Ordering::Release);
        }

        // Allocate Bounce's Thread and queue it runnable: it starts only
        // when the boot thread blocks. Bounce executes in the boot
        // AddressSpace (fixture threads need no private translation context).
        // Implementation status: the preceding shared-boot-AddressSpace setup
        // is superseded by a distinct AddressSpace bound to Bounce's table at
        // provisioning, before Thread creation. Bounce remains an EL1
        // kernel-high-map fixture: there is no actual translation-context
        // switch, protected isolation proof, or PPC migration trial here.
        // Implementation status update: the complete linked low image now gets
        // a distinct root/ASID below, before runnable admission. Execution still
        // uses trusted EL1t/high SP_EL0 and invariant high SP_EL1; the two-Thread
        // switch is neither a confinement proof nor same-Thread PPC migration.
        // SAFETY: Retype initialized the full private carve; its accounted
        // backing is never relocated, reclaimed, or reinitialized while the
        // binding is live, including after Bounce's Thread is retired.
        let bounce_table_binding = unsafe { (&*(bounce_table_addr as *const KeyTable)).binding() };
        assert_ne!(bounce_table_binding.address(), boot_table_binding.address());
        assert_eq!(bounce_table_binding.size_bits(), BOOT_TABLE_SIZE_BITS);
        let (bounce_as_id, bounce_as) = nucleus
            .pools
            .arch
            .address_spaces
            .allocate(ArchObjectsImpl::new_address_space(bounce_table_binding))
            .expect("no Bounce AddressSpace slot");
        assert_eq!(bounce_as_id.index, 1);
        assert_ne!(bounce_as_id, boot_as_id);
        assert_eq!(bounce_as.keytable().address(), bounce_table_addr);
        assert_eq!(bounce_as.keytable().size_bits(), BOOT_TABLE_SIZE_BITS);
        assert!(bounce_as.translation_root.is_none());
        assert!(bounce_as.asid.is_none());
        let bounce_as_key = {
            // SAFETY: both initialized private tables have retained accounted
            // carves; the source and target are distinct, and neither borrow
            // survives a capability invocation.
            let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
            let key = boot_table
                .insert(
                    KeySlot(69),
                    KeyEntry::new::<<ArchObjectsImpl as ArchObjects>::AddressSpace>(
                        bounce_as_id,
                        Rights::all(),
                        0,
                    ),
                    BOOT_TABLE_GUARD,
                )
                .unwrap_or_else(|failure| {
                    panic!("Bounce AS grant failed: {:?}", failure.error.code())
                });
            // SAFETY: distinct retained initialized Bounce table, exclusively
            // borrowed for this bootstrap grant.
            unsafe { &mut *(bounce_table_addr as *mut KeyTable) }
                .insert(
                    KeySlot::SELF_ADDRESS_SPACE,
                    KeyEntry::new::<<ArchObjectsImpl as ArchObjects>::AddressSpace>(
                        bounce_as_id,
                        Rights::all(),
                        0,
                    ),
                    TEST_TABLE_GUARD,
                )
                .unwrap_or_else(|failure| {
                    panic!("Bounce self-AS grant failed: {:?}", failure.error.code())
                });
            key
        };
        let bounce_root_key = translation::Provisioner {
            untyped: &untyped,
            self_table: &self_table,
            boot_table_addr: keytable_addr,
            retained: &retained_init,
            source_as: boot_as_key,
            bounce_as: bounce_as_key,
        }
        .provision([root_pt_key, l1_pt_key, l2_pt_key], bounce_table_key);
        let bounce_hardware_asid = boot_asid_pool
            .assign(bounce_as_key)
            .unwrap_or_else(|error| panic!("Bounce ASID assignment failed: {:?}", error.code()));
        assert_eq!(bounce_hardware_asid, 2);
        let source_root = nucleus
            .pools
            .arch
            .address_spaces
            .get_live(0)
            .expect("source AS missing")
            .translation_root
            .expect("source root missing");
        let bounce_root = nucleus
            .pools
            .arch
            .address_spaces
            .get_live(1)
            .expect("Bounce AS missing")
            .translation_root
            .expect("Bounce root missing");
        translation::verify_retained(&retained_init, source_root, bounce_root);
        translation::bind_contexts(source_root, bound_asid, bounce_root, bounce_hardware_asid);
        // The source must run under its own ASID-1 root before the first wait.
        // Never confuse this bound context with the raw ASID-0 bootstrap TTBR.
        boot_as
            .activate()
            .unwrap_or_else(|error| panic!("early source Activate failed: {:?}", error.code()));
        assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);
        translation::observe_source();
        assert!(matches!(
            AddressSpaceKey::from_key(bounce_as_key).activate(),
            Err(CapError::InvalidOperation)
        ));
        assert_eq!(TTBR0_EL1.get(), source_root | (u64::from(bound_asid) << 48));

        // An already-satisfied Wait consumes and returns the bits
        // immediately through the real SVC path.
        notification
            .signal(0b1)
            .unwrap_or_else(|error| panic!("third Notification.Signal failed: {:?}", error.code()));
        assert_eq!(
            notification
                .wait(NotificationKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!(
                    "satisfied Notification.Wait failed: {:?}",
                    error.code()
                )),
            0b1
        );

        // A wait that would block now blocks for real (completion foundation,
        // 2026-09-16): the end-to-end proof below parks the boot domain and
        // resumes it through the Bounce fixture domain. A finite timeout is
        // still rejected: the time subsystem does not exist yet.
        assert!(matches!(
            notification.wait(1_000_000),
            Err(CapError::InvalidOperation)
        ));
        assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);
        let (bounce_id, _bounce_thread) = nucleus
            .pools
            .threads
            .allocate(Thread {
                address_space: bounce_as_id,
                context: ExecutionContext::NotStarted {
                    saved: SavedContext::el1t(bounce_entry as *const () as u64, bounce_stack_top),
                },
            })
            .unwrap_or_else(|| panic!("no Bounce Thread slot"));
        assert_eq!(bounce_id.index, 1);
        assert!(nucleus.scheduler.push(bounce_id.index));

        // The boot thread blocks on N1: this SVC does not return — the
        // kernel parks it, starts Bounce (which signals N1 and parks on N2),
        // then resumes the boot thread with the delivered bitmap.
        let received = translation::notification_wait(n1_key).unwrap_or_else(|error| {
            panic!("blocking Notification.Wait failed: {:?}", error.code())
        });
        assert_eq!(received, BOUNCE_MAGIC_BITS);
        assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);
        translation::observe_source();
        assert_eq!(fixture_stack_state(), (boot_execution_sp, shared_trap_sp));
        // Bounce is parked on N2; the boot thread resumed with the bits.
        let first_bounce_parked = {
            let bounce = nucleus
                .pools
                .threads
                .get_live(usize::from(bounce_id.index))
                .unwrap_or_else(|| panic!("Bounce Domain missing"));
            assert_bounce_parked_context(bounce, bounce_stack_bottom, bounce_stack_top);
            assert_eq!(bounce.address_space, bounce_as_id);
            assert_eq!(
                nucleus
                    .pools
                    .arch
                    .address_spaces
                    .get_live(usize::from(bounce.address_space.index))
                    .expect("Bounce AddressSpace missing")
                    .keytable()
                    .address(),
                bounce_table_binding.address()
            );
            bounce.context
        };

        // ─────────────────────────────────────────────────────────────────
        // EventCount end-to-end (2026-09-18): monotonic counting, the
        // selected overflow policy, and blocking Await through the Bounce
        // fixture (broadcast wakeups, error wakeups)
        // ─────────────────────────────────────────────────────────────────

        let event_count = EventCountKey::from_key(ec_key);

        // A fresh counter reads zero.
        assert_eq!(
            event_count
                .read()
                .unwrap_or_else(|error| panic!("EventCount.Read failed: {:?}", error.code())),
            0
        );
        // Every advance is counted and returns the new value.
        assert_eq!(
            event_count
                .advance(7)
                .unwrap_or_else(|error| panic!("EventCount.Advance failed: {:?}", error.code())),
            7
        );
        assert_eq!(
            event_count.read().unwrap_or_else(|error| panic!(
                "second EventCount.Read failed: {:?}",
                error.code()
            )),
            7
        );
        // Zero is invalid: an advance must strictly increase (selected
        // 2026-09-18).
        assert!(matches!(
            event_count.advance(0),
            Err(CapError::InvalidOperation)
        ));
        // Overflow rejects, leaves the counter unchanged, and carries the
        // shared status (selected 2026-09-18). No waiter is queued here, so
        // only the advancer observes the error.
        assert!(matches!(
            event_count.advance(u64::MAX),
            Err(CapError::CounterOverflow)
        ));
        assert_eq!(
            event_count
                .read()
                .unwrap_or_else(|error| panic!("third EventCount.Read failed: {:?}", error.code())),
            7
        );
        // An already-satisfied await returns the current value without
        // blocking; awaiting does not consume the counter.
        assert_eq!(
            event_count
                .await_ge(7, EventCountKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!(
                    "satisfied EventCount.Await failed: {:?}",
                    error.code()
                )),
            7
        );
        // A finite timeout is still rejected: the time subsystem does not
        // exist yet.
        assert!(matches!(
            event_count.await_ge(100, 1_000_000),
            Err(CapError::InvalidOperation)
        ));

        // The preceding nonblocking SVCs reused the shared trap stack while
        // Bounce stayed parked. Its entire saved state and wait identity must
        // remain unchanged, not merely its completion-result registers.
        assert_eq!(
            nucleus
                .pools
                .threads
                .get_live(usize::from(bounce_id.index))
                .expect("parked Bounce missing")
                .context,
            first_bounce_parked
        );
        assert_eq!(fixture_stack_state(), (boot_execution_sp, shared_trap_sp));

        // Blocking Await end-to-end: request Bounce's +3 advance by
        // signaling N2 (bit 0), then block on target 10. Bounce advances the
        // counter, this domain's record completes with the new value, and
        // Bounce parks again.
        NotificationKey::from_key(n2_key)
            .signal(0b1)
            .unwrap_or_else(|error| panic!("N2 trigger signal failed: {:?}", error.code()));
        assert_eq!(
            translation::event_count_await(ec_key, 10).unwrap_or_else(|error| {
                panic!("blocking EventCount.Await failed: {:?}", error.code())
            }),
            10
        );
        assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);
        translation::observe_source();
        assert_eq!(
            event_count.read().unwrap_or_else(|error| panic!(
                "fourth EventCount.Read failed: {:?}",
                error.code()
            )),
            10
        );

        assert_eq!(fixture_stack_state(), (boot_execution_sp, shared_trap_sp));
        assert_bounce_parked_context(
            nucleus
                .pools
                .threads
                .get_live(usize::from(bounce_id.index))
                .expect("Bounce missing after successful Await"),
            bounce_stack_bottom,
            bounce_stack_top,
        );

        // Overflow wakes blocked waiters with the shared error (selected
        // 2026-09-18): request Bounce's overflowing advance (bit 1), then
        // block on an unreachable target. The advance completes the await
        // with `CounterOverflow`, the counter stays unchanged, and Bounce
        // parks for good.
        NotificationKey::from_key(n2_key)
            .signal(0b10)
            .unwrap_or_else(|error| panic!("N2 overflow trigger failed: {:?}", error.code()));
        assert!(matches!(
            translation::event_count_await(ec_key, u64::MAX - 2),
            Err(CapError::CounterOverflow)
        ));
        assert_eq!(
            event_count
                .read()
                .unwrap_or_else(|error| panic!("fifth EventCount.Read failed: {:?}", error.code())),
            10
        );
        assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);
        translation::observe_source();
        translation::assert_rounds();
        assert_eq!(fixture_stack_state(), (boot_execution_sp, shared_trap_sp));
        // Bounce is parked on N2 for good; the boot domain resumed with the
        // error completion.
        {
            let bounce = nucleus
                .pools
                .threads
                .get_live(usize::from(bounce_id.index))
                .unwrap_or_else(|| panic!("Bounce Domain missing"));
            assert_bounce_parked_context(bounce, bounce_stack_bottom, bounce_stack_top);
        }

        // ─────────────────────────────────────────────────────────────────
        // Thread.Retire end-to-end: the Thread-control teardown trigger —
        // cancel every pending record naming the target as waiter, purge its
        // queued wakeup, reclaim its pool slot — through the real SVC path
        // under `RETIRE` authority.
        // ─────────────────────────────────────────────────────────────────
        {
            // Bounce is parked on N2 with a Waiting record: the canonical
            // teardown state of a blocked Thread.
            let ExecutionContext::Parked { saved, record } = nucleus
                .pools
                .threads
                .get_live(usize::from(bounce_id.index))
                .unwrap_or_else(|| panic!("Bounce Domain missing"))
                .context
            else {
                panic!("Bounce is not parked")
            };
            assert_eq!(
                nucleus.pending.state(record).ok(),
                Some(PendingState::Waiting)
            );
            assert_eq!(nucleus.pending.waiter(record).ok(), Some(bounce_id));

            // Bootstrap grants: Bounce's Thread capability in the boot table,
            // kernel-privately (like the boot console grant) — one with full
            // rights and one without `RETIRE`, so the authority check is
            // observable through the real SVC path. Thread is not on the
            // CopyDerive allowlist, so no public path could build these.
            let (bounce_thread_key, unprivileged_bounce_key) = {
                // SAFETY: keytable_addr names the live carved boot KeyTable.
                let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
                let full = boot_table
                    .insert(
                        KeySlot(38),
                        KeyEntry::new::<Thread>(bounce_id, Rights::all(), 0),
                        BOOT_TABLE_GUARD,
                    )
                    .unwrap_or_else(|failure| {
                        panic!("Bounce Thread grant failed: {:?}", failure.error.code())
                    });
                let limited = boot_table
                    .insert(
                        KeySlot(39),
                        KeyEntry::new::<Thread>(bounce_id, Rights(Rights::READ), 0),
                        BOOT_TABLE_GUARD,
                    )
                    .unwrap_or_else(|failure| {
                        panic!(
                            "limited Bounce Thread grant failed: {:?}",
                            failure.error.code()
                        )
                    });
                (full, limited)
            };

            // Authority is explicit: a capability without `RETIRE` is
            // rejected before any teardown effect — Bounce stays parked.
            assert!(matches!(
                ThreadKey::from_key(unprivileged_bounce_key, DomainId(1)).retire(),
                Err(CapError::InsufficientRights)
            ));
            assert_eq!(
                nucleus.pending.state(record).ok(),
                Some(PendingState::Waiting)
            );

            // Self-retirement is rejected: the current Thread must survive
            // its own invocation (never-returns self-retirement is recorded
            // in the contract as wanted follow-up) — and again nothing was
            // torn down. The boot Thread's own capability (slot 40) is the
            // invoked key.
            let boot_thread_key = boot_key(50, 1);
            assert!(matches!(
                ThreadKey::from_key(boot_thread_key, DomainId(0)).retire(),
                Err(CapError::InvalidOperation)
            ));
            assert_eq!(
                nucleus.pending.state(record).ok(),
                Some(PendingState::Waiting)
            );

            assert_eq!(
                nucleus
                    .pools
                    .threads
                    .get_live(usize::from(bounce_id.index))
                    .expect("rejected retirement removed Bounce")
                    .context,
                ExecutionContext::Parked { saved, record }
            );
            assert_eq!(fixture_stack_state(), (boot_execution_sp, shared_trap_sp));

            // Retire Bounce through the real SVC path: the parked record is
            // cancelled and released, and no queued wakeup survives.
            ThreadKey::from_key(bounce_thread_key, DomainId(1))
                .retire()
                .unwrap_or_else(|error| panic!("Bounce Retire failed: {:?}", error.code()));
            assert!(nucleus.pending.is_empty());
            assert!(nucleus.scheduler.is_empty());
            // The released record's identity is stale.
            nucleus.pending.state(record).unwrap_err();

            // N2 no longer holds Bounce: a signal through the real SVC path
            // delivers to no dead waiter — it succeeds and the bits stay
            // pending — and the notification remains fully usable.
            NotificationKey::from_key(n2_key)
                .signal(0b100)
                .unwrap_or_else(|error| panic!("post-retire Signal failed: {:?}", error.code()));
            let bits = NotificationKey::from_key(n2_key)
                .wait(NotificationKey::WAIT_INFINITE)
                .unwrap_or_else(|error| panic!("post-retire Wait failed: {:?}", error.code()));
            assert_eq!(bits, 0b100);

            // The retired Thread's pool slot is reclaimed and its capability
            // is stale: the identity no longer resolves, and a further Retire
            // through the same capability fails with a defined error.
            nucleus.pools.threads.validate(bounce_id).unwrap_err();
            // Thread retirement does not retire its AddressSpace or reset the
            // shared table's counters/backing; the binding remains live.
            nucleus
                .pools
                .arch
                .address_spaces
                .validate(bounce_as_id)
                .unwrap_or_else(|_| panic!("Thread retirement invalidated Bounce's AddressSpace"));
            assert_eq!(
                nucleus
                    .pools
                    .arch
                    .address_spaces
                    .get_live(usize::from(bounce_as_id.index))
                    .expect("Thread retirement removed Bounce's AddressSpace")
                    .keytable()
                    .address(),
                bounce_table_binding.address()
            );
            assert!(
                nucleus
                    .pools
                    .threads
                    .get_live(usize::from(bounce_id.index))
                    .is_none()
            );
            assert!(matches!(
                ThreadKey::from_key(bounce_thread_key, DomainId(1)).retire(),
                Err(CapError::InvalidOperation)
            ));
        }

        // ─────────────────────────────────────────────────────────────────
        // AddressSpace.Retire end-to-end: the address-space teardown —
        // whole-ASID invalidation, ASID release to the originating pool,
        // root/ASID fields cleared, pool slot reclaimed — through the real
        // SVC path under `RETIRE` authority.
        // ─────────────────────────────────────────────────────────────────
        {
            // A fixture AddressSpace with its own root and ASID.
            // Provision a distinct initialized table too; its Retype carve
            // stays accounted and is never reclaimed/reinitialized, even
            // after the AddressSpace pool slot is retired.
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
                let entry = unsafe { &*(keytable_addr as *const KeyTable) }
                    .lookup(fixture_table_key, BOOT_TABLE_GUARD)
                    .unwrap_or_else(|_| panic!("retirement KeyTable capability missing"));
                let address = entry
                    .keytable_address()
                    .unwrap_or_else(|_| panic!("retirement table capability has the wrong kind"));
                // SAFETY: the kernel-issued address names the full initialized
                // private Retype carve retained without relocation, reclaim,
                // or reinitialization while any binding is live.
                let binding = unsafe { (&*(address as *const KeyTable)).binding() };
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
            let fixture_as_id = nucleus
                .pools
                .arch
                .address_spaces
                .allocate(ArchObjectsImpl::new_address_space(fixture_table_binding))
                .expect("no fixture AddressSpace slot")
                .0;
            assert_eq!(fixture_as_id.index, 2);
            let fixture_as_key = {
                // SAFETY: keytable_addr names the live carved boot KeyTable.
                let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
                boot_table
                    .insert(
                        KeySlot(54),
                        KeyEntry::new::<<ArchObjectsImpl as ArchObjects>::AddressSpace>(
                            fixture_as_id,
                            Rights::all(),
                            0,
                        ),
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
            let fixture_bound_asid =
                boot_asid_pool
                    .assign(fixture_as_key)
                    .unwrap_or_else(|error| {
                        panic!("fixture ASIDPool.Assign failed: {:?}", error.code())
                    });
            assert_eq!(
                fixture_bound_asid, 3,
                "the fixture binds the next free ASID"
            );
            assert!(matches!(
                fixture_as.retire(),
                Err(CapError::InvalidOperation)
            ));

            // Unmap the (empty) root, then retire: the ASID is released back
            // to the boot pool and the pool slot is reclaimed.
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
                    .pools
                    .arch
                    .address_spaces
                    .get_live(2)
                    .expect("fixture AS missing before retirement")
                    .translation_root,
                None
            );
            fixture_as.retire().unwrap_or_else(|error| {
                panic!("fixture AddressSpace.Retire failed: {:?}", error.code())
            });
            nucleus
                .pools
                .arch
                .address_spaces
                .validate(fixture_as_id)
                .unwrap_err();
            // The retired AddressSpace's capability is stale: a further Retire
            // fails with a defined error.
            assert!(matches!(
                fixture_as.retire(),
                Err(CapError::InvalidOperation)
            ));

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
                .unwrap_or_else(|error| {
                    panic!("rebind KeyTable Retype failed: {:?}", error.code())
                });
            let rebind_table_binding = {
                // SAFETY: the live boot table is borrowed only for this lookup.
                let entry = unsafe { &*(keytable_addr as *const KeyTable) }
                    .lookup(rebind_table_key, BOOT_TABLE_GUARD)
                    .unwrap_or_else(|_| panic!("rebind KeyTable capability missing"));
                let address = entry
                    .keytable_address()
                    .unwrap_or_else(|_| panic!("rebind table capability has the wrong kind"));
                // SAFETY: the kernel-issued address names the full initialized
                // private Retype carve retained without relocation, reclaim,
                // or reinitialization while any binding is live.
                let binding = unsafe { (&*(address as *const KeyTable)).binding() };
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
            let rebind_as_id = nucleus
                .pools
                .arch
                .address_spaces
                .allocate(ArchObjectsImpl::new_address_space(rebind_table_binding))
                .expect("no rebind AddressSpace slot")
                .0;
            assert_eq!(rebind_as_id.index, 3);
            let rebind_as_key = {
                // SAFETY: keytable_addr names the live carved boot KeyTable.
                let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
                boot_table
                    .insert(
                        KeySlot(55),
                        KeyEntry::new::<<ArchObjectsImpl as ArchObjects>::AddressSpace>(
                            rebind_as_id,
                            Rights::all(),
                            0,
                        ),
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
                .unwrap_or_else(|error| {
                    panic!("rebind root PageTable.Map failed: {:?}", error.code())
                });
            let rebound_asid = boot_asid_pool
                .assign(rebind_as_key)
                .unwrap_or_else(|error| {
                    panic!("rebind ASIDPool.Assign failed: {:?}", error.code())
                });
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
                0
            ),
            Err(CapError::MissingIntermediate { vaddr: 0x2000_0000 })
        ));
        // A misaligned virtual address is rejected with the frame's size.
        assert!(matches!(
            frame.map(
                boot_as_key,
                0x1000_0001,
                Rights(Rights::READ | Rights::WRITE),
                0
            ),
            Err(CapError::InvalidSize(12))
        ));
        // Unsupported attributes are rejected.
        assert!(matches!(
            frame.map(
                boot_as_key,
                0x1000_0000,
                Rights(Rights::READ | Rights::WRITE),
                1
            ),
            Err(CapError::InvalidOperation)
        ));

        // Real mapping: the walk installs the page descriptor at level 3.
        frame
            .map(
                boot_as_key,
                0x1000_0000,
                Rights(Rights::READ | Rights::WRITE),
                0,
            )
            .unwrap_or_else(|error| panic!("Frame.Map failed: {:?}", error.code()));
        // A second mapping of the same capability is rejected.
        assert!(matches!(
            frame.map(
                boot_as_key,
                0x1000_0000,
                Rights(Rights::READ | Rights::WRITE),
                0
            ),
            Err(CapError::AlreadyMapped)
        ));

        // Verify the descriptor chain by hand through the direct map.
        let root_paddr = nucleus
            .pools
            .arch
            .address_spaces
            .get_live(0)
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
            let boot_table = unsafe { &*(keytable_addr as *const KeyTable) };
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
                0
            ),
            Err(CapError::PhysicalAlias { paddr }) if paddr == frame_paddr
        ));

        // A 2 MiB frame installs a block descriptor at level 2 in the same
        // chain (a distinct slot, read-only).
        let large_frame = FrameKey::from_key(large_frame_key);
        large_frame
            .map(boot_as_key, 0x1020_0000, Rights(Rights::READ), 0)
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
                0,
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
        // Repeat in the reverse direction while the second translation is
        // warm: neither physical marker changes, only the same VA's backing.
        frame
            .map(
                boot_as_key,
                0x1000_0000,
                Rights(Rights::READ | Rights::WRITE),
                0,
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
        frame.unmap().unwrap_or_else(|error| {
            panic!("original frame final Unmap failed: {:?}", error.code())
        });
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
                0,
            )
            .unwrap_or_else(|error| {
                panic!("post-unmap derived Frame.Map failed: {:?}", error.code())
            });
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
                .pools
                .arch
                .address_spaces
                .get_live(0)
                .unwrap_or_else(|| panic!("boot AddressSpace missing"));
            assert_eq!(address_space.translation_root, Some(root_paddr));
            assert_eq!(address_space.asid, Some(bound_asid));
        }

        // Page-table pool accounting: a batch that cannot fit releases its
        // partially allocated metadata slots, and a later smaller batch
        // succeeds. Capacity is now exactly the retained source/Bounce tables,
        // both disposable retirement roots and their teardown chain, this
        // uninstalled L3 metadata, and
        // twelve spare entries. Unmap does not release page-table pool entries.
        assert_eq!(
            nucleus.pools.arch.page_tables.len(),
            page_table_capacity - 12
        );
        assert!(matches!(
            untyped.retype(
                ObjectType::PAGE_TABLE,
                12,
                0,
                13,
                &self_table,
                KeySlot(24).0,
                Rights::all(),
            ),
            Err(CapError::PoolExhausted)
        ));
        let refill = untyped
            .retype(
                ObjectType::PAGE_TABLE,
                12,
                0,
                12,
                &self_table,
                KeySlot(24).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("refill PageTable Retype failed: {:?}", error.code()));
        assert_eq!(refill.slot(), boot_slot(24));
        assert_eq!(nucleus.pools.arch.page_tables.len(), page_table_capacity);
        assert_source_selected(nucleus, boot_as_id, source_root, bound_asid);
        translation::verify_retained(&retained_init, source_root, bounce_root);
        assert!(matches!(
            untyped.retype(
                ObjectType::PAGE_TABLE,
                12,
                0,
                1,
                &self_table,
                KeySlot(36).0,
                Rights::all(),
            ),
            Err(CapError::PoolExhausted)
        ));
    }

    let (_, privilege_level) = libexception::current_privilege_level();
    liblog::info!("Current privilege level: {privilege_level}");

    liblog::info!("Exception handling state:");
    libexception::asynchronous::print_state();

    print_my_sp();

    cfg_if::cfg_if! {
        if #[cfg(feature = "qemu")] {
            libqemu::semihosting::exit_success()
        } else {
            endless_sleep()
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// Bounce fixture domain (completion foundation, 2026-09-16)
// ─────────────────────────────────────────────────────────────────────

/// The bits Bounce delivers to the blocked boot domain.
#[cfg(feature = "debug_kernel")]
const BOUNCE_MAGIC_BITS: u64 = 0b1010_1010;

/// The Bounce fixture domain's entry (N4-A, 2026-09-16): a second EL1
/// execution context that unlocks the first blocked domain for testing.
///
/// Bounce first signals the notification the boot domain is blocked on
/// (waking it). It then serves one `EventCount` advance per N2 trigger:
/// trigger bit 0 requests a plain +3 advance, trigger bit 1 an overflowing
/// one (which completes waiters with the shared `CounterOverflow` error,
/// 2026-09-18). After two rounds it parks forever on N2 — nothing ever
/// signals it again, so the scheduler resumes the boot domain.
/// Bootstrap-era fixture mechanism, not the Phase 7 Activate contract: no
/// budget, no EL0 entry, no legal-transition enforcement beyond what this
/// path exercises.
/// Implementation status: Bounce's Thread now names its own `AddressSpace` and
/// its provisioned table, but still runs through the kernel high map without
/// an actual translation-context switch. This is not a PPC migration trial.
/// Implementation status update: the linked low image now runs under Bounce's
/// independent ASID-2 root, selected by the parent scheduling path. The trusted
/// high execution stack is retained; this remains a two-Thread test, not PPC.
/// Implementation status: execution uses `EL1t`/`SP_EL0`, with the same shared
/// high `SP_EL1` trap stack as the boot Thread and Thread-resident saved state.
#[cfg(feature = "debug_kernel")]
#[unsafe(no_mangle)]
extern "C" fn bounce_entry() -> ! {
    translation::observe_bounce();
    let initial_stack_state = fixture_stack_state();
    assert_eq!(
        initial_stack_state.1,
        FIXTURE_TRAP_SP.load(Ordering::Acquire)
    );
    let n1 = NotificationKey::from_key(RawKey::from_wire(BOUNCE_N1_KEY.load(Ordering::Acquire)));
    let n2_key = RawKey::from_wire(BOUNCE_N2_KEY.load(Ordering::Acquire));
    let ec = EventCountKey::from_key(RawKey::from_wire(BOUNCE_EC_KEY.load(Ordering::Acquire)));
    n1.signal(BOUNCE_MAGIC_BITS)
        .unwrap_or_else(|error| panic!("Bounce: Notification.Signal failed: {:?}", error.code()));
    // Serve one EventCount advance per N2 trigger, then park forever.
    for round in 0..2_u64 {
        let trigger = translation::notification_wait(n2_key).unwrap_or_else(|error| {
            panic!("Bounce: round {round} wait failed: {:?}", error.code())
        });
        assert_eq!(fixture_stack_state(), initial_stack_state);
        translation::observe_bounce();
        let result = match trigger {
            // A plain +3 advance that satisfies the boot domain's target.
            0b1 => ec.advance(3),
            // The overflowing advance: it completes waiters with the shared
            // error and leaves the counter unchanged (selected 2026-09-18).
            0b10 => ec.advance(u64::MAX),
            other => panic!("Bounce: unexpected trigger bits {other:#x}"),
        };
        if trigger == 0b10 {
            assert!(
                matches!(result, Err(CapError::CounterOverflow)),
                "Bounce: round {round} advance should overflow"
            );
        } else {
            result.unwrap_or_else(|error| {
                panic!("Bounce: round {round} advance failed: {:?}", error.code())
            });
        }
    }
    // Park forever: this wait blocks, so the scheduler resumes the boot
    // domain. Reaching either arm below is a fixture failure.
    match translation::notification_wait(n2_key) {
        Ok(bits) => panic!("Bounce: unexpected wakeup with bits {bits:#x}"),
        Err(error) => panic!("Bounce: Notification.Wait failed: {:?}", error.code()),
    }
}
