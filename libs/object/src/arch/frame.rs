#[repr(u8)]
pub enum FrameOp {
    /// Map frame into a VSpace at given virtual address
    Map = 0,
    /// Unmap frame from VSpace
    Unmap = 1,
    /// Get physical address (requires special rights)
    GetAddress = 2,
    /// Remap with different attributes
    Remap = 3,
}
