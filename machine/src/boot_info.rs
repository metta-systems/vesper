//! Boot regions
//!
//! Define a map of memory regions used during boot allocations.
use {
    crate::{arch::memory::PhysAddr, println, sync},
    core::fmt,
    once_cell::unsync::Lazy,
    snafu::Snafu,
};

// @fixme These are copied from memory/mod.rs Descriptor helper structs:

/// Memory region attributes.
#[derive(Copy, Clone)]
pub enum MemAttributes {
    /// Regular memory
    CacheableDRAM,
    /// Memory without caching
    NonCacheableDRAM,
    /// Device memory
    Device,
}

/// Memory region access permissions.
#[derive(Copy, Clone)]
pub enum AccessPermissions {
    /// Read-write access
    ReadWrite,
    /// Read-only access
    ReadOnly,
}

/// Memory region translation.
#[allow(dead_code)]
#[derive(Copy, Clone)]
pub enum Translation {
    /// One-to-one address mapping
    Identity,
    /// Mapping with a specified offset
    Offset(usize),
}

/// Summary structure of memory region properties.
#[derive(Copy, Clone)]
pub struct AttributeFields {
    /// Attributes
    pub mem_attributes: MemAttributes,
    /// Permissions
    pub acc_perms: AccessPermissions,
    /// Allow executable code in this region
    pub executable: bool,
    /// This memory region is occupied
    pub occupied: bool,
}

impl Default for AttributeFields {
    fn default() -> Self {
        Self::defaulted()
    }
}

impl AttributeFields {
    /// Create zero-initialized attribute structure.
    pub const fn defaulted() -> Self {
        Self {
            mem_attributes: MemAttributes::CacheableDRAM,
            acc_perms: AccessPermissions::ReadWrite,
            executable: false,
            occupied: false,
        }
    }
}

impl fmt::Debug for MemAttributes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let attr = match self {
            MemAttributes::CacheableDRAM => "C",
            MemAttributes::NonCacheableDRAM => "NC",
            MemAttributes::Device => "Dev",
        };
        write!(f, "{: <3}", attr)
    }
}

impl fmt::Debug for AccessPermissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let acc_p = match self {
            AccessPermissions::ReadOnly => "RO",
            AccessPermissions::ReadWrite => "RW",
        };
        write!(f, "{}", acc_p)
    }
}

impl fmt::Debug for AttributeFields {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AttributeFields")
            .field("mem_attributes", &self.mem_attributes)
            .field("acc_perms", &self.acc_perms)
            .field("executable", &self.executable)
            .field("occupied", &self.occupied)
            .finish()
    }
}

//=================================================================================================
// BootInfoMemRegion
//=================================================================================================

/// Memory region .
#[derive(Default, Copy, Clone, Debug)]
pub struct BootInfoMemRegion {
    /// Region start is inclusive.
    pub start_inclusive: PhysAddr,
    /// Region end is exclusive.
    pub end_exclusive: PhysAddr,
    pub attributes: AttributeFields,
}

impl BootInfoMemRegion {
    /// Create an empty region.
    pub const fn new() -> Self {
        Self {
            start_inclusive: PhysAddr::zero(),
            end_exclusive: PhysAddr::zero(),
            attributes: AttributeFields::defaulted(),
        }
    }

    /// Create an occupied or free region with start and end.
    /// Region is in range [start, end), that is, for start 0x0 and end 0x2000 the region will
    /// occupy memory between addresses 0x0 and 0x1fff.
    pub fn at(start_inclusive: PhysAddr, end_exclusive: PhysAddr, free: bool) -> Self {
        Self {
            start_inclusive: start_inclusive.min(end_exclusive),
            end_exclusive: end_exclusive.max(start_inclusive),
            attributes: AttributeFields {
                occupied: !free,
                ..core::default::default()
            },
        }
    }

    /// Calculate region size.
    pub fn size(&self) -> u64 {
        self.end_exclusive - self.start_inclusive
    }

    /// Is this region empty?
    pub fn is_empty(&self) -> bool {
        self.start_inclusive == self.end_exclusive
    }

    /// Clear the region to empty.
    pub fn clear(&mut self) {
        *self = Self::new();
    }

    /// Does this region intersect the given one?
    /// Based on [Intersection of 1D segments](https://eli.thegreenplace.net/2008/08/15/intersection-of-1d-segments/).
    ///
    /// Since end is exclusive, the actual value is one less than what it contains, for this reason,
    /// end equal to other's start means they touch but do not intersect.
    ///
    /// Assumes start_inclusive <= end_exclusive, which holds for memory regions by construction.
    pub fn intersects(&self, other: &BootInfoMemRegion) -> bool {
        self.end_exclusive > other.start_inclusive && other.end_exclusive > self.start_inclusive
    }
}

impl fmt::Display for BootInfoMemRegion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // log2(1024)
        const KIB_RSHIFT: u32 = 10;

        // log2(1024 * 1024)
        const MIB_RSHIFT: u32 = 20;

        let size = self.size();

        let (size, unit) = if (size >> MIB_RSHIFT) > 0 {
            (size >> MIB_RSHIFT, "MiB")
        } else if (size >> KIB_RSHIFT) > 0 {
            (size >> KIB_RSHIFT, "KiB")
        } else {
            (size, "B")
        };

        let attr = match self.attributes.mem_attributes {
            MemAttributes::CacheableDRAM => "C",
            MemAttributes::NonCacheableDRAM => "NC",
            MemAttributes::Device => "Dev",
        };

        let acc_p = match self.attributes.acc_perms {
            AccessPermissions::ReadOnly => "RO",
            AccessPermissions::ReadWrite => "RW",
        };

        let xn = if self.attributes.executable {
            "PX"
        } else {
            "PXN"
        };

        write!(
            f,
            "      {:#010X} - {:#010X} | {: >3} {} | {: <3} {} {: <3}", // | {}",
            self.start_inclusive,
            self.end_exclusive,
            size,
            unit,
            attr,
            acc_p,
            xn, //self.name
        )
    }
}

#[cfg(test)]
mod boot_info_region_tests {
    use super::*;

    #[test_case]
    fn test_construct_regular_region() {
        let region = BootInfoMemRegion::at(0x0.into(), 0x2000.into(), true);
        assert_eq!(region.start_inclusive, 0x0);
        assert_eq!(region.end_exclusive, 0x2000);
        assert_eq!(region.size(), 0x2000);
        assert_eq!(region.attributes.occupied, false);
    }

    #[test_case]
    fn test_construct_reverse_region() {
        let region = BootInfoMemRegion::at(0x2000.into(), 0x0.into(), true);
        assert_eq!(region.start_inclusive, 0x0);
        assert_eq!(region.end_exclusive, 0x2000);
        assert_eq!(region.size(), 0x2000);
        assert_eq!(region.attributes.occupied, false);
    }

    #[test_case]
    fn test_regions_touch() {
        let region1 = BootInfoMemRegion::at(0x0.into(), 0x2000.into(), false);
        let region2 = BootInfoMemRegion::at(0x2000.into(), 0x4000.into(), false);
        assert_eq!(region1.intersects(&region2), false);
        assert_eq!(region2.intersects(&region1), false);
    }

    #[test_case]
    fn test_regions_intersect() {
        let region1 = BootInfoMemRegion::at(0x0.into(), 0x2000.into(), false);
        let region2 = BootInfoMemRegion::at(0x1000.into(), 0x3000.into(), false);
        assert_eq!(region1.intersects(&region2), true);
        assert_eq!(region2.intersects(&region1), true);
    }

    #[test_case]
    fn test_self_intersect() {
        let region1 = BootInfoMemRegion::at(0x0.into(), 0x2000.into(), false);
        let region1 = BootInfoMemRegion::at(0x0.into(), 0x2000.into(), false);
        assert_eq!(region1.intersects(&region2), true);
        assert_eq!(region2.intersects(&region1), true);
    }

    #[test_case]
    fn test_regions_fully_overlap() {}
}

//=================================================================================================
// BootInfo
//=================================================================================================

const NUM_MEM_REGIONS: usize = 256;

#[derive(Snafu, Debug)]
pub enum BootInfoError {
    NoFreeMemRegions,
    InvalidRegion,
}

pub struct BootInfo {
    pub regions: [BootInfoMemRegion; NUM_MEM_REGIONS],
    pub max_slot_pos: usize,
}

/// Implement Default manually to work around stupid Rust idea of not defining Default for arrays over 32 entries in size
impl Default for BootInfo {
    fn default() -> Self {
        Self::new()
    }
}

// @todo
// - use boot info to mark regions that are usable and not usable
// - build full memory map from the addresses we know and the DTB
// - print the derived memory layout

impl BootInfo {
    /// Create empty boot region map.
    pub const fn new() -> BootInfo {
        BootInfo {
            regions: [BootInfoMemRegion::new(); NUM_MEM_REGIONS],
            max_slot_pos: 0,
        }
    }

    /// Add a free memory region.
    pub fn insert_region(&mut self, reg: BootInfoMemRegion) -> Result<(), BootInfoError> {
        if reg.is_empty() {
            return Ok(());
        }
        if reg.start_inclusive > reg.end_exclusive {
            return Err(BootInfoError::InvalidRegion);
        }
        for region in self.regions.iter_mut() {
            if region.is_empty() {
                *region = reg;
                return Ok(());
            }
        }
        return Err(BootInfoError::NoFreeMemRegions);
    }

    /// Remove a free memory region, turning it into an allocated one.
    /// Different from alloc_region() in that we have a specific address and size to remove.
    pub fn remove_region(&mut self, remove_region: BootInfoMemRegion) -> Result<(), BootInfoError> {
        // Find intersection with existing regions.
        // Subtracted region may intersect zero or more regions.
        // Regions are not sorted in the list, so it may overlap any region at any point.
        for iterated_region in self.regions.iter_mut() {
            if iterated_region.start_inclusive == remove_region.start_inclusive {
                // it may either cut off a piece from the start or completely eat the region
                if remove_region.end_exclusive >= iterated_region.end_exclusive {
                    iterated_region.clear();
                } else {
                    iterated_region.start_inclusive = remove_region.end_exclusive;
                    return Ok(());
                }
            } else if remove_region.intersects(iterated_region) {
                // they have common points, which must be resolved
                // it may intersect over the beginning of the region
                if remove_region.start_inclusive <= iterated_region.start_inclusive
                    && remove_region.end_exclusive < iterated_region.end_exclusive
                {
                    iterated_region.start_inclusive = remove_region.end_exclusive;
                    // @todo end inclusive here?
                }
                // it may intersect entirely inside the region, in which case we stop iterating
                if remove_region.start_inclusive > iterated_region.start_inclusive
                    && remove_region.end_exclusive < iterated_region.end_exclusive
                {
                    // split current region in two parts
                    let first_region = BootInfoMemRegion::at(
                        iterated_region.start_inclusive,
                        remove_region.start_inclusive,
                        true,
                    );
                    let second_region = BootInfoMemRegion::at(
                        remove_region.end_exclusive,
                        iterated_region.end_exclusive,
                        true,
                    );
                    iterated_region.clear();
                    if first_region.size() > second_region.size() {
                        self.insert_region(first_region)?;
                        return self.insert_region(second_region);
                    } else {
                        self.insert_region(second_region)?;
                        return self.insert_region(first_region);
                    }
                }
                // it may intersect over the end of the region
                if remove_region.start_inclusive > iterated_region.start_inclusive
                    && remove_region.end_exclusive >= iterated_region.end_exclusive
                {
                    iterated_region.end_exclusive = remove_region.start_inclusive;
                }
                // or it may entirely subsume the reg_iter
                if remove_region.start_inclusive <= iterated_region.start_inclusive
                    && remove_region.end_exclusive >= iterated_region.end_exclusive
                {
                    iterated_region.clear();
                    // it could also touch adjacent regions, so continue.
                }
            } else {
                // no intersection and we can continue
            }
        }
        Ok(())
    }

    // this method assumes all non-empty regions in BootInfo represent free memory
    pub fn alloc_region(&mut self, size_bits: usize) -> Result<PhysAddr, BootInfoError> {
        let mut reg_index: usize = 0;
        let mut reg: BootInfoMemRegion = BootInfoMemRegion::new();
        let mut rem_small: BootInfoMemRegion = BootInfoMemRegion::new();
        let mut rem_large: BootInfoMemRegion = BootInfoMemRegion::new();
        /*
         * Search for a free mem region that will be the best fit for an allocation. We favour allocations
         * that are aligned to either end of the region. If an allocation must split a region we favour
         * an unbalanced split. In both cases we attempt to use the smallest region possible. In general
         * this means we aim to make the size of the smallest remaining region smaller (ideally zero)
         * followed by making the size of the largest remaining region smaller.
         */
        for (i, reg_iter) in self.regions.iter().enumerate() {
            let mut new_reg: BootInfoMemRegion = BootInfoMemRegion::new();

            /* Determine whether placing the region at the start or the end will create a bigger left over region */
            if reg_iter.start_inclusive.aligned_up(1u64 << size_bits) - reg_iter.start_inclusive
                < reg_iter.end_exclusive - reg_iter.end_exclusive.aligned_down(1u64 << size_bits)
            {
                new_reg.start_inclusive = reg_iter.start_inclusive.aligned_up(1u64 << size_bits);
                new_reg.end_exclusive = new_reg.start_inclusive + (1u64 << size_bits);
            } else {
                new_reg.end_exclusive = reg_iter.end_exclusive.aligned_down(1u64 << size_bits);
                new_reg.start_inclusive = new_reg.end_exclusive - (1u64 << size_bits);
            }
            if new_reg.end_exclusive > new_reg.start_inclusive
                && new_reg.start_inclusive >= reg_iter.start_inclusive
                && new_reg.end_exclusive <= reg_iter.end_exclusive
            {
                let mut new_rem_small: BootInfoMemRegion = BootInfoMemRegion::new();
                let mut new_rem_large: BootInfoMemRegion = BootInfoMemRegion::new();

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
            panic!("Kernel init failed: not enough memory\n");
        }
        /* Remove the region in question */
        self.regions[reg_index].clear();
        /* Add the remaining regions in largest to smallest order */
        self.insert_region(rem_large)?;
        if self.insert_region(rem_small).is_err() {
            println!("BootInfo::alloc_region(): wasted {} bytes due to alignment, try to increase NUM_MEM_REGIONS", rem_small.size());
        }
        Ok(reg.start_inclusive)
    }
}

// #[link_section = ".bss.boot"]
pub static BOOT_INFO: sync::NullLock<Lazy<BootInfo>> =
    sync::NullLock::new(Lazy::new(|| BootInfo::new()));

#[cfg(test)]
mod boot_info_tests {
    use super::*;

    #[test_case]
    fn test_add_invalid_region() {
        let mut bi = BootInfo::new();
        let region = BootInfoMemRegion::at(0x2000.into(), 0x0.into(), true);
        let res = bi.insert_region(region);
        assert!(res.is_err());
        assert_eq!(res.err(), Some(BootInfoError::InvalidRegion));
    }
}
