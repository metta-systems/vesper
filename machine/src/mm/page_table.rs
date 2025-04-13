use core::marker::PhantomData;

/// A page table entry that can represent different page sizes
#[repr(transparent)]
pub struct PageTableEntry<PS: PageSizeSupport> {
    entry: usize,
    _phantom: PhantomData<PS>,
}

impl<PS: PageSizeSupport> PageTableEntry<PS> {
    /// Create a new page table entry
    pub const fn new() -> Self {
        Self {
            entry: 0,
            _phantom: PhantomData,
        }
    }

    /// Check if this entry is valid
    pub fn is_valid(&self) -> bool {
        self.entry & 1 == 1
    }

    /// Get the page size this entry maps
    pub fn page_size(&self) -> Option<PageSize> {
        if !self.is_valid() {
            return None;
        }
        // Implementation depends on architecture-specific encoding
        unimplemented!()
    }
}

/// A page table that can handle multiple page sizes
pub struct PageTable<PS: PageSizeSupport> {
    entries: &'static mut [PageTableEntry<PS>],
    page_size_support: PS,
}

impl<PS: PageSizeSupport> PageTable<PS> {
    /// Map a virtual address range with a specific page size
    pub fn map_range(
        &mut self,
        virt_range: Range<VirtAddr>,
        phys_range: Range<PhysAddr>,
        page_size: PageSize,
        flags: PageTableFlags,
    ) -> Result<(), MapError> {
        // Validate alignment
        if !page_size.is_aligned(virt_range.start.as_usize())
            || !page_size.is_aligned(phys_range.start.as_usize())
        {
            return Err(MapError::NotAligned);
        }

        // Calculate number of pages needed
        let pages = (virt_range.end.as_usize() - virt_range.start.as_usize()) / page_size.size;

        // Map each page
        for i in 0..pages {
            let virt = virt_range.start + (i * page_size.size);
            let phys = phys_range.start + (i * page_size.size);
            self.map_page(virt, phys, page_size, flags)?;
        }

        Ok(())
    }

    /// Find the best page size for a mapping
    pub fn optimal_page_size(&self, size: usize, alignment: usize) -> PageSize {
        self.page_size_support
            .supported_sizes()
            .iter()
            .rev() // Start with largest
            .find(|ps| {
                size >= ps.size && // Mapping is at least this size
                alignment % ps.size == 0 // Alignment is compatible
            })
            .copied()
            .unwrap_or_else(|| self.page_size_support.base_page_size())
    }
}
