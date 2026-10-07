//! Bootstrap provisioning through ordinary capability invocations.
//!
//! The boot Thread builds fixture translation contexts with `Untyped.Retype`,
//! `PageTable.Map`, `Frame.Map`, `CopyDerive` and `Move` on its own table.
//! The direct boot-table address is used only for bootstrap-origin grants and
//! read-only identity lookups, never held across an SVC.

use {
    crate::{
        keys::{SlotCursor, boot_key},
        paging::{ADDR_MASK, AP_MASK, LEAF_SPAN, PAGE, PXN, UXN, image_table_count, read_leaf},
    },
    kickstart::bootstrap::{BOOT_TABLE_GUARD, RetainedInitMemory},
    libaddress::PhysAddr,
    libobject::{
        ASIDPoolKey, CapError, FrameKey, KeySlot, KeyTableKey, ObjectType, PageTableKey, RawKey,
        Rights, UntypedKey,
    },
    nucleus::{
        api::key_entry::KeyEntry,
        objects::{KeyTable, access::ObjectId},
    },
};

/// The boot Thread's provisioning authority: the boot Untyped, its own table
/// capability and the retained init image.
pub struct Builder<'a> {
    pub untyped: &'a UntypedKey,
    pub self_table: &'a KeyTableKey,
    /// Kernel-window address of the boot table, for bootstrap-origin grants
    /// and read-only lookups only.
    pub boot_table_addr: u64,
    pub retained: &'a RetainedInitMemory,
}

/// One root that receives the retained image: its `AddressSpace` capability,
/// the L2 table covering the image, and the first boot slot for its image L3
/// tables.
pub struct ImageTarget {
    pub address_space: RawKey,
    pub l2: RawKey,
    pub first_table_slot: u32,
}

/// Where the mapped image Frame capabilities are archived: a carved table at
/// `slot` with `guard`, plus two scratch boot slots for the grant and its
/// derived copies.
pub struct ImageArchive {
    pub slot: u32,
    pub guard: u32,
    pub grant_scratch: u32,
    pub copy_scratch: u32,
}

/// What [`Builder::map_retained_image`] mapped.
pub struct ImageSummary {
    pub image_pages: u64,
    pub stack_pages: u64,
    pub archive_bits: u8,
    /// Archived capabilities (one per mapped page per root).
    pub archived: u64,
}

/// A high direct-map execution stack built from contiguous accounted Frames.
pub struct ExecutionStack {
    pub bottom: u64,
    pub top: u64,
}

impl Builder<'_> {
    /// Retype `count` 4 KiB page tables into consecutive boot slots.
    pub fn carve_tables(&self, first: u32, count: u32) -> RawKey {
        self.untyped
            .retype(
                ObjectType::PAGE_TABLE,
                12,
                0,
                count,
                self.self_table,
                first,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("fixture PageTable Retype failed: {:?}", error.code()))
    }

    /// Install `table` under `parent` (a page table or, for a root, an
    /// `AddressSpace`) for `vaddr`.
    pub fn map_table(table: RawKey, parent: RawKey, vaddr: u64) {
        PageTableKey::from_key(table)
            .map(parent, vaddr)
            .unwrap_or_else(|error| panic!("fixture PageTable.Map failed: {:?}", error.code()));
    }

    /// Retype `count` 4 KiB Frames into consecutive boot slots.
    pub fn retype_frames(&self, first: u32, count: u32) -> RawKey {
        self.untyped
            .retype(
                ObjectType::FRAME,
                12,
                0,
                count,
                self.self_table,
                first,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("fixture Frame Retype failed: {:?}", error.code()))
    }

    /// Physical base of `count` consecutive Frames starting at boot slot
    /// `first`, checking that they are contiguous.
    pub fn contiguous_frames(first: u32, incarnation: u32, count: u32) -> u64 {
        let base = Self::frame_paddr(boot_key(first, incarnation));
        for index in 0..count {
            assert_eq!(
                Self::frame_paddr(boot_key(first + index, incarnation)),
                base + u64::from(index) * PAGE,
                "fixture Frames must be contiguous"
            );
        }
        base
    }

    pub fn frame_paddr(frame: RawKey) -> u64 {
        let (paddr, size) = FrameKey::from_key(frame)
            .get_extent()
            .unwrap_or_else(|error| panic!("Frame.GetExtent failed: {:?}", error.code()));
        assert_eq!(size, PAGE);
        paddr
    }

    /// Map `count` Frames from boot slot `first` at consecutive pages from
    /// `vaddr` in `target`.
    pub fn map_frames(
        first: u32,
        incarnation: u32,
        count: u32,
        target: RawKey,
        vaddr: u64,
        rights: Rights,
    ) {
        for index in 0..count {
            FrameKey::from_key(boot_key(first + index, incarnation))
                .map(target, vaddr + u64::from(index) * PAGE, rights, 0)
                .unwrap_or_else(|error| panic!("fixture Frame.Map failed: {:?}", error.code()));
        }
    }

    /// A root/L1/L2 chain toward VA 0 for `address_space`, from three
    /// fresh boot slots. Returns the L2 key.
    pub fn root_chain(&self, address_space: RawKey, slots: &mut SlotCursor) -> RawKey {
        let first = slots.take(3);
        let root = self.carve_tables(first, 3);
        Self::map_table(root, address_space, 0);
        let l1 = boot_key(first + 1, root.incarnation());
        let l2 = boot_key(first + 2, root.incarnation());
        Self::map_table(l1, root, 0);
        Self::map_table(l2, l1, 0);
        l2
    }

    /// Bind the lowest free ASID from the boot pool to `address_space`,
    /// whose root must already be installed.
    pub fn assign_asid(address_space: RawKey) -> u16 {
        ASIDPoolKey::from_key(boot_key(KeySlot::BOOT_ASID_POOL.0, 1))
            .assign(address_space)
            .unwrap_or_else(|error| panic!("ASIDPool.Assign failed: {:?}", error.code()))
    }

    /// `pages` fresh RW Frames at `vaddr`, mapped in one `AddressSpace`
    /// only, under a new L3 table below `l2`. `vaddr` must start a 2 MiB
    /// span the root does not map yet. Returns the contiguous physical base.
    pub fn private_pages(
        &self,
        address_space: RawKey,
        l2: RawKey,
        vaddr: u64,
        pages: u32,
        slots: &mut SlotCursor,
    ) -> u64 {
        assert_eq!(vaddr % LEAF_SPAN, 0);
        Self::map_table(self.carve_tables(slots.take(1), 1), l2, vaddr);
        let first = slots.take(pages);
        let frames = self.retype_frames(first, pages);
        Self::map_frames(
            first,
            frames.incarnation(),
            pages,
            address_space,
            vaddr,
            Rights(Rights::READ | Rights::WRITE),
        );
        Self::contiguous_frames(first, frames.incarnation(), pages)
    }

    /// An execution stack of `pages` contiguous accounted Frames, used
    /// through the invariant high direct map, so it is valid in every root.
    pub fn execution_stack(&self, pages: u32, slots: &mut SlotCursor) -> ExecutionStack {
        let first = slots.take(pages);
        let frames = self.retype_frames(first, pages);
        let paddr = Self::contiguous_frames(first, frames.incarnation(), pages);
        let bottom = PhysAddr::new(paddr).user_to_kernel().as_u64();
        let top = bottom + u64::from(pages) * PAGE;
        assert_eq!(top & 15, 0);
        ExecutionStack { bottom, top }
    }

    /// The checked pool identity behind a boot-table key.
    pub fn object_id(&self, key: RawKey) -> ObjectId {
        // SAFETY: the boot table is the live carved boot KeyTable; this
        // read-only borrow ends before any capability invocation.
        let table = unsafe { &*(self.boot_table_addr as *const KeyTable) };
        table
            .lookup(key, BOOT_TABLE_GUARD)
            .and_then(KeyEntry::object_id)
            .unwrap_or_else(|error| panic!("identity lookup failed: {:?}", error.code()))
    }

    /// Kernel-window address of the carved table a boot-table capability
    /// names.
    pub fn table_address(&self, key: RawKey) -> u64 {
        // SAFETY: as in `object_id`.
        let table = unsafe { &*(self.boot_table_addr as *const KeyTable) };
        table
            .lookup(key, BOOT_TABLE_GUARD)
            .and_then(KeyEntry::keytable_address)
            .unwrap_or_else(|error| panic!("table capability lookup failed: {:?}", error.code()))
    }

    /// Map the retained init image (identity VA, RW+X) into every target
    /// root, and the retained low execution stack (RW) into the first
    /// target only.
    ///
    /// Every root maps the *same* physical image pages: each page is granted
    /// once from the retained backing, `CopyDerive`d for the other roots, and
    /// every mapped capability is then moved into a charged archive table,
    /// keeping its mapping record. Scratch slots are reused with their
    /// returned incarnations, never guessed keys. Move is not
    /// deprovisioning: no backing, capability or mapping is discarded.
    pub fn map_retained_image(
        &self,
        targets: &[ImageTarget],
        archive: &ImageArchive,
    ) -> ImageSummary {
        let count = image_table_count(self.retained);
        let table_count = u32::try_from(count).unwrap_or(u32::MAX);
        for target in targets {
            let tables = self.carve_tables(target.first_table_slot, table_count);
            for index in 0..table_count {
                Self::map_table(
                    boot_key(target.first_table_slot + index, tables.incarnation()),
                    target.l2,
                    u64::from(index) * LEAF_SPAN,
                );
            }
        }

        let (image_start, image_end) = self.retained.image();
        let (stack_start, stack_end) = self.retained.stack();
        let image_pages = (image_end - image_start) / PAGE;
        let stack_pages = (stack_end - stack_start) / PAGE;
        let roots = u64::try_from(targets.len()).unwrap_or(u64::MAX);
        let entries = 1 + roots * image_pages + stack_pages; // slot zero is reserved
        let bits = u8::try_from(entries.next_power_of_two().trailing_zeros()).unwrap_or(u8::MAX);
        let archive_key = self
            .untyped
            .retype(
                ObjectType::KEY_TABLE,
                bits,
                archive.guard,
                1,
                self.self_table,
                archive.slot,
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("image archive Retype failed: {:?}", error.code()));
        let archive_table = KeyTableKey::from_key(archive_key);
        let archive_addr = self.table_address(archive_key);
        let mut next_slot = 1;
        for (start, end, shared) in [
            (image_start, image_end, true),
            (stack_start, stack_end, false),
        ] {
            let rights =
                Rights(Rights::READ | Rights::WRITE | if shared { Rights::EXECUTE } else { 0 });
            for paddr in (start..end).step_by(usize::try_from(PAGE).unwrap_or(usize::MAX)) {
                let original = {
                    // SAFETY: the initialized private boot carve; no table
                    // reference survives the following capability calls.
                    let table = unsafe { &mut *(self.boot_table_addr as *mut KeyTable) };
                    self.retained.grant_page(
                        table,
                        KeySlot(archive.grant_scratch),
                        BOOT_TABLE_GUARD,
                        paddr,
                    )
                };
                let receivers = if shared { targets.len() } else { 1 };
                // Copies for the other roots are derived while the original
                // is still in its scratch slot; the original goes last.
                for target in targets[..receivers].iter().skip(1) {
                    let copy = self
                        .self_table
                        .copy_derive(
                            original,
                            self.self_table,
                            archive.copy_scratch,
                            Rights::all(),
                        )
                        .unwrap_or_else(|error| {
                            panic!("image CopyDerive failed: {:?}", error.code())
                        });
                    // Copy did not install a descriptor or inherit a mapping.
                    assert!(matches!(
                        FrameKey::from_key(copy).unmap(),
                        Err(CapError::NotMapped)
                    ));
                    next_slot = self.map_and_archive(
                        copy,
                        target,
                        paddr,
                        rights,
                        &archive_table,
                        archive,
                        archive_addr,
                        next_slot,
                    );
                }
                next_slot = self.map_and_archive(
                    original,
                    &targets[0],
                    paddr,
                    rights,
                    &archive_table,
                    archive,
                    archive_addr,
                    next_slot,
                );
            }
        }
        assert_eq!(u64::from(next_slot), entries);
        {
            // SAFETY: full initialized retained private archive carve; no SVC
            // or mutable table access occurs while this borrow is live.
            let table = unsafe { &*(archive_addr as *const KeyTable) };
            assert_eq!(table.capacity(), 1_usize << bits);
            assert_eq!(u64::try_from(table.len()).unwrap_or(u64::MAX), entries - 1);
        }
        ImageSummary {
            image_pages,
            stack_pages,
            archive_bits: bits,
            archived: entries - 1,
        }
    }

    /// Map one image/stack Frame capability into `target` at its identity VA
    /// and move it into the archive, checking the mapping record survives.
    #[allow(clippy::too_many_arguments)]
    fn map_and_archive(
        &self,
        frame: RawKey,
        target: &ImageTarget,
        paddr: u64,
        rights: Rights,
        archive_table: &KeyTableKey,
        archive: &ImageArchive,
        archive_addr: u64,
        next_slot: u32,
    ) -> u32 {
        let target_id = self.object_id(target.address_space);
        FrameKey::from_key(frame)
            .map(target.address_space, paddr, rights, 0)
            .unwrap_or_else(|error| {
                panic!(
                    "retained Frame.Map at {paddr:#x} failed: {:?}",
                    error.code()
                )
            });
        let archived = self
            .self_table
            .transfer(frame, archive_table, next_slot)
            .unwrap_or_else(|error| panic!("mapped image Move failed: {:?}", error.code()));
        // SAFETY: archive_addr came from the live capability to the full
        // private Retype carve. This borrow ends before the next SVC; Move
        // must preserve the mapping record.
        let table = unsafe { &*(archive_addr as *const KeyTable) };
        let archived_frame = table
            .lookup(archived, archive.guard)
            .and_then(KeyEntry::as_frame)
            .unwrap_or_else(|error| panic!("archived mapping missing: {:?}", error.code()));
        let mapping = archived_frame
            .mapping()
            .expect("Move lost the mapping record");
        assert_eq!(archived_frame.paddr, paddr);
        assert_eq!(mapping.vaddr, paddr);
        assert_eq!(mapping.address_space, target_id);
        next_slot + 1
    }
}

/// Verify the full page-granular image closure in every root, and the low
/// execution stack in `roots[0]`, before any of them runs.
pub fn verify_retained_image(retained: &RetainedInitMemory, roots: &[u64]) {
    let (image_start, image_end) = retained.image();
    let (stack_start, stack_end) = retained.stack();
    let step = usize::try_from(PAGE).unwrap_or(usize::MAX);
    for &root in roots {
        for paddr in (image_start..image_end).step_by(step) {
            let (level, leaf) = read_leaf(root, paddr);
            assert_eq!(level, 3);
            assert_eq!(leaf & ADDR_MASK, paddr);
            assert_ne!(leaf & (1 << 11), 0);
            assert_eq!(
                leaf & (PXN | UXN | AP_MASK),
                UXN,
                "trusted RW+X image must execute at EL1 only: AP=00, PXN clear, UXN set"
            );
        }
    }
    for paddr in (stack_start..stack_end).step_by(step) {
        let (level, leaf) = read_leaf(roots[0], paddr);
        assert_eq!(level, 3);
        assert_eq!(leaf & ADDR_MASK, paddr);
        assert_ne!(leaf & (1 << 11), 0);
        assert_eq!(leaf & (3 << 6), 1 << 6);
        assert_eq!(leaf & ((1 << 53) | (1 << 54)), (1 << 53) | (1 << 54));
    }
}
