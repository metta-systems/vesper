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

pub fn invoke<A: ArchObjects>(
    frame: &mut A::Frame,
    rights: Rights,
    op: u32,
    args: &[u64; 6],
) -> Result<(u64, u64), CapError> {
    let op = FrameOp::try_from(op as u8).map_err(|_| CapError::InvalidOperation)?;

    match op {
        FrameOp::Map => {
            // args[0] = vspace_slot
            // args[1] = virt_addr
            // args[2] = attrs (R/W/X)
            if !rights.contains(Rights::READ) {
                return Err(CapError::InsufficientRights);
            }
            // Implementation depends on A::Frame
            todo!("frame map")
        }
        FrameOp::Unmap => {
            todo!("frame unmap")
        }
        FrameOp::GetAddress => {
            if !rights.contains(Rights::GRANT) {
                return Err(CapError::InsufficientRights);
            }
            // Return physical address
            todo!("frame get_address")
        }
        FrameOp::Remap => {
            todo!("frame remap")
        }
    }
}
