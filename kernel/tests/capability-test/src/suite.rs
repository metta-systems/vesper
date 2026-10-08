//! The capability suite: what the boot Thread can do with the capabilities
//! Kickstart hands it — the boot table and its invariants, the issued debug
//! console key, KeyTable/Frame Retype with sanitization and carve placement,
//! Untyped splitting, and the current-relative Return sentinel.

use {
    core::{
        arch::asm,
        mem::{align_of, size_of},
        slice,
    },
    kickstart::bootstrap::{
        BOOT_TABLE_GUARD, BOOT_TABLE_SIZE_BITS, BootState, PoolCapacities, bootstrap_nucleus,
    },
    libaddress::{PhysAddr, VirtAddr},
    libkicktest::keys::{boot_key, boot_slot, table_slot},
    libobject::{
        CapError, DebugConsoleKey, InvalidKeyReason, KeySlot, KeyTableKey, ObjectType, RawKey,
        Rights, UntypedKey, thread::ThreadOp,
    },
    libqemu::semihosting as semi,
    nucleus::{
        api::key_entry::{KeyEntry, ThreadSelector},
        objects::{KeyTable, ObjectPool, Thread, arch_objects::AddressSpaceObject},
    },
};

/// The guard for the tables this suite carves at runtime: distinct from the
/// boot table's guard so cross-table key confusion is exercised. Fits the 24
/// guard bits of a 256-entry table's table-relative address.
const TEST_TABLE_GUARD: u32 = 0xFEE_D42;

/// The slot half of a key in one of the suite's runtime-carved tables.
fn test_slot(index: u32) -> KeySlot {
    table_slot(TEST_TABLE_GUARD, index)
}

pub fn run() {
    // Only the boot Thread and AddressSpace exist; Retype needs no other pool.
    let boot = bootstrap_nucleus(&PoolCapacities {
        threads: 1,
        address_spaces: 1,
        notifications: 0,
        event_counts: 0,
        page_tables: 0,
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

    assert_eq!(boot_as_id.index, 0);
    // Bootstrap sizes every pool exactly as requested.
    assert_eq!(nucleus.pools.threads.capacity(), 1);
    assert!(ObjectPool::<Thread>::carve_size(2) >= size_of::<Thread>() * 2);
    assert_eq!(nucleus.pools.arch.address_spaces.capacity(), 1);
    assert_eq!(nucleus.pools.arch.page_tables.capacity(), 0);
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
        let table_alignment = align_of::<KeyEntry>();
        let table_bytes = (KeyTable::HEADER_SIZE
            + boot_table.capacity() * (size_of::<KeyEntry>() + size_of::<u32>()))
        .next_multiple_of(table_alignment);
        assert_eq!(KeyTable::carve_size(BOOT_TABLE_SIZE_BITS), table_bytes);
        assert_eq!(keytable_addr % u64::try_from(table_alignment).unwrap(), 0);
        let boot_region = boot_table
            .lookup(boot_untyped_key, BOOT_TABLE_GUARD)
            .unwrap_or_else(|error| panic!("boot Untyped missing: {:?}", error.code()))
            .as_untyped()
            .unwrap_or_else(|error| panic!("boot Untyped payload: {:?}", error.code()));
        let table_paddr = VirtAddr::new(keytable_addr).kernel_to_user().as_u64();
        assert!(table_paddr >= boot_region.paddr);
        assert!(
            table_paddr - boot_region.paddr + u64::try_from(table_bytes).unwrap()
                <= u64::try_from(boot_region.watermark_bytes()).unwrap(),
            "the full type-derived boot KeyTable carve must be charged"
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
    // fails the reservation instead (2^20 type-sized entries plus counters);
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
    // Include worst-case alignment padding as well as the complete table;
    // this fixture must follow the actual entry stride, not a 16 KiB guess.
    let misaligned_region_bytes =
        (KeyTable::carve_size(8) + usize::try_from(align - 1).unwrap()).next_power_of_two();
    let misaligned_region_bits = u8::try_from(misaligned_region_bytes.trailing_zeros()).unwrap();
    let misaligned_untyped_key = {
        // SAFETY: see above.
        let boot_table = unsafe { &mut *(keytable_addr as *mut KeyTable) };
        boot_table
            .insert(
                KeySlot(8),
                KeyEntry::new_untyped(
                    misaligned_base,
                    misaligned_region_bits,
                    false,
                    Rights::all(),
                ),
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
        .unwrap_or_else(|error| panic!("misaligned-base CopyDerive failed: {:?}", error.code()));
    assert_eq!(derived_misaligned.slot(), test_slot(1));
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
    // Return on the sentinel at depth zero is an illegal-return fault,
    // which the interim policy turns into a kernel halt; the PPC round
    // trip below exercises Return success instead.
    for op in [
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

    assert_ordinary_calls_preserve_registers(self_table_key);

    semi::println!("Capability suite passed");
}

/// An ordinary invocation writes only `x0..x2`: every other register, SP and
/// NZCV survive both success and rejection (the blocking-resume case is
/// checked by `sync-test` through the same probe).
fn assert_ordinary_calls_preserve_registers(self_table_key: RawKey) {
    use {libkicktest::registers, libobject::KeyTableOp};

    const PROBE_SLOT: u32 = 100;

    // Success with a result word: CopyDerive the self-table capability.
    let copy_args = [
        self_table_key.to_wire(),
        self_table_key.to_wire(),
        u64::from(PROBE_SLOT),
        u64::from(Rights::DERIVE),
        0,
        0,
    ];
    let copied = registers::invoke(self_table_key, KeyTableOp::CopyDerive as u64, copy_args);
    copied.assert_preserved(self_table_key, copy_args);
    let (status, derived_word, second_word) = copied.result();
    assert_eq!((status, second_word), (0, 0));
    let derived_key = RawKey::from_wire(derived_word);
    assert_eq!(derived_key.slot(), boot_slot(PROBE_SLOT));

    // Success with zero result words: Delete it again.
    let delete_args = [derived_key.to_wire(), 0, 0, 0, 0, 0];
    let deleted = registers::invoke(self_table_key, KeyTableOp::Delete as u64, delete_args);
    deleted.assert_preserved(self_table_key, delete_args);
    assert_eq!(deleted.result(), (0, 0, 0));

    // Rejection with zero details: an unassigned KeyTable operation.
    let unassigned = registers::invoke(self_table_key, 3, [0; 6]);
    unassigned.assert_preserved(self_table_key, [0; 6]);
    assert_eq!(unassigned.result(), CapError::InvalidOperation.code());

    // Rejection with detail words: Delete through the now-stale key.
    let stale = registers::invoke(self_table_key, KeyTableOp::Delete as u64, delete_args);
    stale.assert_preserved(self_table_key, delete_args);
    let (status, detail1, _) = stale.result();
    assert_ne!(status, 0);
    assert_eq!(detail1, derived_key.to_wire());

    semi::println!("✅ ordinary invocations preserve x3..x30, SP and NZCV");
}
