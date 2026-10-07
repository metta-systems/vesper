//! Loading bundled EL0 components and giving their Threads stacks.
//!
//! A component's file-backed segments are page-aligned inside the bundling
//! image (the `.components` section), so they are mapped in place: each page
//! is granted once as a Frame over the retained image and mapped at the
//! component's link address with its segment's permissions. `.bss` is fresh,
//! zeroed Frames. Nothing else of the bundling image is mapped.

use {
    crate::{
        builder::Builder,
        component::Component,
        keys::SlotCursor,
        paging::{LEAF_SPAN, PAGE},
    },
    kickstart::bootstrap::BOOT_TABLE_GUARD,
    libimage::{ComponentImage, PAGE_BYTES, Permissions},
    libobject::{FrameKey, KeySlot, KeyTableKey, ObjectType, RawKey, Rights},
    nucleus::objects::KeyTable,
};

/// Frame rights for a segment: code is read-only and executable at EL0, data
/// is read/write and never executable, everything else read-only.
fn segment_rights(permissions: Permissions) -> Rights {
    match (permissions.writable, permissions.executable) {
        (false, true) => Rights(Rights::READ | Rights::EXECUTE),
        (true, false) => Rights(Rights::READ | Rights::WRITE),
        (false, false) => Rights(Rights::READ),
        (true, true) => panic!("a component segment may not be both writable and executable"),
    }
}

impl Builder<'_> {
    /// Map `image` into `component`'s `AddressSpace` at its link addresses.
    ///
    /// L3 tables are carved for every 2 MiB span the image touches. The
    /// mapped page capabilities are moved into a new archive table with
    /// `archive_guard`, which keeps their mapping records.
    pub fn load_component(
        &self,
        component: &Component,
        image: &ComponentImage,
        archive_guard: u32,
        slots: &mut SlotCursor,
    ) {
        let bss_range = image
            .bss
            .map(|bss| (bss.virt_addr, bss.virt_addr + bss.size as u64));
        let ranges = image
            .segments
            .iter()
            .map(|segment| {
                (
                    segment.meta.virt_addr,
                    segment.meta.virt_addr + segment.data.len() as u64,
                )
            })
            .chain(bss_range);
        let (low, high) = ranges.fold((u64::MAX, 0), |(low, high), (start, end)| {
            (low.min(start), high.max(end))
        });
        assert!(low < high, "component `{}` has nothing to load", image.name);
        for span in (low / LEAF_SPAN)..=((high - 1) / LEAF_SPAN) {
            Self::map_table(
                self.carve_tables(slots.take(1), 1),
                component.l2,
                span * LEAF_SPAN,
            );
        }

        let pages: usize = image
            .segments
            .iter()
            .map(|segment| segment.data.len() / PAGE_BYTES)
            .sum();
        let bits =
            u8::try_from((pages + 1).next_power_of_two().trailing_zeros()).unwrap_or(u8::MAX);
        let archive_key = self
            .untyped
            .retype(
                ObjectType::KEY_TABLE,
                bits.max(1),
                archive_guard,
                1,
                self.self_table,
                slots.take(1),
                Rights::all(),
            )
            .unwrap_or_else(|error| panic!("component archive Retype failed: {:?}", error.code()));
        let archive = KeyTableKey::from_key(archive_key);
        let scratch = slots.take(1);
        let mut next_slot = 1;
        for segment in image.segments {
            let rights = segment_rights(segment.meta.permissions);
            let base = segment.data.as_ptr() as u64;
            assert_eq!(base % PAGE, 0, "bundled segment data must be page-aligned");
            for index in 0..segment.data.len() as u64 / PAGE {
                // The bundling image is identity-mapped: its address is the
                // physical address of the retained page.
                let frame = {
                    // SAFETY: the live boot table; no reference survives the
                    // capability calls below.
                    let table = unsafe { &mut *(self.boot_table_addr as *mut KeyTable) };
                    self.retained.grant_page(
                        table,
                        KeySlot(scratch),
                        BOOT_TABLE_GUARD,
                        base + index * PAGE,
                    )
                };
                FrameKey::from_key(frame)
                    .map(
                        component.address_space_key,
                        segment.meta.virt_addr + index * PAGE,
                        rights,
                        0,
                    )
                    .unwrap_or_else(|error| {
                        panic!(
                            "{} {} Frame.Map failed: {:?}",
                            image.name,
                            segment.meta.name,
                            error.code()
                        )
                    });
                self.self_table
                    .transfer(frame, &archive, next_slot)
                    .unwrap_or_else(|error| {
                        panic!("component page Move failed: {:?}", error.code())
                    });
                next_slot += 1;
            }
        }

        if let Some(bss) = image.bss {
            let count = u32::try_from(bss.page_count()).unwrap_or(u32::MAX);
            let first = slots.take(count);
            let frames = self.retype_frames(first, count);
            Self::map_frames(
                first,
                frames.incarnation(),
                count,
                component.address_space_key,
                bss.virt_addr,
                Rights(Rights::READ | Rights::WRITE),
            );
        }
    }
}

/// A 2 MiB span of an `AddressSpace` reserved for Thread stacks, each with an
/// unmapped guard page below it and above it.
pub struct StackRegion {
    address_space: RawKey,
    base: u64,
    /// Next free page index in the span; page 0 is the first guard.
    next_page: u64,
}

/// One mapped stack: `[bottom, top)`, 16-byte aligned, guarded on both sides.
#[derive(Clone, Copy, Debug)]
pub struct UserStack {
    pub bottom: u64,
    pub top: u64,
}

impl Builder<'_> {
    /// Reserve the 2 MiB span at `base` in `component` for stacks.
    pub fn stack_region(
        &self,
        component: &Component,
        base: u64,
        slots: &mut SlotCursor,
    ) -> StackRegion {
        assert_eq!(base % LEAF_SPAN, 0, "a stack region spans one L3 table");
        Self::map_table(self.carve_tables(slots.take(1), 1), component.l2, base);
        StackRegion {
            address_space: component.address_space_key,
            base,
            next_page: 1,
        }
    }

    /// Map a `pages`-page EL0 stack in `region`, leaving the page below it
    /// and the page above it unmapped as guards.
    pub fn user_stack(
        &self,
        region: &mut StackRegion,
        pages: u32,
        slots: &mut SlotCursor,
    ) -> UserStack {
        let first_page = region.next_page;
        region.next_page = first_page + u64::from(pages) + 1;
        assert!(
            region.next_page * PAGE < LEAF_SPAN,
            "stack region exhausted (it must keep its trailing guard page)"
        );
        let first = slots.take(pages);
        let frames = self.retype_frames(first, pages);
        let bottom = region.base + first_page * PAGE;
        Self::map_frames(
            first,
            frames.incarnation(),
            pages,
            region.address_space,
            bottom,
            Rights(Rights::READ | Rights::WRITE),
        );
        UserStack {
            bottom,
            top: bottom + u64::from(pages) * PAGE,
        }
    }
}
