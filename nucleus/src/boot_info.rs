use crate::{memory::PhysAddr, println, sync};

// @todo These are copied from memory/mod.rs Descriptor helper structs:

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
    /// Read-only access
    ReadOnly,
    /// Read-write access
    ReadWrite,
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
    /// Disable executable code in this region
    pub execute_never: bool,
    /// This memory region is free
    pub free: bool,
}

impl Default for AttributeFields {
    fn default() -> Self {
        Self::defaulted()
    }
}

impl AttributeFields {
    pub const fn defaulted() -> Self {
        Self {
            mem_attributes: MemAttributes::CacheableDRAM,
            acc_perms: AccessPermissions::ReadWrite,
            execute_never: true,
            free: true,
        }
    }
}

/// Memory region .
#[derive(Default, Copy, Clone)]
pub struct BootInfoMemRegion {
    pub start: PhysAddr, // start is inclusive
    pub end: PhysAddr,   // end is exclusive
    pub attributes: AttributeFields,
}

impl BootInfoMemRegion {
    pub const fn new() -> Self {
        Self {
            start: PhysAddr::zero(),
            end: PhysAddr::zero(),
            attributes: AttributeFields::defaulted(),
        }
    }

    pub fn at(start: PhysAddr, end: PhysAddr, free: bool) -> Self {
        use core::default::default;
        Self {
            start,
            end,
            attributes: AttributeFields { free, ..default() },
        }
    }

    pub fn size(&self) -> u64 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    pub fn empty(&mut self) {
        *self = Self::new();
    }

    pub fn intersects(&self, other: &BootInfoMemRegion) -> bool {
        // https://eli.thegreenplace.net/2008/08/15/intersection-of-1d-segments/
        self.end >= other.start && other.end >= self.start
    }
}

const NUM_MEM_REGIONS: usize = 256;

pub enum BootInfoError {
    NoFreeMemRegions,
}

pub struct BootInfo {
    pub regions: [BootInfoMemRegion; NUM_MEM_REGIONS],
    pub max_slot_pos: usize,
}

// Implement Default manually to work around stupid Rust idea of not defining Default for arrays over 32 entries in size
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
    pub const fn new() -> BootInfo {
        BootInfo {
            regions: [BootInfoMemRegion::new(); NUM_MEM_REGIONS],
            max_slot_pos: 0,
        }
    }

    // Add a free memory region.
    pub fn insert_region(&mut self, reg: BootInfoMemRegion) -> Result<(), BootInfoError> {
        if reg.is_empty() {
            return Ok(());
        }
        assert!(reg.start <= reg.end);
        for region in self.regions.iter_mut() {
            if region.is_empty() {
                *region = reg;
                return Ok(());
            }
        }
        return Err(BootInfoError::NoFreeMemRegions);
    }

    // Remove a free memory region, turning it into allocated one.
    // Different from alloc_region() in that we have a specific address and size to remove.
    pub fn remove_region(&mut self, reg: BootInfoMemRegion) -> Result<(), BootInfoError> {
        // Find intersection with existing regions.
        // Subtracted region may intersect zero or more regions.
        // Regions are not sorted in the list, so it may overlap any region at any point.
        for (i, reg_iter) in self.regions.iter().enumerate() {
            if reg_iter.start == reg.start {
                // it may either cut off a piece from the start or completely eat the region
            } else if reg.intersects(reg_iter) {
                // they have common points, which must be resolved
                /// it may intersect over the beginning of the region
                if reg.start <= reg_iter.start && reg.end < reg_iter.end {
                    self.regions[i].start = reg.end; // end inclusive here?
                }
                /// it may intersect entirely inside the region, in which case we stop iterating
                if reg.start > reg_iter.start && reg.end < reg_iter.end {
                    // split current region in two parts
                    let mut first_region = BootInfoMemRegion::at(reg_iter.start, reg.start, true);
                    let mut second_region = BootInfoMemRegion::at(reg.end, reg_iter.end, true);
                    self.regions[i].empty();
                    if first_region.size() > second_region.size() {
                        self.insert_region(first_region);
                        return self.insert_region(second_region);
                    } else {
                        self.insert_region(second_region);
                        return self.insert_region(first_region);
                    }
                }
                /// it may intersect over the end of the region
                if reg.start > reg_iter.start && reg.end > reg_iter.end {
                    self.regions[i].end = reg.start;
                }
                /// or it may entirely subsume the reg_iter
                if reg.start <= reg_iter.start && reg.end >= reg_iter.end {
                    self.regions[i].empty();
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
            if reg_iter.start.aligned_up(1usize << size_bits) - reg_iter.start
                < reg_iter.end - reg_iter.end.aligned_down(1usize << size_bits)
            {
                new_reg.start = reg_iter.start.aligned_up(1usize << size_bits);
                new_reg.end = new_reg.start + (1u64 << size_bits);
            } else {
                new_reg.end = reg_iter.end.aligned_down(1usize << size_bits);
                new_reg.start = new_reg.end - (1u64 << size_bits);
            }
            if new_reg.end > new_reg.start
                && new_reg.start >= reg_iter.start
                && new_reg.end <= reg_iter.end
            {
                let mut new_rem_small: BootInfoMemRegion = BootInfoMemRegion::new();
                let mut new_rem_large: BootInfoMemRegion = BootInfoMemRegion::new();

                if new_reg.start - reg_iter.start < reg_iter.end - new_reg.end {
                    new_rem_small.start = reg_iter.start;
                    new_rem_small.end = new_reg.start;
                    new_rem_large.start = new_reg.end;
                    new_rem_large.end = reg_iter.end;
                } else {
                    new_rem_large.start = reg_iter.start;
                    new_rem_large.end = new_reg.start;
                    new_rem_small.start = new_reg.end;
                    new_rem_small.end = reg_iter.end;
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
        self.regions[reg_index].empty();
        /* Add the remaining regions in largest to smallest order */
        self.insert_region(rem_large)?;
        if self.insert_region(rem_small).is_err() {
            println!("BootInfo::alloc_region(): wasted {} bytes due to alignment, try to increase NUM_MEM_REGIONS", rem_small.size());
        }
        Ok(reg.start)
    }
}

#[link_section = ".data.boot"] // @todo put zero-initialized stuff to .bss.boot!
static BOOT_INFO: sync::NullLock<BootInfo> = sync::NullLock::new(BootInfo::new());
