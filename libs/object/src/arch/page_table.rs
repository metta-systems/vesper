#[repr(u8)]
pub enum PageTableOp {
    /// Map this page table into parent table or VSpace
    Map = 0,
    /// Unmap from parent
    Unmap = 1,
}
