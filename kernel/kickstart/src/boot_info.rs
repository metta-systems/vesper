//! Boot regions
//!
//! Define a map of memory regions used during boot allocations.
//!
//! Insert sections that are either "free" or "used", disparate used sections can not overlap,
//! overlapping free sections are merged (unless they have different `MemAttributes`).
//!
#[cfg(feature = "qemu")]
use libqemu::semi_println;
use {
    core::{cell::LazyCell, fmt},
    libaddress::PhysAddr,
    liblocking::IRQSafeNullLock,
    libmapping::{AccessPermissions, AttributeFields, MemAttributes},
    snafu::Snafu,
};

//=================================================================================================
// BootInfoMemRegion
//=================================================================================================

/// Memory region.
#[derive(Default, Copy, Clone, Debug)]
pub struct BootInfoMemRegion {
    /// Region start is inclusive.
    pub start_inclusive: PhysAddr,
    /// Region end is exclusive.
    pub end_exclusive: PhysAddr,
    pub attributes: AttributeFields,
    pub name: &'static str,
}

impl BootInfoMemRegion {
    /// Create an empty region.
    pub const fn new() -> Self {
        Self {
            start_inclusive: PhysAddr::zero(),
            end_exclusive: PhysAddr::zero(),
            attributes: AttributeFields::defaulted(),
            name: "",
        }
    }

    /// Create an occupied or free region with start and end.
    /// Region is in range [start, end), that is, for start 0x0 and end 0x2000 the region will
    /// occupy memory between addresses 0x0 and 0x1fff.
    pub fn at(
        start_inclusive: PhysAddr,
        end_exclusive: PhysAddr,
        free: bool,
        name: &'static str,
    ) -> Self {
        Self {
            start_inclusive: start_inclusive.min(end_exclusive),
            end_exclusive: end_exclusive.max(start_inclusive),
            attributes: AttributeFields {
                occupied: !free,
                ..AttributeFields::default()
            },
            name,
        }
    }

    /// Create a region with explicit attributes.
    pub fn with_attributes(
        start_inclusive: PhysAddr,
        end_exclusive: PhysAddr,
        attributes: AttributeFields,
        name: &'static str,
    ) -> Self {
        Self {
            start_inclusive: start_inclusive.min(end_exclusive),
            end_exclusive: end_exclusive.max(start_inclusive),
            attributes,
            name,
        }
    }

    /// Calculate region size.
    pub fn size(&self) -> usize {
        self.end_exclusive - self.start_inclusive
    }

    /// Is this region empty?
    pub fn is_empty(&self) -> bool {
        self.start_inclusive == self.end_exclusive
    }

    /// Is this a free (unoccupied) region?
    pub fn is_free(&self) -> bool {
        !self.attributes.occupied && !self.is_empty()
    }

    /// Is this a used (occupied) region?
    pub fn is_used(&self) -> bool {
        self.attributes.occupied && !self.is_empty()
    }

    /// Clear the region to empty.
    pub fn clear(&mut self) {
        self.start_inclusive = PhysAddr::zero();
        self.end_exclusive = PhysAddr::zero();
        self.attributes = AttributeFields::defaulted();
        self.name = "";
    }

    /// Does this region intersect the given one?
    /// Based on [Intersection of 1D segments](https://eli.thegreenplace.net/2008/08/15/intersection-of-1d-segments/).
    ///
    /// Since end is exclusive, the actual value is one less than what it contains, for this reason,
    /// end equal to other's start means they touch but do not intersect.
    ///
    /// Assumes `start_inclusive` <= `end_exclusive`, which holds for memory regions by construction.
    pub fn intersects(&self, other: &BootInfoMemRegion) -> bool {
        self.end_exclusive > other.start_inclusive && other.end_exclusive > self.start_inclusive
    }

    /// Does this region touch or overlap the given one?
    /// Two regions touch when one's end equals the other's start.
    /// This is used for merging adjacent free regions.
    pub fn touches_or_overlaps(&self, other: &BootInfoMemRegion) -> bool {
        self.end_exclusive >= other.start_inclusive && other.end_exclusive >= self.start_inclusive
    }

    /// Check if two regions have compatible attributes for merging.
    /// Compares `mem_attributes`, `acc_perms`, and executable — ignores the occupied flag.
    pub fn compatible_attributes(&self, other: &BootInfoMemRegion) -> bool {
        self.attributes.mem_attributes == other.attributes.mem_attributes
            && self.attributes.acc_perms == other.attributes.acc_perms
            && self.attributes.executable == other.attributes.executable
    }
}

impl fmt::Display for BootInfoMemRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (size, unit) = liblog::size_human_readable_ceil(self.size());

        write!(
            f,
            "[{} - {}) | {: >3} {: <3} | {} | {}",
            self.start_inclusive, self.end_exclusive, size, unit, self.attributes, self.name
        )
    }
}

// #[cfg(test)]
// mod boot_info_region_tests {
//     use super::*;

//     #[test_case]
//     fn test_construct_regular_region() {
//         let region = BootInfoMemRegion::at(0x0.into(), 0x2000.into(), true, "RAM");
//         assert_eq!(region.start_inclusive, 0x0);
//         assert_eq!(region.end_exclusive, 0x2000);
//         assert_eq!(region.size(), 0x2000);
//         assert_eq!(region.attributes.occupied, false);
//         assert_eq!(region.name, "RAM");
//     }

//     #[test_case]
//     fn test_construct_reverse_region() {
//         let region = BootInfoMemRegion::at(0x2000.into(), 0x0.into(), true, "RAM");
//         assert_eq!(region.start_inclusive, 0x0);
//         assert_eq!(region.end_exclusive, 0x2000);
//         assert_eq!(region.size(), 0x2000);
//         assert_eq!(region.attributes.occupied, false);
//         assert_eq!(region.name, "RAM");
//     }

//     #[test_case]
//     fn test_regions_touch() {
//         let region1 = BootInfoMemRegion::at(0x0.into(), 0x2000.into(), false, "R1");
//         let region2 = BootInfoMemRegion::at(0x2000.into(), 0x4000.into(), false, "R2");
//         assert_eq!(region1.intersects(&region2), false);
//         assert_eq!(region2.intersects(&region1), false);
//         // But they do touch
//         assert_eq!(region1.touches_or_overlaps(&region2), true);
//         assert_eq!(region2.touches_or_overlaps(&region1), true);
//     }

//     #[test_case]
//     fn test_regions_intersect() {
//         let region1 = BootInfoMemRegion::at(0x0.into(), 0x2000.into(), false, "R1");
//         let region2 = BootInfoMemRegion::at(0x1000.into(), 0x3000.into(), false, "R2");
//         assert_eq!(region1.intersects(&region2), true);
//         assert_eq!(region2.intersects(&region1), true);
//     }

//     #[test_case]
//     fn test_self_intersect() {
//         let region1 = BootInfoMemRegion::at(0x0.into(), 0x2000.into(), false, "R1");
//         let region2 = BootInfoMemRegion::at(0x0.into(), 0x2000.into(), false, "R2");
//         assert_eq!(region1.intersects(&region2), true);
//         assert_eq!(region2.intersects(&region1), true);
//     }

//     #[test_case]
//     fn test_regions_fully_overlap() {
//         let outer = BootInfoMemRegion::at(0x0.into(), 0x4000.into(), false, "outer");
//         let inner = BootInfoMemRegion::at(0x1000.into(), 0x3000.into(), false, "inner");
//         assert_eq!(outer.intersects(&inner), true);
//         assert_eq!(inner.intersects(&outer), true);
//     }

//     #[test_case]
//     fn test_regions_disjoint() {
//         let region1 = BootInfoMemRegion::at(0x0.into(), 0x1000.into(), false, "R1");
//         let region2 = BootInfoMemRegion::at(0x2000.into(), 0x3000.into(), false, "R2");
//         assert_eq!(region1.intersects(&region2), false);
//         assert_eq!(region2.intersects(&region1), false);
//         assert_eq!(region1.touches_or_overlaps(&region2), false);
//         assert_eq!(region2.touches_or_overlaps(&region1), false);
//     }

//     #[test_case]
//     fn test_compatible_attributes() {
//         let r1 = BootInfoMemRegion::at(0x0.into(), 0x1000.into(), true, "R1");
//         let r2 = BootInfoMemRegion::at(0x1000.into(), 0x2000.into(), true, "R2");
//         assert_eq!(r1.compatible_attributes(&r2), true);

//         let r3 = BootInfoMemRegion::with_attributes(
//             0x2000.into(),
//             0x3000.into(),
//             AttributeFields {
//                 mem_attributes: MemAttributes::Device,
//                 ..AttributeFields::default()
//             },
//             "MMIO",
//         );
//         assert_eq!(r1.compatible_attributes(&r3), false);
//     }

//     #[test_case]
//     fn test_is_free_and_is_used() {
//         let free = BootInfoMemRegion::at(0x0.into(), 0x1000.into(), true, "free");
//         assert_eq!(free.is_free(), true);
//         assert_eq!(free.is_used(), false);

//         let used = BootInfoMemRegion::at(0x0.into(), 0x1000.into(), false, "used");
//         assert_eq!(used.is_free(), false);
//         assert_eq!(used.is_used(), true);

//         let empty = BootInfoMemRegion::new();
//         assert_eq!(empty.is_free(), false);
//         assert_eq!(empty.is_used(), false);
//     }
// }

//=================================================================================================
// BootInfo
//=================================================================================================

const NUM_MEM_REGIONS: usize = 256;

#[derive(Snafu, Debug, PartialEq)]
pub enum BootInfoError {
    NoFreeSlots,
    OverlappingUsedRegions,
}

pub struct BootInfo {
    pub regions: [BootInfoMemRegion; NUM_MEM_REGIONS],
    pub num_regions: usize,
}

/// Implement Default manually to work around Rust not defining Default for arrays over 32 items.
impl Default for BootInfo {
    fn default() -> Self {
        Self::new()
    }
}

impl BootInfo {
    /// Create empty boot region map.
    pub const fn new() -> BootInfo {
        BootInfo {
            regions: [BootInfoMemRegion::new(); NUM_MEM_REGIONS],
            num_regions: 0,
        }
    }

    /// Low-level: find an empty slot and place a region there.
    fn insert_raw(&mut self, region: BootInfoMemRegion) -> Result<usize, BootInfoError> {
        if region.is_empty() {
            return Ok(0);
        }
        for (i, slot) in self.regions.iter_mut().enumerate() {
            if slot.is_empty() {
                *slot = region;
                if i >= self.num_regions {
                    self.num_regions = i + 1;
                }
                return Ok(i);
            }
        }
        Err(BootInfoError::NoFreeSlots)
    }

    /// Insert a free memory region.
    ///
    /// The region is added and then merged with any adjacent or overlapping free regions
    /// that have compatible attributes (same `mem_attributes`, `acc_perms`, executable).
    pub fn insert_free_region(
        &mut self,
        start: PhysAddr,
        end: PhysAddr,
        attributes: AttributeFields,
        name: &'static str,
    ) -> Result<(), BootInfoError> {
        let region = BootInfoMemRegion::with_attributes(
            start,
            end,
            AttributeFields {
                occupied: false,
                ..attributes
            },
            name,
        );
        if region.is_empty() {
            return Ok(());
        }
        #[cfg(feature = "qemu")]
        semi_println!("BOOT_INFO.insert_free_region: {}", region);
        self.insert_raw(region)?;
        self.merge_free_regions();
        Ok(())
    }

    /// Insert a used (occupied) memory region.
    ///
    /// The region is recorded and then cut out of any overlapping free regions.
    /// Returns an error if the new used region overlaps an existing used region.
    pub fn insert_used_region(
        &mut self,
        start: PhysAddr,
        end: PhysAddr,
        attributes: AttributeFields,
        name: &'static str,
    ) -> Result<(), BootInfoError> {
        let region = BootInfoMemRegion::with_attributes(
            start,
            end,
            AttributeFields {
                occupied: true,
                ..attributes
            },
            name,
        );
        if region.is_empty() {
            return Ok(());
        }
        #[cfg(feature = "qemu")]
        semi_println!("BOOT_INFO.insert_used_region: {}", region);

        // Check for overlap with existing used regions.
        for slot in &self.regions {
            if slot.is_used() && slot.intersects(&region) {
                #[cfg(feature = "qemu")]
                semi_println!(
                    "BOOT_INFO.insert_used_region: ERROR overlaps existing used region: {}",
                    slot
                );
                return Err(BootInfoError::OverlappingUsedRegions);
            }
        }

        self.insert_raw(region)?;
        self.cut_out_used_from_free(&region)?;
        Ok(())
    }

    /// Insert an overlay region that fills gaps between existing used regions.
    ///
    /// The overlay covers `[start, end)` but instead of failing on overlap with
    /// existing used regions, it inserts new used regions only for the gaps.
    /// This is useful for marking a large area (like the `Kickstart` bump allocator
    /// arena) as used, where sub-regions have already been individually recorded.
    pub fn insert_overlay_region(
        &mut self,
        start: PhysAddr,
        end: PhysAddr,
        attributes: AttributeFields,
        name: &'static str,
    ) -> Result<(), BootInfoError> {
        let overlay_start = start.min(end);
        let overlay_end = end.max(start);

        if overlay_start == overlay_end {
            return Ok(());
        }

        #[cfg(feature = "qemu")]
        semi_println!(
            "BOOT_INFO.insert_overlay_region: [{} - {}) {}",
            overlay_start,
            overlay_end,
            name
        );

        // Collect starts and ends of existing used regions that intersect the overlay.
        // We only need the boundaries to compute gaps, so collect (start, end) pairs.
        let mut boundaries: [(PhysAddr, PhysAddr); NUM_MEM_REGIONS] =
            [(PhysAddr::zero(), PhysAddr::zero()); NUM_MEM_REGIONS];
        let mut count = 0;

        for slot in &self.regions {
            if !slot.is_used() {
                continue;
            }
            // Clip to overlay range.
            let s = slot.start_inclusive.max(overlay_start);
            let e = slot.end_exclusive.min(overlay_end);
            if s < e {
                boundaries[count] = (s, e);
                count += 1;
            }
        }

        // Sort by start address.
        boundaries[..count].sort_unstable_by_key(|&(s, _)| s);

        // Walk left-to-right, inserting gap regions.
        let mut cursor = overlay_start;

        for &(used_start, used_end) in boundaries.iter().take(count) {
            if cursor < used_start {
                // Gap before this used region.
                let gap = BootInfoMemRegion::with_attributes(
                    cursor,
                    used_start,
                    AttributeFields {
                        occupied: true,
                        ..attributes
                    },
                    name,
                );
                self.insert_raw(gap)?;
                self.cut_out_used_from_free(&gap)?;
            }
            // Advance cursor past this used region.
            if used_end > cursor {
                cursor = used_end;
            }
        }

        // Trailing gap after the last used region.
        if cursor < overlay_end {
            let gap = BootInfoMemRegion::with_attributes(
                cursor,
                overlay_end,
                AttributeFields {
                    occupied: true,
                    ..attributes
                },
                name,
            );
            self.insert_raw(gap)?;
            self.cut_out_used_from_free(&gap)?;
        }

        Ok(())
    }

    /// Merge all overlapping or adjacent free regions with compatible attributes.
    /// Repeats until no more merges are possible.
    fn merge_free_regions(&mut self) {
        loop {
            let mut merged = false;
            // Find a pair of free regions to merge.
            // We need indices so we can mutate the array.
            'outer: for i in 0..self.num_regions {
                if !self.regions[i].is_free() {
                    continue;
                }
                for j in (i + 1)..self.num_regions {
                    if !self.regions[j].is_free() {
                        continue;
                    }
                    if self.regions[i].touches_or_overlaps(&self.regions[j])
                        && self.regions[i].compatible_attributes(&self.regions[j])
                    {
                        // Merge j into i.
                        let new_start = self.regions[i]
                            .start_inclusive
                            .min(self.regions[j].start_inclusive);
                        let new_end = self.regions[i]
                            .end_exclusive
                            .max(self.regions[j].end_exclusive);
                        self.regions[i].start_inclusive = new_start;
                        self.regions[i].end_exclusive = new_end;
                        // Keep name from the larger region (or the first one).
                        self.regions[j].clear();
                        merged = true;
                        break 'outer;
                    }
                }
            }
            if !merged {
                break;
            }
        }
    }

    /// Cut a used region out of all overlapping free regions.
    fn cut_out_used_from_free(&mut self, used: &BootInfoMemRegion) -> Result<(), BootInfoError> {
        // Collect regions to split (we can't mutate while iterating for splits that
        // need to insert new regions).
        // Process in a loop: each iteration handles one free region.
        let mut idx = 0;
        while idx < self.num_regions {
            let free = &self.regions[idx];
            if !free.is_free() || !free.intersects(used) {
                idx += 1;
                continue;
            }

            let free_start = self.regions[idx].start_inclusive;
            let free_end = self.regions[idx].end_exclusive;
            let free_attrs = self.regions[idx].attributes;
            let free_name = self.regions[idx].name;

            // Case 1: used fully contains free → remove free entirely
            if used.start_inclusive <= free_start && used.end_exclusive >= free_end {
                self.regions[idx].clear();
                idx += 1;
                continue;
            }

            // Case 2: used overlaps start of free → shrink free
            if used.start_inclusive <= free_start && used.end_exclusive < free_end {
                self.regions[idx].start_inclusive = used.end_exclusive;
                idx += 1;
                continue;
            }

            // Case 3: used overlaps end of free → shrink free
            if used.start_inclusive > free_start && used.end_exclusive >= free_end {
                self.regions[idx].end_exclusive = used.start_inclusive;
                idx += 1;
                continue;
            }

            // Case 4: used is entirely inside free → split free into two
            if used.start_inclusive > free_start && used.end_exclusive < free_end {
                // Shrink current region to the left part.
                self.regions[idx].end_exclusive = used.start_inclusive;
                // Insert right part as a new free region.
                let right = BootInfoMemRegion::with_attributes(
                    used.end_exclusive,
                    free_end,
                    free_attrs,
                    free_name,
                );
                self.insert_raw(right)?;
                idx += 1;
                continue;
            }

            idx += 1;
        }
        Ok(())
    }

    /// Allocate a region of given `size_bits` size.
    ///
    /// Search for a free mem region that will be the best fit for an allocation. We favour
    /// allocations that are aligned to either end of the region. If an allocation must split
    /// a region we favour an unbalanced split. In both cases we attempt to use the smallest
    /// region possible. In general this means we aim to make the size of the smallest remaining
    /// region smaller (ideally zero) followed by making the size of the largest remaining
    /// region smaller.
    pub fn alloc_region(
        &mut self,
        size_bits: usize,
        name: &'static str,
    ) -> Result<PhysAddr, BootInfoError> {
        let mut reg_index: usize = 0;
        let mut reg = BootInfoMemRegion::new();
        let mut rem_small = BootInfoMemRegion::new();
        let mut rem_large = BootInfoMemRegion::new();

        // Iterate only free regions.
        for (i, reg_iter) in self
            .regions
            .iter()
            .enumerate()
            .filter(|(_, reg)| reg.is_free())
        {
            // Determine whether placing the region at the start or the end
            // will create a bigger left over region.
            let aligned_start = reg_iter.start_inclusive.aligned_up(1_u64 << size_bits);
            let aligned_end = reg_iter.end_exclusive.aligned_down(1_u64 << size_bits);
            let new_reg = if aligned_start - reg_iter.start_inclusive
                < reg_iter.end_exclusive - aligned_end
            {
                BootInfoMemRegion::at(
                    aligned_start,
                    aligned_start + (1_u64 << size_bits),
                    false,
                    name,
                )
            } else {
                BootInfoMemRegion::at(aligned_end - (1_u64 << size_bits), aligned_end, false, name)
            };

            if new_reg.start_inclusive >= reg_iter.start_inclusive
                && new_reg.end_exclusive <= reg_iter.end_exclusive
            {
                let mut new_rem_small = BootInfoMemRegion::new();
                let mut new_rem_large = BootInfoMemRegion::new();

                if new_reg.start_inclusive - reg_iter.start_inclusive
                    < reg_iter.end_exclusive - new_reg.end_exclusive
                {
                    new_rem_small.start_inclusive = reg_iter.start_inclusive;
                    new_rem_small.end_exclusive = new_reg.start_inclusive;
                    new_rem_large.start_inclusive = new_reg.end_exclusive;
                    new_rem_large.end_exclusive = reg_iter.end_exclusive;
                } else {
                    new_rem_large.start_inclusive = reg_iter.start_inclusive;
                    new_rem_large.end_exclusive = new_reg.start_inclusive;
                    new_rem_small.start_inclusive = new_reg.end_exclusive;
                    new_rem_small.end_exclusive = reg_iter.end_exclusive;
                }
                // Find better fit.
                if reg.is_empty()
                    || (new_rem_small.size() < rem_small.size())
                    || (new_rem_small.size() == rem_small.size()
                        && new_rem_large.size() < rem_large.size())
                {
                    reg = new_reg;
                    rem_small = new_rem_small;
                    rem_large = new_rem_large;
                    reg_index = i;
                }
            }
        }

        if reg.is_empty() {
            return Err(BootInfoError::NoFreeSlots);
        }

        // Remove the region in question.
        self.regions[reg_index].clear();

        // Add the remaining regions in largest to smallest order.
        self.insert_raw(rem_large)?;
        if self.insert_raw(rem_small).is_err() {
            #[cfg(feature = "qemu")]
            semi_println!(
                "BootInfo::alloc_region(): wasted {} bytes due to alignment, try to increase NUM_MEM_REGIONS",
                rem_small.size()
            );
        }
        Ok(reg.start_inclusive)
    }

    /// Sort regions by start address. Empty regions sort to the end.
    pub fn sort(&mut self) {
        self.regions
            .sort_unstable_by(|a, b| match (a.is_empty(), b.is_empty()) {
                (true, true) => core::cmp::Ordering::Equal,
                (true, false) => core::cmp::Ordering::Greater,
                (false, true) => core::cmp::Ordering::Less,
                (false, false) => a.start_inclusive.cmp(&b.start_inclusive),
            });
        // Update num_regions after sort.
        self.num_regions = self
            .regions
            .iter()
            .rposition(|r| !r.is_empty())
            .map_or(0, |p| p + 1);
    }

    /// Remove empty gaps in the region array by shifting entries down.
    pub fn compact(&mut self) {
        let mut write = 0;
        for read in 0..self.num_regions {
            if !self.regions[read].is_empty() {
                if write != read {
                    self.regions[write] = self.regions[read];
                    self.regions[read].clear();
                }
                write += 1;
            }
        }
        self.num_regions = write;
    }

    /// Print all non-empty regions for debug purposes.
    pub fn dump(&self) {
        #[cfg(feature = "qemu")]
        semi_println!("BOOT_INFO: {} region(s):", self.count());
        for region in &self.regions {
            if !region.is_empty() {
                #[cfg(feature = "qemu")]
                semi_println!("  {}", region);
            }
        }
    }

    /// Count non-empty regions.
    pub fn count(&self) -> usize {
        self.regions.iter().filter(|r| !r.is_empty()).count()
    }

    /// Iterate over all non-empty regions.
    pub fn iter(&self) -> impl Iterator<Item = &BootInfoMemRegion> {
        self.regions.iter().filter(|r| !r.is_empty())
    }

    /// Iterate over free regions.
    pub fn free_regions(&self) -> impl Iterator<Item = &BootInfoMemRegion> {
        self.regions.iter().filter(|r| r.is_free())
    }

    /// Iterate over used regions.
    pub fn used_regions(&self) -> impl Iterator<Item = &BootInfoMemRegion> {
        self.regions.iter().filter(|r| r.is_used())
    }

    /// Total free memory in bytes.
    pub fn total_free(&self) -> usize {
        self.free_regions().map(BootInfoMemRegion::size).sum()
    }

    /// Total used memory in bytes.
    pub fn total_used(&self) -> usize {
        self.used_regions().map(BootInfoMemRegion::size).sum()
    }
}

// Should go to BSS
pub static BOOT_INFO: IRQSafeNullLock<LazyCell<BootInfo>> =
    IRQSafeNullLock::new(LazyCell::new(BootInfo::new));

// #[cfg(test)]
// mod boot_info_tests {
//     use super::*;

//     fn default_attrs() -> AttributeFields {
//         AttributeFields::default()
//     }

//     fn device_attrs() -> AttributeFields {
//         AttributeFields {
//             mem_attributes: MemAttributes::Device,
//             acc_perms: AccessPermissions::ReadWrite,
//             executable: false,
//             occupied: false,
//         }
//     }

//     // -- insert_free_region tests --

//     #[test_case]
//     fn test_insert_free_region() {
//         let mut bi = BootInfo::new();
//         let res = bi.insert_free_region(0x0.into(), 0x4000.into(), default_attrs(), "RAM");
//         assert!(res.is_ok());
//         assert_eq!(bi.count(), 1);
//         assert_eq!(bi.total_free(), 0x4000);
//     }

//     #[test_case]
//     fn test_insert_free_region_empty_is_noop() {
//         let mut bi = BootInfo::new();
//         let res = bi.insert_free_region(0x1000.into(), 0x1000.into(), default_attrs(), "empty");
//         assert!(res.is_ok());
//         assert_eq!(bi.count(), 0);
//     }

//     #[test_case]
//     fn test_insert_free_merge_adjacent() {
//         let mut bi = BootInfo::new();
//         bi.insert_free_region(0x0.into(), 0x2000.into(), default_attrs(), "RAM")
//             .unwrap();
//         bi.insert_free_region(0x2000.into(), 0x4000.into(), default_attrs(), "RAM")
//             .unwrap();
//         // Should merge into one region [0x0, 0x4000).
//         assert_eq!(bi.count(), 1);
//         assert_eq!(bi.total_free(), 0x4000);
//         let r = bi.free_regions().next().unwrap();
//         assert_eq!(r.start_inclusive, 0x0);
//         assert_eq!(r.end_exclusive, 0x4000);
//     }

//     #[test_case]
//     fn test_insert_free_merge_overlapping() {
//         let mut bi = BootInfo::new();
//         bi.insert_free_region(0x0.into(), 0x3000.into(), default_attrs(), "RAM")
//             .unwrap();
//         bi.insert_free_region(0x2000.into(), 0x5000.into(), default_attrs(), "RAM")
//             .unwrap();
//         assert_eq!(bi.count(), 1);
//         assert_eq!(bi.total_free(), 0x5000);
//         let r = bi.free_regions().next().unwrap();
//         assert_eq!(r.start_inclusive, 0x0);
//         assert_eq!(r.end_exclusive, 0x5000);
//     }

//     #[test_case]
//     fn test_insert_free_no_merge_different_attrs() {
//         let mut bi = BootInfo::new();
//         bi.insert_free_region(0x0.into(), 0x2000.into(), default_attrs(), "RAM")
//             .unwrap();
//         bi.insert_free_region(0x2000.into(), 0x4000.into(), device_attrs(), "MMIO")
//             .unwrap();
//         // Different attributes: should NOT merge.
//         assert_eq!(bi.count(), 2);
//     }

//     #[test_case]
//     fn test_insert_free_merge_three_regions() {
//         let mut bi = BootInfo::new();
//         bi.insert_free_region(0x0.into(), 0x1000.into(), default_attrs(), "R1")
//             .unwrap();
//         bi.insert_free_region(0x2000.into(), 0x3000.into(), default_attrs(), "R3")
//             .unwrap();
//         // Insert a bridging region that touches both.
//         bi.insert_free_region(0x1000.into(), 0x2000.into(), default_attrs(), "R2")
//             .unwrap();
//         assert_eq!(bi.count(), 1);
//         assert_eq!(bi.total_free(), 0x3000);
//     }

//     // -- insert_used_region tests --

//     #[test_case]
//     fn test_insert_used_cuts_free_start() {
//         let mut bi = BootInfo::new();
//         bi.insert_free_region(0x0.into(), 0x4000.into(), default_attrs(), "RAM")
//             .unwrap();
//         bi.insert_used_region(0x0.into(), 0x1000.into(), default_attrs(), "kernel")
//             .unwrap();
//         // Free region should be shrunk to [0x1000, 0x4000).
//         assert_eq!(bi.total_free(), 0x3000);
//         assert_eq!(bi.total_used(), 0x1000);
//         let free = bi.free_regions().next().unwrap();
//         assert_eq!(free.start_inclusive, 0x1000);
//         assert_eq!(free.end_exclusive, 0x4000);
//     }

//     #[test_case]
//     fn test_insert_used_cuts_free_end() {
//         let mut bi = BootInfo::new();
//         bi.insert_free_region(0x0.into(), 0x4000.into(), default_attrs(), "RAM")
//             .unwrap();
//         bi.insert_used_region(0x3000.into(), 0x4000.into(), default_attrs(), "kernel")
//             .unwrap();
//         assert_eq!(bi.total_free(), 0x3000);
//         let free = bi.free_regions().next().unwrap();
//         assert_eq!(free.start_inclusive, 0x0);
//         assert_eq!(free.end_exclusive, 0x3000);
//     }

//     #[test_case]
//     fn test_insert_used_splits_free() {
//         let mut bi = BootInfo::new();
//         bi.insert_free_region(0x0.into(), 0x8000.into(), default_attrs(), "RAM")
//             .unwrap();
//         bi.insert_used_region(0x2000.into(), 0x4000.into(), default_attrs(), "kernel")
//             .unwrap();
//         // Should split into [0x0, 0x2000) and [0x4000, 0x8000).
//         let free: usize = bi.free_regions().map(|r| r.size()).sum();
//         assert_eq!(free, 0x6000);
//         assert_eq!(bi.free_regions().count(), 2);
//         assert_eq!(bi.used_regions().count(), 1);
//     }

//     #[test_case]
//     fn test_insert_used_subsumes_free() {
//         let mut bi = BootInfo::new();
//         bi.insert_free_region(0x1000.into(), 0x3000.into(), default_attrs(), "RAM")
//             .unwrap();
//         bi.insert_used_region(0x0.into(), 0x4000.into(), default_attrs(), "kernel")
//             .unwrap();
//         assert_eq!(bi.total_free(), 0);
//         assert_eq!(bi.total_used(), 0x4000);
//     }

//     #[test_case]
//     fn test_insert_used_overlapping_error() {
//         let mut bi = BootInfo::new();
//         bi.insert_used_region(0x0.into(), 0x2000.into(), default_attrs(), "kernel")
//             .unwrap();
//         let res = bi.insert_used_region(0x1000.into(), 0x3000.into(), default_attrs(), "dtb");
//         assert_eq!(res, Err(BootInfoError::OverlappingUsedRegions));
//     }

//     #[test_case]
//     fn test_insert_used_no_overlap_ok() {
//         let mut bi = BootInfo::new();
//         bi.insert_used_region(0x0.into(), 0x2000.into(), default_attrs(), "kernel")
//             .unwrap();
//         bi.insert_used_region(0x2000.into(), 0x4000.into(), default_attrs(), "dtb")
//             .unwrap();
//         assert_eq!(bi.used_regions().count(), 2);
//     }

//     #[test_case]
//     fn test_insert_used_cuts_multiple_free_regions() {
//         let mut bi = BootInfo::new();
//         // Two separate free regions.
//         bi.insert_free_region(0x0.into(), 0x2000.into(), default_attrs(), "R1")
//             .unwrap();
//         bi.insert_free_region(0x3000.into(), 0x5000.into(), default_attrs(), "R2")
//             .unwrap();
//         // Used region spans across both.
//         bi.insert_used_region(0x1000.into(), 0x4000.into(), default_attrs(), "kernel")
//             .unwrap();
//         // R1 shrunk to [0x0, 0x1000), R2 shrunk to [0x4000, 0x5000).
//         assert_eq!(bi.total_free(), 0x2000);
//         assert_eq!(bi.total_used(), 0x3000);
//     }

//     // -- sort and compact tests --

//     #[test_case]
//     fn test_sort() {
//         let mut bi = BootInfo::new();
//         bi.insert_free_region(0x4000.into(), 0x8000.into(), default_attrs(), "R2")
//             .unwrap();
//         bi.insert_free_region(0x0.into(), 0x2000.into(), default_attrs(), "R1")
//             .unwrap();
//         bi.sort();
//         let regions: Vec<_> = bi.iter().collect();
//         assert!(regions[0].start_inclusive < regions[1].start_inclusive);
//     }

//     #[test_case]
//     fn test_compact() {
//         let mut bi = BootInfo::new();
//         bi.insert_free_region(0x0.into(), 0x2000.into(), default_attrs(), "R1")
//             .unwrap();
//         bi.insert_free_region(0x4000.into(), 0x6000.into(), default_attrs(), "R2")
//             .unwrap();
//         // Manually clear first to create a gap.
//         bi.regions[0].clear();
//         assert_eq!(bi.count(), 1);
//         bi.compact();
//         // After compact, the remaining region should be in slot 0.
//         assert!(!bi.regions[0].is_empty());
//         assert_eq!(bi.regions[0].start_inclusive, 0x4000);
//     }

//     // -- alloc_region tests --

//     #[test_case]
//     fn test_alloc_region_no_memory() {
//         let mut bi = BootInfo::new();
//         let res = bi.alloc_region(12, "test");
//         assert_eq!(res, Err(BootInfoError::NoFreeSlots));
//     }

//     #[test_case]
//     fn test_alloc_region_basic() {
//         let mut bi = BootInfo::new();
//         bi.insert_free_region(0x0.into(), 0x10_0000.into(), default_attrs(), "RAM")
//             .unwrap();
//         let addr = bi.alloc_region(12, "page").unwrap(); // 4 KiB
//         // The allocated address should be within the original free region.
//         assert!(addr >= PhysAddr::from(0x0u64));
//         assert!(addr < PhysAddr::from(0x10_0000u64));
//         // Free space should be reduced by 4 KiB.
//         assert_eq!(bi.total_free(), 0x10_0000 - 0x1000);
//     }

//     // -- insert_overlay_region tests --

//     #[test_case]
//     fn test_overlay_fills_gaps_between_used() {
//         let mut bi = BootInfo::new();
//         bi.insert_free_region(0x0.into(), 0xA000.into(), default_attrs(), "RAM")
//             .unwrap();
//         bi.insert_used_region(0x1000.into(), 0x3000.into(), default_attrs(), "kernel")
//             .unwrap();
//         bi.insert_used_region(0x5000.into(), 0x7000.into(), default_attrs(), "stack")
//             .unwrap();
//         // Overlay [0x0, 0xA000) should fill three gaps.
//         bi.insert_overlay_region(0x0.into(), 0xA000.into(), default_attrs(), "init")
//             .unwrap();

//         bi.compact();
//         bi.sort();

//         // Should have: [0,1000) init, [1000,3000) kernel, [3000,5000) init,
//         //              [5000,7000) stack, [7000,A000) init = 5 used regions
//         assert_eq!(bi.used_regions().count(), 5);
//         assert_eq!(bi.total_used(), 0xA000);
//         assert_eq!(bi.total_free(), 0);
//     }

//     #[test_case]
//     fn test_overlay_no_existing_used() {
//         let mut bi = BootInfo::new();
//         bi.insert_free_region(0x0.into(), 0x4000.into(), default_attrs(), "RAM")
//             .unwrap();
//         // Overlay with no existing used regions — becomes one big used region.
//         bi.insert_overlay_region(0x0.into(), 0x4000.into(), default_attrs(), "init")
//             .unwrap();
//         assert_eq!(bi.used_regions().count(), 1);
//         assert_eq!(bi.total_used(), 0x4000);
//         assert_eq!(bi.total_free(), 0);
//     }

//     #[test_case]
//     fn test_overlay_fully_covered() {
//         let mut bi = BootInfo::new();
//         // Used region covers entire overlay — no gaps to fill.
//         bi.insert_used_region(0x0.into(), 0x4000.into(), default_attrs(), "kernel")
//             .unwrap();
//         bi.insert_overlay_region(0x0.into(), 0x4000.into(), default_attrs(), "init")
//             .unwrap();
//         assert_eq!(bi.used_regions().count(), 1);
//         assert_eq!(bi.total_used(), 0x4000);
//     }

//     #[test_case]
//     fn test_overlay_empty_is_noop() {
//         let mut bi = BootInfo::new();
//         bi.insert_overlay_region(0x1000.into(), 0x1000.into(), default_attrs(), "init")
//             .unwrap();
//         assert_eq!(bi.count(), 0);
//     }

//     #[test_case]
//     fn test_overlay_cuts_from_free() {
//         let mut bi = BootInfo::new();
//         bi.insert_free_region(0x0.into(), 0x8000.into(), default_attrs(), "RAM")
//             .unwrap();
//         bi.insert_used_region(0x2000.into(), 0x4000.into(), default_attrs(), "kernel")
//             .unwrap();
//         // Overlay [0x1000, 0x5000) — gaps: [0x1000,0x2000) and [0x4000,0x5000)
//         bi.insert_overlay_region(0x1000.into(), 0x5000.into(), default_attrs(), "init")
//             .unwrap();
//         // Used: kernel [2000,4000) + init [1000,2000) + init [4000,5000) = 0x4000
//         assert_eq!(bi.total_used(), 0x4000);
//         // Free: [0x0,0x1000) + [0x5000,0x8000) = 0x4000
//         assert_eq!(bi.total_free(), 0x4000);
//     }
// }
