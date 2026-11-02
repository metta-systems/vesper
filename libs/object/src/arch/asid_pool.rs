#[repr(u8)]
pub enum ASIDPoolOp {
    /// Allocate an ASID from this pool
    Allocate = 0,
}
