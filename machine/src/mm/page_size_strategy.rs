/// Strategy for selecting page sizes when mapping memory
pub trait PageSizeStrategy {
    /// Select the best page size for a mapping
    fn select_page_size(
        &self,
        range: Range<VirtAddr>,
        alignment: usize,
        flags: PageTableFlags,
    ) -> PageSize;
}

/// Always use the smallest possible page size
pub struct SmallestPageStrategy;

impl PageSizeStrategy for SmallestPageStrategy {
    fn select_page_size(
        &self,
        _range: Range<VirtAddr>,
        _alignment: usize,
        _flags: PageTableFlags,
    ) -> PageSize {
        self.base_page_size()
    }
}

/// Try to use huge pages when possible
pub struct HugePagesStrategy {
    threshold: usize,
}

impl PageSizeStrategy for HugePagesStrategy {
    fn select_page_size(
        &self,
        range: Range<VirtAddr>,
        alignment: usize,
        flags: PageTableFlags,
    ) -> PageSize {
        let size = range.end.as_usize() - range.start.as_usize();

        if size >= self.threshold {
            // Try to use huge pages
            self.optimal_page_size(size, alignment)
        } else {
            // Fall back to base pages
            self.base_page_size()
        }
    }
}
