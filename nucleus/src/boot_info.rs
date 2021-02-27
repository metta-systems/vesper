use crate::{memory::PhysAddr, println, sync};

#[derive(Default, Copy, Clone)]
struct BootInfoMemRegion {
    pub start: PhysAddr,
    pub end: PhysAddr,
}

impl BootInfoMemRegion {
    pub const fn new() -> BootInfoMemRegion {
        BootInfoMemRegion {
            start: PhysAddr::zero(),
            end: PhysAddr::zero(),
        }
    }

    pub fn size(&self) -> u64 {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

const NUM_MEM_REGIONS: usize = 16;

pub enum BootInfoError {
    NoFreeMemRegions,
}

#[derive(Default)]
struct BootInfo {
    pub regions: [BootInfoMemRegion; NUM_MEM_REGIONS],
    pub max_slot_pos: usize,
}

impl BootInfo {
    pub const fn new() -> BootInfo {
        BootInfo {
            regions: [BootInfoMemRegion::new(); NUM_MEM_REGIONS],
            max_slot_pos: 0,
        }
    }

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
            if reg_iter.start.aligned_up(1u64 << size_bits) - reg_iter.start
                < reg_iter.end - reg_iter.end.aligned_down(1u64 << size_bits)
            {
                new_reg.start = reg_iter.start.aligned_up(1u64 << size_bits);
                new_reg.end = new_reg.start + (1u64 << size_bits);
            } else {
                new_reg.end = reg_iter.end.aligned_down(1u64 << size_bits);
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
        self.regions[reg_index] = BootInfoMemRegion::new();
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
