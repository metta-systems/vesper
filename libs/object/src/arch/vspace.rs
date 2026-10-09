#[repr(u8)]
pub enum VSpaceOp {
    /// Assign root page table
    SetRoot = 0,
    /// Assign ASID
    AssignASID = 1,
    /// Activate (switch to this address space)
    Activate = 2,
    /// Get current ASID
    GetASID = 3,
}
