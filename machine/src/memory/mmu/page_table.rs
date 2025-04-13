use {
    super::{Address, AddressType, PageSize, PageSizeSupport, Physical, Virtual},
    crate::synchronization::{InitStateLock, interface::ReadWriteEx},
    core::{fmt, marker::PhantomData},
    snafu::Snafu,
};

#[derive(Debug, Snafu)]
pub enum MapError {
    #[snafu(display("Address not aligned to page size"))]
    NotAligned,
    #[snafu(display("Page already mapped"))]
    AlreadyMapped,
    #[snafu(display("Invalid page size"))]
    InvalidPageSize,
}

/// Page table entry flags
#[derive(Copy, Clone, Debug, Default)]
pub struct PageTableFlags {
    pub writable: bool,
    pub executable: bool,
    pub user_accessible: bool,
    pub cacheable: bool,
}

/// A page table entry that can represent different page sizes
#[repr(transparent)]
pub struct PageTableEntry<PS: PageSizeSupport> {
    entry: usize,
    _phantom: PhantomData<PS>,
}

/// A page table that can handle multiple page sizes
pub struct PageTable<PS: PageSizeSupport> {
    entries: InitStateLock<&'static mut [PageTableEntry<PS>]>,
    page_size_support: PS,
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

impl<PS: PageSizeSupport> PageTable<PS> {
    /// Create a new page table
    pub const fn new(page_size_support: PS) -> Self {
        Self {
            entries: InitStateLock::new(&mut []),
            page_size_support,
        }
    }

    /// Map a virtual address range to a physical address range
    pub fn map_range(
        &self,
        virt_range: (Address<Virtual>, Address<Virtual>),
        phys_range: (Address<Physical>, Address<Physical>),
        page_size: PageSize,
        flags: PageTableFlags,
    ) -> Result<(), MapError> {
        // Validate alignment
        if !virt_range.0.is_aligned(page_size) || !phys_range.0.is_aligned(page_size) {
            return Err(MapError::NotAligned);
        }

        // Validate page size
        if !self
            .page_size_support
            .supported_sizes()
            .contains(&page_size)
        {
            return Err(MapError::InvalidPageSize);
        }

        // Calculate number of pages
        let size = virt_range.1.as_usize() - virt_range.0.as_usize();
        let pages = size / page_size.size;

        self.entries.write(|entries| {
            // Map each page
            for i in 0..pages {
                let virt = virt_range.0 + (i * page_size.size);
                let phys = phys_range.0 + (i * page_size.size);

                let idx = self.index_for_address(virt, page_size);
                if entries[idx].is_valid() {
                    return Err(MapError::AlreadyMapped);
                }

                entries[idx] = self.make_entry(phys, page_size, flags);
            }
            Ok(())
        })
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

    // Internal helper methods
    fn index_for_address(&self, addr: Address<Virtual>, page_size: PageSize) -> usize {
        (addr.as_usize() >> page_size.offset_bits) & ((1 << page_size.level) - 1)
    }

    fn make_entry(
        &self,
        phys: Address<Physical>,
        page_size: PageSize,
        flags: PageTableFlags,
    ) -> PageTableEntry<PS> {
        // Architecture-specific implementation
        unimplemented!()
    }
}
