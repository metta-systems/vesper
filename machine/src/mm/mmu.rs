pub struct MMU<PS: PageSizeSupport, S: PageSizeStrategy> {
    page_table: PageTable<PS>,
    page_size_strategy: S,
}

impl<PS: PageSizeSupport, S: PageSizeStrategy> MMU<PS, S> {
    /// Map a memory range with automatically selected page size
    pub fn map_range(
        &mut self,
        virt_range: Range<VirtAddr>,
        phys_range: Range<PhysAddr>,
        flags: PageTableFlags,
    ) -> Result<(), MapError> {
        let alignment = virt_range.start.as_usize().min(phys_range.start.as_usize());

        let page_size =
            self.page_size_strategy
                .select_page_size(virt_range.clone(), alignment, flags);

        self.page_table
            .map_range(virt_range, phys_range, page_size, flags)
    }
}
