pub struct AArch64PageTable;

#[repr(u8)]
pub enum PageTableOp {
    /// Map a page table into parent table
    Map = 0,
    /// Unmap from parent
    Unmap = 1,
}

pub fn invoke<A: ArchObjects>(
    pt: &mut A::PageTable,
    rights: Rights,
    op: u32,
    args: &[u64; 6],
    kernel: &mut Kernel<A>,
) -> Result<(u64, u64), CapError> {
    todo!("page_table invoke")
}
