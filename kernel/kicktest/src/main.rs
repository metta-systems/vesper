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
    cfg_if::cfg_if,
    core::{panic::PanicInfo, slice},
    kickstart::{
        bootstrap::{
            BOOT_TABLE_GUARD, BOOT_TABLE_SIZE_BITS, BootState, PoolCapacities, bootstrap_nucleus,
        },
        kickstart_init_el2, print_my_sp,
    },
    libaddress::{PhysAddr, VirtAddr},
    libboot as boot,
    libcpu::endless_sleep,
    libobject::{
        KeySlot, ObjectType, Rights, address_space::AddressSpaceKey, domain::DomainId,
        thread::ThreadKey,
    },
    libqemu::semihosting as semi,
    nucleus::{
        api::key_entry::KeyEntry,
        objects::{
            ArchObjects, ArchObjectsImpl, ExecutionContext, KeyTable, Nucleus, Thread,
            access::{ObjectId, PoolTag},
            completion::PendingState,
        },
    },
};

#[cfg(feature = "debug_kernel")]
use {
    aarch64_cpu::registers::{Readable, TTBR0_EL1, Writeable},
    libobject::{
        ASIDPoolKey, CapError, DebugConsoleKey, EventCountKey, FrameKey, InvalidKeyReason,
        KeyTableKey, NotificationKey, PageTableKey, RawKey, UntypedKey,
    },
};

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
#[cfg(feature = "debug_kernel")]
const TEST_TABLE_GUARD: u32 = 0xFEE_D42;

/// Compose a boot-table key from a bare slot index and incarnation: the boot
/// guard packed above the index.
#[cfg(feature = "debug_kernel")]
fn boot_key(slot: u32, incarnation: u32) -> RawKey {
    RawKey::from_parts(BOOT_TABLE_GUARD, BOOT_TABLE_SIZE_BITS, slot, incarnation)
}

/// The boot-table slot half for a bare index (the guard packed above it).
#[cfg(feature = "debug_kernel")]
fn boot_slot(index: u32) -> KeySlot {
    KeySlot((BOOT_TABLE_GUARD << u32::from(BOOT_TABLE_SIZE_BITS)) | index)
}

/// The slot half of a key in one of the boot test's runtime-carved tables.
#[cfg(feature = "debug_kernel")]
fn test_slot(index: u32) -> KeySlot {
    KeySlot((TEST_TABLE_GUARD << u32::from(BOOT_TABLE_SIZE_BITS)) | index)
}

// DTB should be available to this code through BOOT_INFO records.
pub fn kicktest_run() -> ! {
    semi::println!("kicktest_run: enabled MMU and dropped to EL1");
    print_my_sp();

    // ─────────────────────────────────────────────────────────────────────
    // Build the initial kernel state in carved memory (inert nucleus), with
    // the e2e suite's fixture extents: the boot Thread + the Bounce fixture
    // Thread, the boot + two fixture AddressSpaces, the Notification and
    // EventCount pools the suite Retypes from, and the mapping-chain
    // page-table pool (16 slots plus the two fixture roots of the
    // AddressSpace.Retire test).
    // ─────────────────────────────────────────────────────────────────────
    let boot = bootstrap_nucleus(&PoolCapacities {
        threads: 2,
        address_spaces: 3,
        notifications: 4,
        event_counts: 4,
        page_tables: 18,
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
                KeySlot(5).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("boot Retype failed: {:?}", error.code()));
        assert_eq!(new_table_key.slot(), boot_slot(5));
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
                KeySlot(6).0,
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
                KeySlot(5).0,
                Rights::all(),
            ),
            Err(CapError::SlotOccupied(KeySlot(5)))
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
                KeySlot(7).0,
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
                KeySlot(7).0,
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
                KeySlot(6).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("second boot Retype failed: {:?}", error.code()));
        assert_eq!(second_table_key.slot(), boot_slot(6));

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
                KeySlot(9).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("frame Retype failed: {:?}", error.code()));
        assert_eq!(frame_key.slot(), boot_slot(9));

        // Non-granular frame sizes are rejected with the architecture's own
        // error, leaving the table unchanged (slot 10 stays free).
        assert!(matches!(
            untyped.retype(
                ObjectType::FRAME,
                13,
                0,
                1,
                &self_table,
                KeySlot(10).0,
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
                KeySlot(10).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("second frame Retype failed: {:?}", error.code()));
        assert_eq!(second_frame_key.slot(), boot_slot(10));
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
                KeySlot(11).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("large frame Retype failed: {:?}", error.code()));
        assert_eq!(large_frame_key.slot(), boot_slot(11));
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
        // Untyped split (2026-09-23): Retype an Untyped into smaller
        // Untypeds through the real SVC path, then carve from a child.
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
                    KeySlot(7),
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
                KeySlot(8).0,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("misaligned-base Retype failed: {:?}", error.code()));
        assert_eq!(carved_key.slot(), boot_slot(8));

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
        // Mapping vertical slice (2026-09-15): carved page tables, real
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
        // ASID binding (2026-09-15) through the real SVC path: before a
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
        let bounce_stack_top =
            PhysAddr::new(bounce_stack_paddr).user_to_kernel().as_u64() + 8 * 4096;

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
            bounce_table
                .insert(
                    KeySlot(1),
                    KeyEntry::from_id(ObjectType::NOTIFICATION, n1_id, Rights::all(), 0),
                    TEST_TABLE_GUARD,
                )
                .unwrap_or_else(|failure| {
                    panic!("Bounce N1 grant failed: {:?}", failure.error.code())
                });
            bounce_table
                .insert(
                    KeySlot(2),
                    KeyEntry::from_id(ObjectType::NOTIFICATION, n2_id, Rights::all(), 0),
                    TEST_TABLE_GUARD,
                )
                .unwrap_or_else(|failure| {
                    panic!("Bounce N2 grant failed: {:?}", failure.error.code())
                });
            // The EventCount sits at slot 4: slot 3 is the well-known
            // self-table slot and must hold the table capability.
            bounce_table
                .insert(
                    KeySlot(4),
                    KeyEntry::from_id(ObjectType::EVENT_COUNT, ec_id, Rights::all(), 0),
                    TEST_TABLE_GUARD,
                )
                .unwrap_or_else(|failure| {
                    panic!("Bounce EC grant failed: {:?}", failure.error.code())
                });
        }

        // Allocate Bounce's Thread and queue it runnable: it starts only
        // when the boot thread blocks. Bounce executes in the boot
        // AddressSpace (fixture threads need no private translation context).
        let (bounce_id, _bounce_thread) = nucleus
            .pools
            .threads
            .allocate(Thread {
                keytable_addr: bounce_table_addr,
                address_space: boot_as_id,
                context: ExecutionContext::NotStarted {
                    pc: bounce_entry as *const () as u64,
                    stack_top: bounce_stack_top,
                },
            })
            .unwrap_or_else(|| panic!("no Bounce Thread slot"));
        assert_eq!(bounce_id.index, 1);
        assert!(nucleus.scheduler.push(bounce_id.index));

        // The boot thread blocks on N1: this SVC does not return — the
        // kernel parks it, starts Bounce (which signals N1 and parks on N2),
        // then resumes the boot thread with the delivered bitmap.
        let received = NotificationKey::from_key(n1_key)
            .wait(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|error| {
                panic!("blocking Notification.Wait failed: {:?}", error.code())
            });
        assert_eq!(received, BOUNCE_MAGIC_BITS);
        // Bounce is parked on N2; the boot thread resumed with the bits.
        {
            let bounce = nucleus
                .pools
                .threads
                .get_live(usize::from(bounce_id.index))
                .unwrap_or_else(|| panic!("Bounce Domain missing"));
            assert!(matches!(bounce.context, ExecutionContext::Parked { .. }));
        }

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

        // Blocking Await end-to-end: request Bounce's +3 advance by
        // signaling N2 (bit 0), then block on target 10. Bounce advances the
        // counter, this domain's record completes with the new value, and
        // Bounce parks again.
        NotificationKey::from_key(n2_key)
            .signal(0b1)
            .unwrap_or_else(|error| panic!("N2 trigger signal failed: {:?}", error.code()));
        assert_eq!(
            event_count
                .await_ge(10, EventCountKey::WAIT_INFINITE)
                .unwrap_or_else(|error| {
                    panic!("blocking EventCount.Await failed: {:?}", error.code())
                }),
            10
        );
        assert_eq!(
            event_count.read().unwrap_or_else(|error| panic!(
                "fourth EventCount.Read failed: {:?}",
                error.code()
            )),
            10
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
            event_count.await_ge(u64::MAX - 2, EventCountKey::WAIT_INFINITE),
            Err(CapError::CounterOverflow)
        ));
        assert_eq!(
            event_count
                .read()
                .unwrap_or_else(|error| panic!("fifth EventCount.Read failed: {:?}", error.code())),
            10
        );
        // Bounce is parked on N2 for good; the boot domain resumed with the
        // error completion.
        {
            let bounce = nucleus
                .pools
                .threads
                .get_live(usize::from(bounce_id.index))
                .unwrap_or_else(|| panic!("Bounce Domain missing"));
            assert!(matches!(bounce.context, ExecutionContext::Parked { .. }));
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
            let ExecutionContext::Parked { record, .. } = nucleus
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
            let fixture_as_id = nucleus
                .pools
                .arch
                .address_spaces
                .allocate(ArchObjectsImpl::new_address_space())
                .expect("no fixture AddressSpace slot")
                .0;
            assert_eq!(fixture_as_id.index, 1);
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
            let fixture_bound_asid =
                boot_asid_pool
                    .assign(fixture_as_key)
                    .unwrap_or_else(|error| {
                        panic!("fixture ASIDPool.Assign failed: {:?}", error.code())
                    });
            assert_eq!(
                fixture_bound_asid, 2,
                "the fixture binds the next free ASID"
            );
            assert!(matches!(
                fixture_as.retire(),
                Err(CapError::InvalidOperation)
            ));

            // Unmap the (empty) root, then retire: the ASID is released back
            // to the boot pool and the pool slot is reclaimed.
            PageTableKey::from_key(fixture_root_pt_key)
                .unmap()
                .unwrap_or_else(|error| {
                    panic!("fixture root PageTable.Unmap failed: {:?}", error.code())
                });
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
            let rebind_as_id = nucleus
                .pools
                .arch
                .address_spaces
                .allocate(ArchObjectsImpl::new_address_space())
                .expect("no rebind AddressSpace slot")
                .0;
            assert_eq!(rebind_as_id.index, 2);
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
                rebound_asid, 2,
                "the retired AddressSpace's ASID was released back to the pool"
            );
        }

        // Build the intermediate chain: L1 under the root, L2 under L1,
        // L3 under L2, all selecting the slots for vaddr 0x1000_0000.
        let l1_pt = PageTableKey::from_key(l1_pt_key);
        l1_pt
            .map(root_pt_key, 0x1000_0000)
            .unwrap_or_else(|error| panic!("L1 PageTable.Map failed: {:?}", error.code()));
        let l2_pt = PageTableKey::from_key(l2_pt_key);
        l2_pt
            .map(l1_pt_key, 0x1000_0000)
            .unwrap_or_else(|error| panic!("L2 PageTable.Map failed: {:?}", error.code()));
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
                KeySlot(12).0,
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
            *PhysAddr::new(frame_paddr)
                .user_to_kernel()
                .as_mut_ptr::<u64>() = MAGIC_ORIGINAL;
            *PhysAddr::new(second_frame_paddr)
                .user_to_kernel()
                .as_mut_ptr::<u64>() = MAGIC_SECOND;
        }

        // Save the boot identity-map context so the test can restore it after
        // the observation (deactivation is not a capability operation yet).
        let boot_ttbr0 = TTBR0_EL1.get();

        // The bootstrap caller (this test) executes in the low half through
        // the boot identity map: its stack sits below the image base at
        // 0x80000 and the kickstart image extends beyond the 2 MiB boundary.
        // A real Domain's address space contains its own image and stack by
        // construction, and the bootstrap caller is no exception — map its
        // low-half working set into the boot Domain's context as two 2 MiB
        // blocks (fabricated bootstrap-test fixtures naming the in-use
        // physical range, like the misaligned-region fixture above), so
        // execution can continue under the activated tables.
        let low_block_keys: [RawKey; 2] = [
            {
                // SAFETY: keytable_addr names the live boot KeyTable.
                let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
                boot_table
                    .insert(
                        KeySlot(60),
                        KeyEntry::new_frame(0, 21, false, Rights::all()),
                        BOOT_TABLE_GUARD,
                    )
                    .unwrap_or_else(|_| panic!("low-half block A install failed"))
            },
            {
                // SAFETY: see above.
                let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
                boot_table
                    .insert(
                        KeySlot(61),
                        KeyEntry::new_frame(0x20_0000, 21, false, Rights::all()),
                        BOOT_TABLE_GUARD,
                    )
                    .unwrap_or_else(|_| panic!("low-half block B install failed"))
            },
        ];
        // The blocks are requested with the EXECUTE right (selected
        // 2026-09-15): the bootstrap caller must keep executing inside its
        // Domain's context, so its image is executable there.
        for (block, vaddr) in low_block_keys.iter().zip([0_u64, 0x20_0000_u64]) {
            FrameKey::from_key(*block)
                .map(
                    boot_as_key,
                    vaddr,
                    Rights(Rights::READ | Rights::WRITE | Rights::EXECUTE),
                    0,
                )
                .unwrap_or_else(|error| {
                    panic!(
                        "low-half Frame.Map at {vaddr:#x} failed: {:?}",
                        error.code()
                    )
                });
        }
        {
            const ADDR_MASK: u64 = 0x0000_FFFF_FFFF_F000;
            // SAFETY: see above.
            let read_entry = |paddr: u64, slot: usize| unsafe {
                *(PhysAddr::new(paddr).user_to_kernel().as_ptr::<u64>()).add(slot)
            };
            let l0e = read_entry(root_paddr, 0);
            let l1e = read_entry(l0e & ADDR_MASK, 0);
            let l2_paddr = l1e & ADDR_MASK;
            for (slot, base) in [(0, 0_u64), (1, 0x20_0000)] {
                let block = read_entry(l2_paddr, slot);
                assert_eq!(
                    block & ADDR_MASK,
                    base,
                    "the low-half block must be identity"
                );
                assert!(block & 0b1 != 0, "the low-half block must be valid");
                assert!(
                    block & (1 << 53) == 0 && block & (1 << 54) == 0,
                    "an EXECUTE-requested mapping must have UXN|PXN clear"
                );
                assert_eq!(
                    block & (0b11 << 6),
                    0,
                    "a writable EXECUTE mapping must be kernel-privilege (AP=00)"
                );
            }
        }

        // Activate through the real SVC path: the tables become hardware-live.
        boot_as
            .activate()
            .unwrap_or_else(|error| panic!("AddressSpace.Activate failed: {:?}", error.code()));

        // A load from the mapped virtual address now walks the AddressSpace's
        // tables: the marker written through the direct map must come back
        // through the level-3 page descriptor.
        // SAFETY: the activated translation context maps this virtual address
        // to the original frame; the boot test runs at EL1 with PAN inactive.
        let observed = unsafe { *(0x1000_0000_u64 as *const u64) };
        assert_eq!(
            observed, MAGIC_ORIGINAL,
            "the activated context must serve the real mapping"
        );

        // Unmapping a non-empty table is rejected: L3 still holds the page.
        assert!(matches!(l3_pt.unmap(), Err(CapError::InvalidOperation)));
        // L2 still holds the L3 table descriptor.
        assert!(matches!(l2_pt.unmap(), Err(CapError::InvalidOperation)));

        // Frame.Unmap clears the descriptor and the record, and withdraws the
        // cached translation under the bound ASID (tlbi vae1is + dsb/isb) —
        // executing the real maintenance sequence here proves it is safe on
        // the live kernel context.
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
        // SAFETY: the activated context now maps this virtual address to the
        // second frame; PAN is inactive at EL1.
        let observed = unsafe { *(0x1000_0000_u64 as *const u64) };
        assert_eq!(
            observed, MAGIC_SECOND,
            "the unmap's TLB invalidation must withdraw the stale translation"
        );
        FrameKey::from_key(second_frame_key)
            .unmap()
            .unwrap_or_else(|error| panic!("second frame Frame.Unmap failed: {:?}", error.code()));

        // Restore the boot identity-map context; the remaining assertions walk
        // tables through the direct map and need no live Domain context.
        // SAFETY: the saved value is the boot TTBR0_EL1 installed by
        // `enable_mmu_and_drop_to_el1`.
        unsafe {
            TTBR0_EL1.set(boot_ttbr0);
            core::arch::asm!("isb", options(nostack));
        }

        // Withdraw the caller's low-half blocks before the table teardown
        // below: the L2 teardown requires an empty table.
        for block in low_block_keys {
            FrameKey::from_key(block)
                .unmap()
                .unwrap_or_else(|error| panic!("low-half Frame.Unmap failed: {:?}", error.code()));
        }

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
        l2_pt
            .unmap()
            .unwrap_or_else(|error| panic!("L2 PageTable.Unmap failed: {:?}", error.code()));
        l1_pt
            .unmap()
            .unwrap_or_else(|error| panic!("L1 PageTable.Unmap failed: {:?}", error.code()));
        root_pt
            .unmap()
            .unwrap_or_else(|error| panic!("root PageTable.Unmap failed: {:?}", error.code()));
        // The root unmap withdrew the whole context: every cached translation
        // under the bound ASID was invalidated (tlbi aside1is + dsb/isb).
        assert!(matches!(root_pt.unmap(), Err(CapError::NotMapped)));
        {
            let address_space = nucleus
                .pools
                .arch
                .address_spaces
                .get_live(0)
                .unwrap_or_else(|| panic!("boot AddressSpace missing"));
            assert_eq!(address_space.translation_root, None);
        }

        // Page-table pool accounting: a batch that cannot fit releases its
        // partially allocated metadata slots, and a later smaller batch
        // succeeds (capacity 16, four tables carved so far).
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
#[cfg(feature = "debug_kernel")]
#[unsafe(no_mangle)]
extern "C" fn bounce_entry() -> ! {
    let n1 = NotificationKey::from_key(RawKey::from_parts(
        TEST_TABLE_GUARD,
        BOOT_TABLE_SIZE_BITS,
        1,
        1,
    ));
    let n2 = NotificationKey::from_key(RawKey::from_parts(
        TEST_TABLE_GUARD,
        BOOT_TABLE_SIZE_BITS,
        2,
        1,
    ));
    let ec = EventCountKey::from_key(RawKey::from_parts(
        TEST_TABLE_GUARD,
        BOOT_TABLE_SIZE_BITS,
        4,
        1,
    ));
    n1.signal(BOUNCE_MAGIC_BITS)
        .unwrap_or_else(|error| panic!("Bounce: Notification.Signal failed: {:?}", error.code()));
    // Serve one EventCount advance per N2 trigger, then park forever.
    for round in 0..2_u64 {
        let trigger = n2
            .wait(NotificationKey::WAIT_INFINITE)
            .unwrap_or_else(|error| {
                panic!("Bounce: round {round} wait failed: {:?}", error.code())
            });
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
    match n2.wait(NotificationKey::WAIT_INFINITE) {
        Ok(bits) => panic!("Bounce: unexpected wakeup with bits {bits:#x}"),
        Err(error) => panic!("Bounce: Notification.Wait failed: {:?}", error.code()),
    }
}
